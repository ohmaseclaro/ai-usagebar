//! Change detection and the dry-run plan (SYNC-01/02/03).
//!
//! Three claims live here, in increasing order of how easy they are to get
//! wrong:
//!
//! 1. **Only changed data uploads.** A file whose D5 tuple
//!    `(path, size, mtime_ns, inode)` still matches the index is never opened;
//!    its stored chunk ids are reused as-is.
//! 2. **A no-op sync uploads nothing.** That is the same branch, and it is
//!    asserted here by [`SyncPlan::files_opened`] being exactly zero — a
//!    counter, not a stopwatch.
//! 3. **Appending to a large file uploads roughly the appended bytes.** An
//!    append displaces no byte below the old end-of-file, so every fully
//!    contained 256 KiB chunk keeps its hash. That is a property of *appends*,
//!    though, not of every write that happens to grow a file: from
//!    `(size, mtime, inode)` alone a rewrite is indistinguishable from an
//!    append. So it is **verified, never assumed** — the last sealed chunk is
//!    re-hashed before its predecessors are reused, and a mismatch falls back
//!    to a full re-chunk. [`SyncPlan::append_check_miss_bytes`] is what that
//!    check costs when it fails, so the "fixed chunks, no CDC" decision stays
//!    measurable in the field rather than merely argued.
//!
//! This module never compresses and never encrypts. Chunk ids come from an
//! injected function so the planner is testable without Phase 1's keys; plan
//! 2-07 supplies the real `blake3::keyed_hash(name_key, plaintext)` at the one
//! call site.

use std::collections::HashSet;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::config::{SyncCategory, SyncConfig};
use crate::error::{AppError, Result};
use crate::sync::index::{FileRecord, Index};
use crate::sync::scope::{self, FileEntry};
use crate::sync::{CHUNK_SIZE, SyncRoots, chunk};

/// Phase 1's fixed chunk size, as the `u64` the offset arithmetic here wants.
///
/// A re-export rather than a second literal: two modules agreeing on 256 KiB by
/// coincidence is a bug that only shows up as a bundle nobody can re-chunk.
pub const CHUNK_BYTES: u64 = CHUNK_SIZE as u64;

/// What one file contributes to a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePlan {
    pub path: PathBuf,
    /// Every chunk of the file, in order — reused and fresh alike.
    pub chunk_ids: Vec<[u8; 32]>,
    /// Count of full [`CHUNK_BYTES`] chunks; the remainder is the tail.
    pub sealed_chunks: u64,
    /// The subset of `chunk_ids` this run is the first to see.
    pub new_chunk_ids: Vec<[u8; 32]>,
    /// Plaintext bytes of `new_chunk_ids`.
    pub new_bytes: u64,
    /// True when the file was never opened — the D5 short-circuit hit.
    pub reused: bool,
}

/// D4's per-category line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryPlan {
    pub category: SyncCategory,
    pub files: usize,
    pub raw_bytes: u64,
    pub new_bytes: u64,
    /// Bound-dropped files (transcripts only); structurally zero elsewhere.
    pub excluded_files: usize,
    pub excluded_bytes: u64,
}

/// What a push would send, and what it cost to work that out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncPlan {
    pub categories: Vec<CategoryPlan>,
    /// Deduplicated across every file: two identical chunks upload once.
    pub new_chunk_ids: Vec<[u8; 32]>,
    pub total_raw_bytes: u64,
    pub total_new_bytes: u64,
    /// Files whose body was read. Zero on a true no-op — this is SYNC-02's
    /// evidence, and the only place a `File::open` is counted.
    pub files_opened: usize,
    /// Bytes re-read by an append check that then failed. Non-trivial values
    /// here are the signal that fixed-size chunking has stopped paying.
    pub append_check_miss_bytes: u64,
    /// The index was unreadable and rebuilt, so everything reads as changed.
    pub index_rebuilt: bool,
    /// Per-file detail, in scan order. Phase 4 builds its manifest from this;
    /// `categories` carries the same bytes already aggregated.
    pub file_plans: Vec<FilePlan>,
}

impl SyncPlan {
    /// Nothing to upload.
    pub fn is_empty(&self) -> bool {
        self.new_chunk_ids.is_empty()
    }
}

/// What the walk actually cost. Private: these are measurements, and the
/// public surface exposes only the two that mean something to a user.
#[derive(Debug, Default, PartialEq, Eq)]
struct Counters {
    files_opened: usize,
    bytes_read: u64,
    append_check_miss_bytes: u64,
}

/// Plan a sync over `roots`.
///
/// Generic over the chunk-id function rather than taking a `fn` pointer,
/// because Phase 1's is `blake3::keyed_hash(name_key, plaintext)` and the key
/// has to be captured.
///
/// Mutates `index`: rows for unchanged files are touched, changed files are
/// re-recorded, and the generation is bumped once. Nothing else is written
/// anywhere, and nothing is transmitted.
pub fn build<F: Fn(&[u8]) -> [u8; 32]>(
    roots: &SyncRoots,
    cfg: &SyncConfig,
    index: &Index,
    now: DateTime<Utc>,
    chunk_id: F,
) -> Result<SyncPlan> {
    // Before any touch/record, so both stamp the run's own generation.
    index.bump_generation()?;

    let mut counters = Counters::default();
    let mut known: HashSet<[u8; 32]> = HashSet::new();
    let mut plan = SyncPlan {
        categories: Vec::new(),
        new_chunk_ids: Vec::new(),
        total_raw_bytes: 0,
        total_new_bytes: 0,
        files_opened: 0,
        append_check_miss_bytes: 0,
        index_rebuilt: index.was_rebuilt(),
        file_plans: Vec::new(),
    };

    for category in SyncCategory::ALL {
        let scan = scope::collect(category, roots, cfg, now);
        let mut new_bytes = 0u64;

        // Pass one: the D5 short-circuit. `lookup` returning Some ends the work
        // for that file — no open, no read, no hash. Doing every hit first also
        // means their chunks are known before any changed file is hashed, so a
        // changed file that happens to share a chunk with an unchanged one is
        // not counted as new because of scan order.
        let mut changed: Vec<&FileEntry> = Vec::new();
        for entry in &scan.files {
            match index.lookup(entry) {
                Some(record) => {
                    known.extend(record.chunk_ids.iter().copied());
                    index.touch(&entry.path)?;
                    plan.file_plans.push(FilePlan {
                        path: entry.path.clone(),
                        chunk_ids: record.chunk_ids,
                        sealed_chunks: record.sealed_chunks,
                        new_chunk_ids: Vec::new(),
                        new_bytes: 0,
                        reused: true,
                    });
                }
                None => changed.push(entry),
            }
        }

        // Pass two: the slow path, the only one that opens a file.
        for entry in changed {
            let file_plan = plan_file(
                entry,
                index.cached(&entry.path),
                &chunk_id,
                &mut known,
                &mut counters,
            )?;
            index.record(entry, file_plan.sealed_chunks, &file_plan.chunk_ids)?;
            new_bytes += file_plan.new_bytes;
            plan.new_chunk_ids
                .extend(file_plan.new_chunk_ids.iter().copied());
            plan.file_plans.push(file_plan);
        }

        plan.categories.push(CategoryPlan {
            category,
            files: scan.files.len(),
            raw_bytes: scan.bytes,
            new_bytes,
            excluded_files: scan.excluded_files,
            excluded_bytes: scan.excluded_bytes,
        });
        plan.total_raw_bytes = plan.total_raw_bytes.saturating_add(scan.bytes);
        plan.total_new_bytes = plan.total_new_bytes.saturating_add(new_bytes);
    }

    plan.files_opened = counters.files_opened;
    plan.append_check_miss_bytes = counters.append_check_miss_bytes;
    Ok(plan)
}

/// Chunk one changed file, taking the append fast path when it verifies.
fn plan_file<F: Fn(&[u8]) -> [u8; 32]>(
    entry: &FileEntry,
    cached: Option<FileRecord>,
    chunk_id: &F,
    known: &mut HashSet<[u8; 32]>,
    counters: &mut Counters,
) -> Result<FilePlan> {
    let path = entry.path.as_path();
    let mut file = io_at(path, File::open(path))?;
    // The one place a file body is opened, so `files_opened` cannot drift from
    // what actually happened.
    counters.files_opened += 1;

    let reused = verified_prefix(&mut file, path, entry.size, cached, chunk_id, counters)?;
    let from = (reused.len() as u64).saturating_mul(CHUNK_BYTES);
    let fresh = chunk_from(&mut file, path, from, chunk_id, counters)?;

    let mut chunk_ids = reused;
    // Reused ids are already uploaded by construction — known, never new.
    known.extend(chunk_ids.iter().copied());

    let mut new_chunk_ids = Vec::new();
    let mut new_bytes = 0u64;
    let mut size = from;
    for (id, len) in fresh {
        chunk_ids.push(id);
        size = size.saturating_add(len);
        if known.insert(id) {
            new_chunk_ids.push(id);
            new_bytes = new_bytes.saturating_add(len);
        }
    }

    Ok(FilePlan {
        path: path.to_path_buf(),
        // From what was actually read, not from the stat: a file that grew
        // between the scan and the read must still record a consistent row.
        sealed_chunks: chunk::sealed_chunk_count(size),
        chunk_ids,
        new_chunk_ids,
        new_bytes,
        reused: false,
    })
}

/// The cached chunk ids that a re-hash of the last sealed chunk proves are
/// still correct, or empty when the file must be re-chunked whole.
///
/// This one 256 KiB read is what makes fixed-size chunking safe here without
/// content-defined chunking. A shrink skips it (there is nothing to verify
/// against); a mismatch bills the read to `append_check_miss_bytes`.
fn verified_prefix<F: Fn(&[u8]) -> [u8; 32]>(
    file: &mut File,
    path: &Path,
    size: u64,
    cached: Option<FileRecord>,
    chunk_id: &F,
    counters: &mut Counters,
) -> Result<Vec<[u8; 32]>> {
    let none = Vec::new();
    let Some(cached) = cached else {
        return Ok(none);
    };
    let Ok(sealed) = usize::try_from(cached.sealed_chunks) else {
        return Ok(none);
    };
    // A row claiming more sealed chunks than it stores ids for is a corrupt
    // hint; re-chunk rather than reuse a short list.
    if sealed == 0 || cached.chunk_ids.len() < sealed {
        return Ok(none);
    }
    let sealed_end = cached.sealed_chunks.saturating_mul(CHUNK_BYTES);
    if size < sealed_end {
        return Ok(none); // truncated, or rewritten shorter
    }

    let probe = read_chunk_at(file, path, sealed_end - CHUNK_BYTES, counters)?;
    if probe.len() as u64 == CHUNK_BYTES && chunk_id(&probe) == cached.chunk_ids[sealed - 1] {
        return Ok(cached.chunk_ids[..sealed].to_vec());
    }
    counters.append_check_miss_bytes = counters
        .append_check_miss_bytes
        .saturating_add(probe.len() as u64);
    Ok(none)
}

/// Hash `path` from `from` to EOF in [`CHUNK_BYTES`] buffers, yielding
/// `(id, plaintext_len)` per chunk. Never holds more than one chunk, so a
/// 50 MB transcript is never resident.
fn chunk_from<F: Fn(&[u8]) -> [u8; 32]>(
    file: &mut File,
    path: &Path,
    from: u64,
    chunk_id: &F,
    counters: &mut Counters,
) -> Result<Vec<([u8; 32], u64)>> {
    io_at(path, file.seek(SeekFrom::Start(from)))?;
    let mut out = Vec::new();
    let mut buf = Vec::with_capacity(CHUNK_BYTES as usize);
    loop {
        buf.clear();
        io_at(path, file.by_ref().take(CHUNK_BYTES).read_to_end(&mut buf))?;
        if buf.is_empty() {
            break;
        }
        counters.bytes_read = counters.bytes_read.saturating_add(buf.len() as u64);
        out.push((chunk_id(&buf), buf.len() as u64));
        if (buf.len() as u64) < CHUNK_BYTES {
            break; // short read means EOF: `Take::read_to_end` fills otherwise
        }
    }
    Ok(out)
}

fn read_chunk_at(
    file: &mut File,
    path: &Path,
    offset: u64,
    counters: &mut Counters,
) -> Result<Vec<u8>> {
    io_at(path, file.seek(SeekFrom::Start(offset)))?;
    let mut buf = Vec::with_capacity(CHUNK_BYTES as usize);
    io_at(path, file.by_ref().take(CHUNK_BYTES).read_to_end(&mut buf))?;
    counters.bytes_read = counters.bytes_read.saturating_add(buf.len() as u64);
    Ok(buf)
}

/// Errors name the path and nothing else — no body, no chunk id (T-2-22).
fn io_at<T>(path: &Path, result: io::Result<T>) -> Result<T> {
    result.map_err(|source| AppError::Io {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::scope::CategoryScan;
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::time::{Duration, SystemTime};
    use tempfile::TempDir;

    /// A 5 MiB fixture, not the roadmap's 50 MB one: 20 sealed chunks instead
    /// of 190 exercises exactly the same arithmetic, and the AUR `check()`
    /// runs this on every installer's machine. Deliberately *not* a multiple
    /// of `CHUNK_BYTES`, so the fixture has a real tail for an append to seal.
    const FIXTURE_BYTES: usize = 5 * 1024 * 1024 + 100 * 1024;
    const APPEND_BYTES: usize = 200 * 1024;

    /// Stand-in for Phase 1's keyed BLAKE3: content- and length-dependent,
    /// which is all this module's logic depends on.
    fn toy_id(data: &[u8]) -> [u8; 32] {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in data {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        let mut id = [0u8; 32];
        id[..8].copy_from_slice(&h.to_le_bytes());
        id[8..16].copy_from_slice(&(data.len() as u64).to_le_bytes());
        id
    }

    /// Deterministic bytes that vary over any window, so no two 256 KiB chunks
    /// collide and dedup cannot mask a counting bug.
    fn pseudo(len: usize, seed: u64) -> Vec<u8> {
        let mut s = seed | 1;
        (0..len)
            .map(|_| {
                s = s
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                (s >> 33) as u8
            })
            .collect()
    }

    fn roots_at(dir: &Path) -> SyncRoots {
        SyncRoots::at(
            dir.join("config/config.toml"),
            dir.join("config"),
            dir.join("desktop"),
            dir.join("profiles"),
            dir.join("claude"),
        )
    }

    /// Only the `config` category, so a test tree is two files and no test
    /// depends on another collector's shape.
    fn cfg() -> SyncConfig {
        SyncConfig {
            categories: vec![SyncCategory::Config],
            transcript_days: 30,
            transcript_max_bytes: 0,
        }
    }

    /// Two files: `config.toml` and one account credential.
    fn seed_tree(dir: &Path) {
        fs::create_dir_all(dir.join("config/accounts/a")).unwrap();
        fs::write(dir.join("config/config.toml"), b"hello").unwrap();
        fs::write(
            dir.join("config/accounts/a/.credentials.json"),
            b"ten-bytes!",
        )
        .unwrap();
    }

    fn index_at(dir: &Path) -> Index {
        Index::at(&dir.join("index.sqlite3")).unwrap()
    }

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_760_000_000, 0).unwrap()
    }

    /// A `FileEntry` for one real file, built through the same stat path the
    /// collectors use.
    fn entry_for(path: &Path) -> FileEntry {
        let mut scan = CategoryScan::empty(SyncCategory::Config);
        scope::push_path(path, &mut scan);
        scan.files.pop().expect("push_path accepted the fixture")
    }

    fn write_fixture(path: &Path, len: usize, seed: u64) {
        fs::write(path, pseudo(len, seed)).unwrap();
    }

    /// Chunk a file with no cached row: the from-scratch answer.
    fn from_scratch(path: &Path) -> (FilePlan, Counters) {
        let mut known = HashSet::new();
        let mut counters = Counters::default();
        let plan = plan_file(
            &entry_for(path),
            None,
            &(toy_id as fn(&[u8]) -> [u8; 32]),
            &mut known,
            &mut counters,
        )
        .unwrap();
        (plan, counters)
    }

    /// Chunk a file against a cached row, i.e. exercise the append check.
    fn against(path: &Path, cached: &FilePlan) -> (FilePlan, Counters) {
        let mut known: HashSet<[u8; 32]> = HashSet::new();
        let mut counters = Counters::default();
        let record = FileRecord {
            sealed_chunks: cached.sealed_chunks,
            chunk_ids: cached.chunk_ids.clone(),
        };
        let plan = plan_file(
            &entry_for(path),
            Some(record),
            &(toy_id as fn(&[u8]) -> [u8; 32]),
            &mut known,
            &mut counters,
        )
        .unwrap();
        (plan, counters)
    }

    // ---- Task 1: change detection --------------------------------------

    #[test]
    fn a_first_plan_opens_every_file_and_lists_every_chunk() {
        let dir = TempDir::new().unwrap();
        seed_tree(dir.path());
        let index = index_at(dir.path());

        let plan = build(&roots_at(dir.path()), &cfg(), &index, now(), toy_id).unwrap();

        assert_eq!(plan.files_opened, 2, "both files are new, so both are read");
        assert_eq!(plan.new_chunk_ids.len(), 2, "one whole-file chunk each");
        assert_eq!(plan.total_new_bytes, 15, "5 + 10 bytes of plaintext");
        assert_eq!(plan.total_raw_bytes, 15);
        assert!(!plan.is_empty());
        assert!(!plan.index_rebuilt);

        let config = plan
            .categories
            .iter()
            .find(|c| c.category == SyncCategory::Config)
            .unwrap();
        assert_eq!(config.files, 2);
        assert_eq!(config.raw_bytes, 15);
        assert_eq!(config.new_bytes, 15);
    }

    #[test]
    fn a_second_plan_over_an_untouched_tree_opens_nothing_and_uploads_nothing() {
        let dir = TempDir::new().unwrap();
        seed_tree(dir.path());
        let index = index_at(dir.path());
        let roots = roots_at(dir.path());

        build(&roots, &cfg(), &index, now(), toy_id).unwrap();
        let second = build(&roots, &cfg(), &index, now(), toy_id).unwrap();

        // The counter, not the clock: SYNC-02 is "no file was opened".
        assert_eq!(second.files_opened, 0);
        assert!(second.is_empty());
        assert!(second.new_chunk_ids.is_empty());
        assert_eq!(second.total_new_bytes, 0);
        assert_eq!(second.total_raw_bytes, 15, "the tree is still scanned");
        assert!(second.file_plans.iter().all(|f| f.reused));
    }

    #[test]
    fn a_rewritten_file_is_rechunked() {
        let dir = TempDir::new().unwrap();
        seed_tree(dir.path());
        let index = index_at(dir.path());
        let roots = roots_at(dir.path());
        build(&roots, &cfg(), &index, now(), toy_id).unwrap();

        fs::write(dir.path().join("config/config.toml"), b"goodbye!").unwrap();
        let second = build(&roots, &cfg(), &index, now(), toy_id).unwrap();

        assert_eq!(second.files_opened, 1, "only the rewritten file");
        assert_eq!(second.new_chunk_ids.len(), 1);
        assert_eq!(second.total_new_bytes, 8);
    }

    #[test]
    fn a_file_rewritten_in_place_at_the_same_size_is_caught_by_mtime() {
        let dir = TempDir::new().unwrap();
        seed_tree(dir.path());
        let index = index_at(dir.path());
        let roots = roots_at(dir.path());
        build(&roots, &cfg(), &index, now(), toy_id).unwrap();

        // Same length, same inode — only mtime_ns separates these two states,
        // and D5 carries it precisely so this is not a missed file.
        let path = dir.path().join("config/config.toml");
        fs::write(&path, b"HELLO").unwrap();
        let bumped = SystemTime::now() + Duration::from_secs(60);
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(bumped)
            .unwrap();

        let second = build(&roots, &cfg(), &index, now(), toy_id).unwrap();
        assert_eq!(second.files_opened, 1);
        assert_eq!(second.new_chunk_ids.len(), 1);
    }

    #[test]
    fn a_file_with_no_index_row_is_rechunked_though_nothing_changed_on_disk() {
        let dir = TempDir::new().unwrap();
        seed_tree(dir.path());
        let index = index_at(dir.path());
        let roots = roots_at(dir.path());
        build(&roots, &cfg(), &index, now(), toy_id).unwrap();

        // Age every row out: the index is a hint, and losing it costs work,
        // never correctness.
        assert!(index.evict_unseen(0).unwrap() >= 2);

        let second = build(&roots, &cfg(), &index, now(), toy_id).unwrap();
        assert_eq!(second.files_opened, 2);
        assert_eq!(second.total_new_bytes, 15);
    }

    #[test]
    fn a_file_under_the_chunk_size_becomes_exactly_one_chunk() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("small.bin");
        write_fixture(&path, 1_000, 7);

        let (plan, counters) = from_scratch(&path);
        assert_eq!(plan.chunk_ids.len(), 1);
        assert_eq!(plan.sealed_chunks, 0, "a lone tail seals nothing");
        assert_eq!(plan.new_bytes, 1_000);
        assert_eq!(counters.bytes_read, 1_000);
    }

    #[test]
    fn a_file_of_exactly_one_chunk_has_one_sealed_chunk_and_no_tail() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("aligned.bin");
        write_fixture(&path, CHUNK_BYTES as usize, 9);

        let (plan, _) = from_scratch(&path);
        assert_eq!(plan.chunk_ids.len(), 1, "not two — the tail is empty");
        assert_eq!(plan.sealed_chunks, 1);
        assert_eq!(plan.new_bytes, CHUNK_BYTES);
    }

    // ---- Task 2: the append fast path ----------------------------------

    fn appended_fixture(dir: &Path) -> (PathBuf, FilePlan) {
        let path = dir.join("big.jsonl");
        write_fixture(&path, FIXTURE_BYTES, 42);
        let (before, _) = from_scratch(&path);
        assert_eq!(before.sealed_chunks, 20, "5 MiB + 100 KiB is 20 chunks");
        assert_eq!(before.chunk_ids.len(), 21, "…plus a 100 KiB tail");

        let mut f = OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(&pseudo(APPEND_BYTES, 4242)).unwrap();
        f.flush().unwrap();
        (path, before)
    }

    #[test]
    fn appending_plans_one_new_sealed_chunk_and_one_new_tail() {
        let dir = TempDir::new().unwrap();
        let (path, before) = appended_fixture(dir.path());

        let (after, _) = against(&path, &before);

        assert_eq!(after.sealed_chunks, 21, "the old tail sealed");
        assert_eq!(after.chunk_ids.len(), 22);
        assert_eq!(
            after.chunk_ids[..20],
            before.chunk_ids[..20],
            "every chunk below the old end-of-file kept its id"
        );
        assert_eq!(after.new_chunk_ids.len(), 2, "one sealed chunk, one tail");
        let expected = CHUNK_BYTES + (FIXTURE_BYTES + APPEND_BYTES) as u64 - 21 * CHUNK_BYTES;
        assert_eq!(after.new_bytes, expected);
        assert!(
            after.new_bytes < 400 * 1024,
            "a 200 KiB append must not plan the whole 5 MiB file"
        );
    }

    #[test]
    fn the_append_reads_one_verification_chunk_plus_the_new_region() {
        let dir = TempDir::new().unwrap();
        let (path, before) = appended_fixture(dir.path());

        let (_, counters) = against(&path, &before);

        let new_region = (FIXTURE_BYTES + APPEND_BYTES) as u64 - 20 * CHUNK_BYTES;
        assert_eq!(counters.files_opened, 1);
        assert_eq!(counters.bytes_read, CHUNK_BYTES + new_region);
        assert_eq!(counters.append_check_miss_bytes, 0, "the check passed");
        assert!(
            counters.bytes_read < (FIXTURE_BYTES / 2) as u64,
            "the point of the fast path is not re-reading the file"
        );
    }

    #[test]
    fn truncating_falls_back_to_a_full_rechunk() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("big.jsonl");
        write_fixture(&path, FIXTURE_BYTES, 42);
        let (before, _) = from_scratch(&path);

        // Shorter than the cached sealed region: there is nothing to verify
        // against, so the whole file is re-read.
        let truncated = 1024 * 1024;
        write_fixture(&path, truncated, 42);

        let (after, counters) = against(&path, &before);
        let (scratch, _) = from_scratch(&path);

        assert_eq!(
            after.chunk_ids, scratch.chunk_ids,
            "a truncation must plan the same ids a fresh run would, not a short list"
        );
        assert_eq!(after.sealed_chunks, scratch.sealed_chunks);
        assert_eq!(counters.bytes_read, truncated as u64, "no probe was read");
        assert_eq!(
            counters.append_check_miss_bytes, 0,
            "a shrink is not a miss"
        );
    }

    #[test]
    fn an_in_place_overwrite_fails_the_append_check_and_is_rechunked() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("big.jsonl");
        write_fixture(&path, FIXTURE_BYTES, 42);
        let (before, _) = from_scratch(&path);

        // Same length, different content: (size, mtime, inode) cannot tell this
        // from an append, so only the re-hash catches it.
        let mut f = OpenOptions::new().write(true).open(&path).unwrap();
        f.seek(SeekFrom::Start(19 * CHUNK_BYTES)).unwrap();
        f.write_all(&pseudo(4096, 777)).unwrap();
        f.flush().unwrap();

        let (after, counters) = against(&path, &before);
        let (scratch, _) = from_scratch(&path);

        assert_eq!(after.chunk_ids, scratch.chunk_ids);
        assert_ne!(
            after.chunk_ids, before.chunk_ids,
            "the rewritten chunk must not keep its old id"
        );
        assert_eq!(
            counters.append_check_miss_bytes, CHUNK_BYTES,
            "the failed check bills exactly its one probe"
        );
        assert_eq!(counters.bytes_read, CHUNK_BYTES + FIXTURE_BYTES as u64);
    }

    #[test]
    fn a_file_that_grew_but_whose_last_sealed_chunk_changed_is_fully_rechunked() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("big.jsonl");
        write_fixture(&path, FIXTURE_BYTES, 42);
        let (before, _) = from_scratch(&path);

        // Grows *and* rewrites: indistinguishable from an append by stat alone.
        write_fixture(&path, FIXTURE_BYTES + APPEND_BYTES, 1234);

        let (after, counters) = against(&path, &before);
        let (scratch, _) = from_scratch(&path);

        assert_eq!(after.chunk_ids, scratch.chunk_ids);
        assert_eq!(counters.append_check_miss_bytes, CHUNK_BYTES);
        assert_eq!(
            counters.bytes_read,
            CHUNK_BYTES + (FIXTURE_BYTES + APPEND_BYTES) as u64
        );
    }

    #[test]
    fn a_cached_sealed_count_of_zero_skips_the_verification_read() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("small.bin");
        write_fixture(&path, 1_000, 3);

        let cached = FilePlan {
            path: path.clone(),
            chunk_ids: vec![[0u8; 32]],
            sealed_chunks: 0,
            new_chunk_ids: Vec::new(),
            new_bytes: 0,
            reused: false,
        };
        let (after, counters) = against(&path, &cached);

        assert_eq!(counters.bytes_read, 1_000, "nothing to verify, so no probe");
        assert_eq!(counters.append_check_miss_bytes, 0);
        assert_eq!(after.chunk_ids.len(), 1);
    }

    #[test]
    fn a_corrupt_row_claiming_more_sealed_chunks_than_it_stores_is_rechunked() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("big.jsonl");
        write_fixture(&path, FIXTURE_BYTES, 42);
        let (before, _) = from_scratch(&path);

        let lying = FilePlan {
            sealed_chunks: 40,
            ..before.clone()
        };
        let (after, counters) = against(&path, &lying);

        assert_eq!(after.chunk_ids, before.chunk_ids);
        assert_eq!(counters.bytes_read, FIXTURE_BYTES as u64, "no probe");
    }

    #[test]
    fn an_identical_chunk_in_two_files_uploads_once() {
        let dir = TempDir::new().unwrap();
        seed_tree(dir.path());
        // Same bytes in both files.
        fs::write(dir.path().join("config/config.toml"), b"same").unwrap();
        fs::write(
            dir.path().join("config/accounts/a/.credentials.json"),
            b"same",
        )
        .unwrap();
        let index = index_at(dir.path());

        let plan = build(&roots_at(dir.path()), &cfg(), &index, now(), toy_id).unwrap();

        assert_eq!(plan.files_opened, 2);
        assert_eq!(plan.new_chunk_ids.len(), 1, "deduplicated across files");
        assert_eq!(plan.total_new_bytes, 4);
        assert_eq!(plan.total_raw_bytes, 8);
    }
}
