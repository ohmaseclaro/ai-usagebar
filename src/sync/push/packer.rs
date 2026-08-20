//! `SyncPlan` to `PushBundle`: sealed chunks packed into remote-sized objects,
//! plus the manifest, the index object and the snapshot root.
//!
//! This is the object that makes GitHub's 80-per-minute content-creation limit
//! irrelevant: 5,000 chunks become a handful of 48 MiB assets rather than 5,000
//! requests (REPO-06).
//!
//! # Manifest paths are root-relative, never absolute
//!
//! [`FilePlan::path`](crate::sync::plan::FilePlan) is an absolute local path
//! carrying the pushing user's home directory and username. Storing it would
//! make the bundle unresolvable on a second machine, leak the username to
//! anyone who obtains the repository, and — worst — be rejected by Phase 5's
//! traversal defence, so the bundle would only be restorable by disabling the
//! very check protecting the machine restoring it.
//!
//! [`manifest_path`] renders the root-prefixed relative form instead: the name of
//! the [`SyncRoots`] root the file was collected under, then the path beneath it.
//! A file under none of the roots is an error rather than a fallback to the
//! absolute path — there is no correct absolute path to fall back to.
//!
//! # The two size constants, and which one governs
//!
//! [`should_seal`] compares against [`PACK_MAX`] (48 MiB) and never reads
//! `PACK_TARGET` (32 MiB), so packs fill to 48 MiB and the target is advisory.
//! Every size expectation in this file's tests is therefore derived from
//! `PACK_MAX`, including the worst-case pack-header guard — a guard built at the
//! advisory target would understate the real case by half again, and a guard
//! that understates its case is a guard that passes on the day it should fail.
//!
//! **Neither constant may be raised here.** `docs/sync-format.md` §7 records
//! that CAL-1 was never run, so the recorded fallback stands; and the pack
//! header is still a *single* sealed chunk, which gap-closure 1-09 deliberately
//! did not reach when it made manifests and index objects multi-chunk. Its
//! entry-count ceiling is a function of `PACK_MAX`.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::Path;

use base64::Engine;
use zeroize::Zeroizing;

use crate::error::{AppError, Result};
use crate::sync::chunk::{Blob, seal_chunk};
use crate::sync::crypto::{ChunkId, Keys};
use crate::sync::index::ChunkLocation;
use crate::sync::model::{FileEntry, IndexEntry, IndexObject, Manifest, Root};
use crate::sync::pack::{PackWriter, should_seal};
use crate::sync::plan::SyncPlan;
use crate::sync::{CHUNK_SIZE, SyncRoots};

use super::{B64, BuiltPack, PushBundle, PushCtx, RemoteIndexEntry};

/// Turn a plan into the bytes a push puts on the wire.
///
/// # The order the three objects are built in
///
/// One sentence in prose and three passes in code, and getting it wrong
/// produces a bundle Phase 5 cannot read while every test here still passes:
///
/// 1. the manifest is built, sealed and **packed**, and the writer is flushed,
///    because
/// 2. the index object describes every pack entry this snapshot references —
///    reused chunks, this run's data chunks, *and* the manifest's chunks. Build
///    it before the manifest is packed and the root's `manifest_chunks` name ids
///    the index object does not describe, so a restore cannot find the manifest
///    at all.
/// 3. the index object's own chunks are necessarily not described by the index
///    object — nothing describes itself — which is exactly why
///    [`PushBundle::index_chunks`] exists as a plaintext bootstrap in the
///    pointer.
///
/// The snapshot root comes last, naming the manifest's chunk ids in order.
///
/// Nothing here seals a new *kind* of object: packs, manifests, index objects
/// and roots are the four the format already defines, so Phase 1's deferred AAD
/// object-type separator stays untriggered.
pub fn build(ctx: &PushCtx<'_>, plan: &SyncPlan) -> Result<PushBundle> {
    // Every chunk the snapshot names, deduplicated, in first-seen order. Note
    // this is the union over `file_plans`, **not** `plan.new_chunk_ids`: a
    // snapshot must name a pack for every chunk it references, and taking only
    // the plan's new ids is precisely how `referenced_packs` ends up missing the
    // packs that hold all the unchanged data.
    let mut named: Vec<ChunkId> = Vec::new();
    let mut seen: HashSet<ChunkId> = HashSet::new();
    for file in &plan.file_plans {
        for raw in &file.chunk_ids {
            let id = ChunkId::from_bytes(*raw);
            if seen.insert(id) {
                named.push(id);
            }
        }
    }
    let reusable = reusable(ctx, &named);

    let mut packs = Packing::default();
    let mut files: Vec<FileEntry> = Vec::new();

    for file in &plan.file_plans {
        let meta = std::fs::metadata(&file.path).map_err(|e| AppError::io_at(&file.path, e))?;
        let wanted: HashSet<ChunkId> = file
            .chunk_ids
            .iter()
            .map(|raw| ChunkId::from_bytes(*raw))
            .filter(|id| !reusable.contains_key(id) && !packs.holds(id))
            .collect();
        // SYNC-02, honoured rather than merely claimed: a file whose every chunk
        // the snapshot can already locate is not opened at all.
        if !wanted.is_empty() {
            pack_file(ctx, &mut packs, &file.path, &wanted)?;
        }
        files.push(FileEntry {
            path: manifest_path(ctx.roots, &file.path)?,
            mode: mode_of(&meta),
            true_len: meta.len(),
            chunks: file
                .chunk_ids
                .iter()
                .map(|raw| ChunkId::from_bytes(*raw))
                .collect(),
        });
    }

    // 1. The manifest, packed like any other chunk, and then sealed into a pack
    //    so every entry above names a pack that exists.
    let manifest = Manifest::new(files);
    let mut manifest_chunks: Vec<ChunkId> = Vec::new();
    for blob in manifest.seal(ctx.keys)? {
        manifest_chunks.push(blob.id);
        packs.push(blob, ctx.keys)?;
    }
    packs.flush(ctx.keys)?;

    // 2. The index object, over every entry this snapshot references.
    let mut entries: Vec<IndexEntry> = named
        .iter()
        .filter_map(|id| reusable.get(id).map(|at| entry_at(*id, at)))
        .collect();
    entries.extend(packs.entries.iter().copied());
    let index_object = IndexObject::new(entries, supersedes(ctx));

    let first_index_entry = packs.entries.len();
    for blob in index_object.seal(ctx.keys)? {
        packs.push(blob, ctx.keys)?;
    }
    packs.flush(ctx.keys)?;

    // 3. The index object's own chunks, as the plaintext bootstrap.
    let index_chunks: Vec<RemoteIndexEntry> = packs.entries[first_index_entry..]
        .iter()
        .map(|e| RemoteIndexEntry {
            id: e.id,
            pack: e.pack,
            offset: e.offset,
            clen: e.clen,
            true_len: e.true_len,
        })
        .collect();

    // 4. The root, naming the manifest's chunks in order.
    let counter = next_counter(ctx)?;
    let root = Root::new(
        counter,
        ctx.now,
        ctx.repo_id.clone(),
        manifest_chunks,
        ctx.kdf,
    )
    .seal(ctx.keys)?;

    // 5. The local chunk table learns where everything this run packed landed —
    //    after `finish`, when the packs' content addresses exist.
    ctx.index.record_chunks(&packs.rows())?;

    // **Every** pack the snapshot needs, reused ones included. A snapshot that
    // named only its new packs would let prune delete the packs holding all of
    // its unchanged data, which is the unrestorable backup D2 exists to prevent.
    let mut referenced_packs: Vec<ChunkId> = Vec::new();
    let mut counted: HashSet<ChunkId> = HashSet::new();
    for pack in index_object
        .entries
        .iter()
        .map(|e| e.pack)
        .chain(index_chunks.iter().map(|e| e.pack))
    {
        if counted.insert(pack) {
            referenced_packs.push(pack);
        }
    }

    Ok(PushBundle {
        packs: packs.done,
        root,
        index_chunks,
        referenced_packs,
        counter,
    })
}

/// The chunks this snapshot may name without sealing them again.
///
/// Two conditions, and the second is the load-bearing one.
///
/// The local `chunk` table records what this machine **packed**, which is not
/// the same as what landed: a push that packs and then fails at upload leaves
/// rows pointing at packs no remote ever saw. Reusing one would publish a
/// snapshot referencing a pack that does not exist — an unrestorable backup,
/// D2's worst outcome, reached with nobody doing anything wrong. So a chunk is
/// reusable only when its pack is named by a snapshot the **pointer** already
/// carries, which is the remote's own evidence that the pack landed *and* has
/// not since been pruned.
///
/// Both halves fail towards not-reusable, and the cost of that is re-uploading
/// bytes that were already there.
fn reusable(ctx: &PushCtx<'_>, ids: &[ChunkId]) -> HashMap<ChunkId, ChunkLocation> {
    let published: HashSet<ChunkId> = ctx
        .previous
        .iter()
        .flat_map(|pointer| pointer.snapshots.iter())
        .flat_map(|snapshot| snapshot.packs.iter().copied())
        .collect();
    let mut known = ctx.index.chunk_locations(ids);
    known.retain(|_, at| published.contains(&at.pack));
    known
}

fn entry_at(id: ChunkId, at: &ChunkLocation) -> IndexEntry {
    IndexEntry {
        id,
        pack: at.pack,
        offset: at.offset,
        clen: at.clen,
        true_len: at.plen,
    }
}

/// Seal and pack every chunk of `path` the snapshot cannot already locate.
///
/// Streamed in [`CHUNK_SIZE`] blocks through **one** reused buffer (T-4-16): a
/// 115 MB transcript never exists as a 115 MB plaintext allocation, each block's
/// plaintext is overwritten by the next, and [`Zeroizing`] wipes the last one.
/// Reading the whole file into a `Vec` would hold every credential in the bundle
/// in memory at once for no gain.
///
/// The skip decision is made on the id of the block that was actually read,
/// not on the plan's list by position: if the file changed under us, the id we
/// would skip on is the id the blob would have had.
fn pack_file(
    ctx: &PushCtx<'_>,
    packs: &mut Packing,
    path: &Path,
    wanted: &HashSet<ChunkId>,
) -> Result<()> {
    let mut file = std::fs::File::open(path).map_err(|e| AppError::io_at(path, e))?;
    let mut buf = Zeroizing::new(vec![0u8; CHUNK_SIZE]);
    loop {
        let read = fill(&mut file, &mut buf).map_err(|e| AppError::io_at(path, e))?;
        if read == 0 {
            return Ok(());
        }
        let block = &buf[..read];
        let id = ctx.keys.chunk_id(block);
        if wanted.contains(&id) && !packs.holds(&id) {
            packs.push(seal_chunk(ctx.keys, block)?, ctx.keys)?;
        }
    }
}

/// Read until `buf` is full or the file ends. `Read::read` is allowed to return
/// short, and a short read misaligned by one byte would re-chunk the whole file
/// into ids nothing recognises.
fn fill(source: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match source.read(&mut buf[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(filled)
}

/// The fill loop's state: one writer at a time, sealed when [`should_seal`] says
/// so, plus every entry's location once the pack holding it has an address.
#[derive(Default)]
struct Packing {
    writer: PackWriter,
    done: Vec<BuiltPack>,
    /// Filled in as each pack is finished — a pack's id does not exist until its
    /// header is sealed, so an entry recorded before that names nothing.
    entries: Vec<IndexEntry>,
    pending: Vec<IndexEntry>,
    held: HashSet<ChunkId>,
}

impl Packing {
    /// Has this run already packed `id`? The de-duplication decision lives at
    /// the call site rather than in [`Packing::push`], so a caller may
    /// deliberately pack the same bytes twice.
    fn holds(&self, id: &ChunkId) -> bool {
        self.held.contains(id)
    }

    fn push(&mut self, blob: Blob, keys: &Keys) -> Result<()> {
        // `should_seal` as it is, and no second size rule: a second size literal
        // in this file is how the header ceiling gets quietly re-broken.
        if !self.writer.is_empty() && should_seal(self.writer.len_bytes(), blob.ciphertext.len()) {
            self.flush(keys)?;
        }
        self.held.insert(blob.id);
        self.pending.push(IndexEntry {
            id: blob.id,
            pack: ChunkId::from_bytes([0; 32]), // filled in at `flush`
            offset: self.writer.len_bytes() as u64,
            clen: blob.ciphertext.len() as u32,
            true_len: blob.true_len,
        });
        self.writer.push(blob);
        Ok(())
    }

    /// Seal the open writer, if there is one, and stamp its address on to every
    /// entry it holds.
    fn flush(&mut self, keys: &Keys) -> Result<()> {
        if self.writer.is_empty() {
            return Ok(());
        }
        let (id, bytes) = std::mem::take(&mut self.writer).finish(keys)?;
        for mut entry in std::mem::take(&mut self.pending) {
            entry.pack = id;
            self.entries.push(entry);
        }
        self.done.push(BuiltPack { id, bytes });
        Ok(())
    }

    /// What [`crate::sync::index::Index::record_chunks`] takes.
    fn rows(&self) -> Vec<(ChunkId, ChunkId, u64, u32, u32)> {
        self.entries
            .iter()
            .map(|e| (e.id, e.pack, e.offset, e.clen, e.true_len))
            .collect()
    }
}

/// The index-object chunk ids the previous snapshot used, which the new one
/// supersedes.
fn supersedes(ctx: &PushCtx<'_>) -> Vec<ChunkId> {
    ctx.previous
        .as_ref()
        .and_then(|p| p.snapshots.last())
        .map(|s| s.index_chunks.iter().map(|e| e.id).collect())
        .unwrap_or_default()
}

/// One above the highest counter the pointer's snapshot roots carry, or 1 on a
/// first push.
///
/// Read out of the sealed roots rather than off the pointer's shape, because
/// position in `snapshots` is remote-controlled and the counter inside a root is
/// not: a reader selects by counter, so the writer must too.
///
/// Do **not** advance the local anchor from here. Phase 1's rule is that the
/// anchor advances only after a snapshot verifies, and this code is producing
/// one, not verifying it.
fn next_counter(ctx: &PushCtx<'_>) -> Result<u64> {
    let Some(previous) = ctx.previous.as_ref() else {
        return Ok(1);
    };
    let highest = previous
        .snapshots
        .iter()
        .filter_map(|s| B64.decode(&s.root).ok())
        .filter_map(|framed| Root::open(ctx.keys, &framed, &ctx.repo_id).ok())
        .map(|root| root.counter)
        // A pointer whose roots this build cannot open — a damaged entry, or one
        // written by a format this build predates. The snapshot count is the
        // fallback the tracer used: monotone for this machine, and never a value
        // this bundle has already published.
        .max()
        .unwrap_or(previous.snapshots.len() as u64);
    Ok(highest + 1)
}

/// The root-prefixed relative encoding — see the module docs.
///
/// The prefix is the *name of the root*, not its value, so nothing about this
/// machine's layout survives into the bundle. Phase 5 resolves it back against
/// that machine's own [`SyncRoots`].
pub fn manifest_path(roots: &SyncRoots, path: &Path) -> Result<String> {
    // Longest root first, so a nested root wins over its parent.
    let mut candidates = [
        ("config", roots.config_dir.as_path()),
        ("desktop-data", roots.desktop_data_dir.as_path()),
        ("desktop-profiles", roots.desktop_profiles_dir.as_path()),
        ("claude-home", roots.claude_home.as_path()),
    ];
    candidates.sort_by_key(|(_, root)| std::cmp::Reverse(root.as_os_str().len()));

    for (name, root) in candidates {
        if let Ok(rest) = path.strip_prefix(root) {
            let rest = rest
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            return Ok(format!("{name}/{rest}"));
        }
    }
    Err(AppError::Other(format!(
        "refusing to record {} in the manifest: it lies under none of the sync roots, and an \
         absolute path in a bundle is unresolvable on another machine",
        path.display()
    )))
}

/// Unix permission bits, or 0o600 where the platform has none.
fn mode_of(meta: &std::fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o7777
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        0o600
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SyncConfig;
    use crate::sync::crypto::{KdfParams, Keyfile, content_address};
    use crate::sync::github::token::TokenSource;
    use crate::sync::github::{Client, Endpoints, RepoRef};
    use crate::sync::index::Index;
    use crate::sync::pack::{PACK_MAX, PackEntry, PackHeader, blob_bytes, read_header};
    use crate::sync::plan::FilePlan;
    use crate::sync::{PACK_HEADER_VERSION, chunk};
    use chrono::{DateTime, Utc};
    use tempfile::TempDir;
    use zeroize::Zeroizing as Z;

    use super::super::{Pointer, SnapshotRecord};

    /// Microseconds instead of ~1.5 s and a gibibyte. Never use production
    /// parameters in a unit test: the AUR `check()` runs these on an
    /// installer's machine.
    const CHEAP: KdfParams = KdfParams {
        m_kib: 8,
        t: 1,
        p: 1,
    };

    const NOW: DateTime<Utc> = match DateTime::from_timestamp(1_700_000_000, 0) {
        Some(t) => t,
        None => panic!("a fixed timestamp"),
    };

    const REPO_ID: &str = "github:1";

    /// Deterministic xorshift: incompressible bytes with no random source and no
    /// clock anywhere near the test.
    fn incompressible(len: usize) -> Vec<u8> {
        let mut state = 0x2545_f491_4f6c_dd1du64;
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 24) as u8
            })
            .collect()
    }

    /// Everything a [`PushCtx`] borrows, owned in one place so a test can hand
    /// out contexts differing only in the pointer that arrived.
    struct Fixture {
        dir: TempDir,
        roots: SyncRoots,
        keys: Keys,
        index: Index,
        cfg: SyncConfig,
        client: Client,
        repo: RepoRef,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = TempDir::new().unwrap();
            let roots = SyncRoots::at(
                dir.path().join("config.toml"),
                dir.path().to_path_buf(),
                dir.path().join("desktop"),
                dir.path().join("profiles"),
                dir.path().join("claude-home"),
            );
            let keys =
                Keyfile::create_with_floor(b"correct horse battery staple", CHEAP, CHEAP.m_kib)
                    .expect("keyfile creation")
                    .1;
            let index = Index::at(&roots.index_file).unwrap();
            Self {
                dir,
                roots,
                keys,
                index,
                cfg: SyncConfig::default(),
                // Nothing under this module makes a request; the base is a
                // parked address so a regression would fail rather than dial
                // anything real.
                client: Client::new(
                    &Endpoints {
                        api_base: "http://127.0.0.1:1".into(),
                        uploads_base: "http://127.0.0.1:1".into(),
                    },
                    Z::new("github_pat_fixture_not_a_real_token".into()),
                    TokenSource::Env,
                )
                .unwrap(),
                repo: RepoRef::parse("o/n").unwrap(),
            }
        }

        fn ctx(&self, previous: Option<Pointer>) -> PushCtx<'_> {
            PushCtx {
                client: &self.client,
                repo: &self.repo,
                cfg: &self.cfg,
                roots: &self.roots,
                keys: &self.keys,
                kdf: CHEAP,
                index: &self.index,
                repo_id: REPO_ID.into(),
                keyfile_asset: "keyfile-x.json".into(),
                previous,
                now: NOW,
            }
        }

        /// Write `bytes` under the config root and return the [`FilePlan`] the
        /// planner would have produced for it.
        fn seed(&self, name: &str, bytes: &[u8]) -> FilePlan {
            let path = self.roots.config_dir.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, bytes).unwrap();
            let chunk_ids: Vec<[u8; 32]> = chunk::split(bytes)
                .map(|block| *self.keys.chunk_id(block).as_bytes())
                .collect();
            FilePlan {
                path,
                sealed_chunks: chunk::sealed_chunk_count(bytes.len() as u64),
                new_chunk_ids: chunk_ids.clone(),
                chunk_ids,
                new_bytes: bytes.len() as u64,
                new_stored_bytes: bytes.len() as u64,
                reused: false,
            }
        }
    }

    fn plan_of(files: Vec<FilePlan>) -> SyncPlan {
        let mut new_chunk_ids: Vec<[u8; 32]> = Vec::new();
        for file in &files {
            for id in &file.new_chunk_ids {
                if !new_chunk_ids.contains(id) {
                    new_chunk_ids.push(*id);
                }
            }
        }
        SyncPlan {
            categories: Vec::new(),
            new_chunk_ids,
            total_raw_bytes: 0,
            total_new_bytes: 0,
            total_new_stored_bytes: 0,
            files_opened: files.len(),
            append_check_miss_bytes: 0,
            index_rebuilt: false,
            file_plans: files,
        }
    }

    /// The pointer the flip would have published for `bundle` — which is what
    /// makes its packs reusable on the next push.
    fn published(bundle: &PushBundle) -> Pointer {
        Pointer {
            format: super::super::POINTER_VERSION,
            repo_id: REPO_ID.into(),
            keyfile: "keyfile-x.json".into(),
            snapshots: vec![SnapshotRecord {
                root: B64.encode(&bundle.root),
                index_chunks: bundle.index_chunks.clone(),
                packs: bundle.referenced_packs.clone(),
            }],
        }
    }

    /// Every chunk id the bundle's own packs really carry, read back through the
    /// format's own reader rather than from the builder's bookkeeping.
    fn packed_ids(bundle: &PushBundle, keys: &Keys) -> HashSet<ChunkId> {
        bundle
            .packs
            .iter()
            .flat_map(|p| read_header(keys, &p.bytes).unwrap().entries)
            .map(|e| e.id)
            .collect()
    }

    /// Which of the bundle's packs really holds `id`, read back through the
    /// format's own reader.
    fn pack_holding(bundle: &PushBundle, keys: &Keys, id: &ChunkId) -> ChunkId {
        bundle
            .packs
            .iter()
            .find(|p| {
                read_header(keys, &p.bytes)
                    .unwrap()
                    .entries
                    .iter()
                    .any(|e| e.id == *id)
            })
            .expect("no pack in this bundle holds that chunk")
            .id
    }

    /// Slice one chunk out of whichever of this bundle's packs holds it.
    fn fetch(bundle: &PushBundle, at: &IndexEntry) -> Vec<u8> {
        let pack = bundle
            .packs
            .iter()
            .find(|p| p.id == at.pack)
            .expect("the entry names a pack this bundle built");
        let entry = PackEntry {
            id: at.id,
            offset: at.offset,
            clen: at.clen,
            true_len: at.true_len,
        };
        blob_bytes(&pack.bytes, &entry).unwrap().to_vec()
    }

    // ---- manifest paths -----------------------------------------------

    /// The defect Phase 5's planning found: an absolute path carries the
    /// username, is unresolvable elsewhere, and is exactly what the traversal
    /// defence is written to reject.
    #[test]
    fn a_manifest_path_is_root_relative_and_never_absolute() {
        let fx = Fixture::new();
        let roots = &fx.roots;

        let rendered = manifest_path(
            roots,
            &fx.dir.path().join("accounts/work/.credentials.json"),
        )
        .unwrap();
        assert_eq!(rendered, "config/accounts/work/.credentials.json");

        let home = manifest_path(roots, &roots.claude_home.join("projects/a.jsonl")).unwrap();
        assert_eq!(home, "claude-home/projects/a.jsonl");

        for path in [rendered, home] {
            assert!(!path.starts_with('/'), "{path}");
            assert!(!path.contains(".."), "{path}");
            assert!(
                !path.contains(fx.dir.path().to_str().unwrap()),
                "no local prefix survives: {path}"
            );
        }
    }

    #[test]
    fn a_file_under_no_root_is_an_error_rather_than_an_absolute_path() {
        let fx = Fixture::new();
        let err = manifest_path(&fx.roots, Path::new("/etc/passwd"))
            .expect_err("that is under no sync root");
        assert!(err.to_string().contains("unresolvable"), "{err}");
    }

    // ---- REPO-06: the fill loop ---------------------------------------

    /// **REPO-06, asserted as a pack count.** Thousands of chunks become a
    /// handful of objects, and the expected count is computed from [`PACK_MAX`]
    /// — the constant [`should_seal`] actually compares against — rather than
    /// written down as a literal that a change to the constant would not move.
    ///
    /// The blobs are sealed **once and cloned**, exactly as `pack.rs`'s own
    /// `PACK_MAX` test does: what is under test here is the boundary arithmetic,
    /// and three hundred real zstd-and-ChaCha passes over 256 KiB are not.
    #[test]
    fn the_fill_loop_turns_many_chunks_into_a_handful_of_self_addressing_packs() {
        let keys = Keyfile::create_with_floor(b"pw", CHEAP, CHEAP.m_kib)
            .unwrap()
            .1;
        let sealed = seal_chunk(&keys, &incompressible(CHUNK_SIZE)).unwrap();
        let (id, ciphertext, true_len) = (sealed.id, sealed.ciphertext, sealed.true_len);

        let per_pack = PACK_MAX / ciphertext.len();
        let blobs = 2 * per_pack + 3;

        let mut packs = Packing::default();
        for _ in 0..blobs {
            packs
                .push(
                    Blob {
                        id,
                        ciphertext: ciphertext.clone(),
                        true_len,
                    },
                    &keys,
                )
                .unwrap();
        }
        packs.flush(&keys).unwrap();

        assert_eq!(
            packs.done.len(),
            blobs.div_ceil(per_pack),
            "{blobs} chunks must become ceil({blobs}/{per_pack}) packs, not {blobs} objects"
        );
        assert!(
            packs.done.len() * 40 < blobs,
            "a handful of objects, not one per chunk"
        );

        for pack in &packs.done {
            assert!(
                pack.bytes.len() <= PACK_MAX,
                "a pack grew to {} past PACK_MAX",
                pack.bytes.len()
            );
            assert_eq!(
                content_address(&pack.bytes),
                pack.id,
                "a pack is named by its own bytes"
            );
            assert!(read_header(&keys, &pack.bytes).is_ok());
        }
        // Every entry names a pack that exists, and none carries the placeholder
        // written before the header was sealed.
        let built: HashSet<ChunkId> = packs.done.iter().map(|p| p.id).collect();
        assert_eq!(packs.entries.len(), blobs);
        assert!(packs.entries.iter().all(|e| built.contains(&e.pack)));
    }

    /// **T-4-14, built at [`PACK_MAX`] rather than at the advisory
    /// `PACK_TARGET`.**
    ///
    /// The pack header is still a *single* sealed chunk: `pack.rs` seals it
    /// through `chunk::seal_chunk`, which gap-closure 1-09 deliberately did not
    /// reach when it made manifests and index objects multi-chunk. This pins the
    /// slack that makes that sound.
    ///
    /// The worst case is built from the smallest blob the format admits **in
    /// practice** — a sealed full `CHUNK_SIZE` chunk, so ~192 entries at
    /// `PACK_MAX`. Building it at `PACK_TARGET` would understate the real case
    /// by half again, and a guard that understates its case is one that passes
    /// on the day it should fail.
    ///
    /// If this ever fails, the upgrade path is a format-2 **multi-chunk** header
    /// through the same `chunk::seal_all` / `reassemble` pair the manifest now
    /// uses. Raising `PACK_MAX` is what would break it.
    #[test]
    fn a_worst_case_pack_header_built_at_pack_max_still_seals_as_one_chunk() {
        let keys = Keyfile::create_with_floor(b"pw", CHEAP, CHEAP.m_kib)
            .unwrap()
            .1;
        let clen = seal_chunk(&keys, &incompressible(CHUNK_SIZE))
            .unwrap()
            .ciphertext
            .len();
        let entries = PACK_MAX / clen;

        let header = PackHeader {
            format: PACK_HEADER_VERSION,
            // Every field at its widest: a full-length id, and an offset near
            // the end of a 48 MiB pack, so no entry serializes shorter than a
            // real one would.
            entries: (0..entries)
                .map(|i| PackEntry {
                    id: ChunkId::from_bytes([0xff; 32]),
                    offset: (PACK_MAX - i * clen) as u64,
                    clen: clen as u32,
                    true_len: CHUNK_SIZE as u32,
                })
                .collect(),
        };

        let json = serde_json::to_vec(&header).unwrap();
        assert!(
            json.len() < CHUNK_SIZE,
            "a worst-case header of {entries} entries is {} bytes, past the {CHUNK_SIZE}-byte \
             single-chunk ceiling — see this test's doc comment for the upgrade path",
            json.len()
        );
        // Comfortably, not marginally: it must still seal after zstd and framing.
        assert!(seal_chunk(&keys, &json).is_ok());
        assert!(
            json.len() * 4 < CHUNK_SIZE,
            "only {} bytes of slack",
            CHUNK_SIZE - json.len()
        );
    }

    // ---- the three objects, and the order they are built in ------------

    /// A first push, walked back through Phase 1's own readers: the index object
    /// locates the manifest, the manifest opens, and the root opens under the
    /// caller's own `repo_id`.
    ///
    /// The ordering assertion is the one no other plan would catch: build the
    /// index object before the manifest is packed and every id in
    /// `manifest_chunks` names a location the index object does not describe, so
    /// a restore cannot find the manifest at all.
    #[test]
    fn a_first_push_produces_a_bundle_that_walks_back_through_phase_ones_readers() {
        let fx = Fixture::new();
        let body = incompressible(3 * CHUNK_SIZE + 17);
        let plan = plan_of(vec![fx.seed("accounts/work/.credentials.json", &body)]);

        let bundle = build(&fx.ctx(None), &plan).unwrap();
        assert_eq!(bundle.counter, 1, "a first push starts at one");

        // The bootstrap: the index object's own chunks, named in the clear.
        assert!(!bundle.index_chunks.is_empty());
        let covered: HashSet<ChunkId> = bundle.packs.iter().map(|p| p.id).collect();
        for at in &bundle.index_chunks {
            assert!(
                covered.contains(&at.pack),
                "index_chunks must name a pack this bundle produces"
            );
        }
        let index_object = IndexObject::open(
            &fx.keys,
            &bundle
                .index_chunks
                .iter()
                .map(|at| {
                    (
                        at.id,
                        fetch(
                            &bundle,
                            &IndexEntry {
                                id: at.id,
                                pack: at.pack,
                                offset: at.offset,
                                clen: at.clen,
                                true_len: at.true_len,
                            },
                        ),
                    )
                })
                .collect::<Vec<_>>(),
        )
        .unwrap();

        let root = Root::open(&fx.keys, &bundle.root, REPO_ID).unwrap();
        assert_eq!(root.counter, 1);
        assert_eq!(root.kdf, CHEAP);

        // **The ordering assertion.**
        let manifest_chunks: Vec<(ChunkId, Vec<u8>)> = root
            .manifest_chunks
            .iter()
            .map(|id| {
                let at = index_object
                    .resolve(id)
                    .expect("every manifest chunk resolves through the index object");
                (*id, fetch(&bundle, at))
            })
            .collect();
        let manifest = Manifest::open(&fx.keys, &manifest_chunks).unwrap();

        assert_eq!(manifest.files.len(), 1);
        let file = &manifest.files[0];
        assert_eq!(file.path, "config/accounts/work/.credentials.json");
        assert_eq!(file.true_len, body.len() as u64);
        assert_eq!(file.chunks.len(), 4);
        // …and every data chunk the manifest names resolves too.
        assert!(
            file.chunks
                .iter()
                .all(|id| index_object.resolve(id).is_some())
        );

        // Nothing about this machine survives into the bytes.
        let needle = fx.dir.path().to_str().unwrap().as_bytes();
        for pack in &bundle.packs {
            assert!(!pack.bytes.windows(needle.len()).any(|w| w == needle));
        }
        assert!(!bundle.root.windows(needle.len()).any(|w| w == needle));
    }

    /// **T-4-13.** A second push over an unchanged tree re-seals no data chunk,
    /// and still names the pack holding every one of them. Omitting a reused
    /// pack is the exact input that makes prune delete live data.
    #[test]
    fn a_second_push_reseals_no_data_chunk_and_still_names_the_pack_holding_it() {
        let fx = Fixture::new();
        let body = incompressible(2 * CHUNK_SIZE);
        let plan = plan_of(vec![fx.seed("accounts/work/.credentials.json", &body)]);

        let first = build(&fx.ctx(None), &plan).unwrap();
        let second = build(&fx.ctx(Some(published(&first))), &plan).unwrap();

        let data: HashSet<ChunkId> = plan.file_plans[0]
            .chunk_ids
            .iter()
            .map(|raw| ChunkId::from_bytes(*raw))
            .collect();
        assert!(
            packed_ids(&second, &fx.keys).is_disjoint(&data),
            "a chunk the chunk table already locates is neither re-sealed nor re-packed"
        );
        for id in &data {
            assert!(
                second
                    .referenced_packs
                    .contains(&pack_holding(&first, &fx.keys, id)),
                "the pack holding the unchanged data must still be named"
            );
        }
        // Only the snapshot's own new objects were packed at all: the manifest,
        // and the index object in its own pack.
        assert_eq!(second.packs.len(), 2);
        assert_eq!(second.counter, 2);
    }

    /// The gap `2-05-SUMMARY.md` recorded: a chunk shared with a file that
    /// failed its append check was re-uploaded, because the only evidence of
    /// "already present" lived in the `file` table.
    #[test]
    fn a_chunk_shared_with_a_file_that_failed_its_append_check_is_not_resealed() {
        let fx = Fixture::new();
        let body = incompressible(2 * CHUNK_SIZE);
        let first = build(&fx.ctx(None), &plan_of(vec![fx.seed("a.json", &body)])).unwrap();

        // The second file is byte-identical, and its plan claims every chunk is
        // new — which is exactly what a failed append check produces.
        let shared = fx.seed("b.json", &body);
        let data: HashSet<ChunkId> = shared
            .chunk_ids
            .iter()
            .map(|raw| ChunkId::from_bytes(*raw))
            .collect();
        let second = build(
            &fx.ctx(Some(published(&first))),
            &plan_of(vec![shared.clone()]),
        )
        .unwrap();

        assert!(
            packed_ids(&second, &fx.keys).is_disjoint(&data),
            "the plan called them new; the chunk table knows better"
        );
        assert!(data.iter().all(|id| {
            second
                .referenced_packs
                .contains(&pack_holding(&first, &fx.keys, id))
        }));
    }

    /// **The reason `reusable` asks the pointer and not just the chunk table.**
    ///
    /// A push that packs and then fails at upload leaves chunk rows pointing at
    /// a pack no remote ever saw. Reusing one would publish a snapshot
    /// referencing a pack that does not exist — an unrestorable backup. So a row
    /// whose pack no published snapshot names buys nothing.
    #[test]
    fn a_chunk_whose_pack_no_published_snapshot_names_is_packed_again() {
        let fx = Fixture::new();
        let body = incompressible(2 * CHUNK_SIZE);
        let plan = plan_of(vec![fx.seed("a.json", &body)]);

        let attempted = build(&fx.ctx(None), &plan).unwrap();
        // The flip never happened: the pointer is still absent.
        let retried = build(&fx.ctx(None), &plan).unwrap();

        let data: HashSet<ChunkId> = plan.file_plans[0]
            .chunk_ids
            .iter()
            .map(|raw| ChunkId::from_bytes(*raw))
            .collect();
        assert!(
            data.is_subset(&packed_ids(&retried, &fx.keys)),
            "the chunk table alone is not evidence that a pack landed"
        );
        // …and the same applies to a pointer that names some other pack.
        let stale = Pointer {
            snapshots: vec![SnapshotRecord {
                packs: vec![ChunkId::from_bytes([0xaa; 32])],
                ..published(&attempted).snapshots[0].clone()
            }],
            ..published(&attempted)
        };
        let third = build(&fx.ctx(Some(stale)), &plan).unwrap();
        assert!(data.is_subset(&packed_ids(&third, &fx.keys)));
    }

    /// One above the counter inside the newest *sealed root*, not one above the
    /// pointer's length: position in `snapshots` is remote-controlled and the
    /// counter is not.
    #[test]
    fn the_counter_is_one_above_the_highest_the_pointers_roots_carry() {
        let fx = Fixture::new();
        let plan = plan_of(vec![fx.seed("a.json", b"small")]);

        let sealed = |counter: u64| {
            B64.encode(
                Root::new(counter, NOW, REPO_ID.into(), Vec::new(), CHEAP)
                    .seal(&fx.keys)
                    .unwrap(),
            )
        };
        let record = |root: String| SnapshotRecord {
            root,
            index_chunks: Vec::new(),
            packs: Vec::new(),
        };
        // Newest last by convention — but the highest counter wins regardless of
        // where a hostile remote puts it.
        let pointer = Pointer {
            format: super::super::POINTER_VERSION,
            repo_id: REPO_ID.into(),
            keyfile: "keyfile-x.json".into(),
            snapshots: vec![record(sealed(7)), record(sealed(3))],
        };
        assert_eq!(build(&fx.ctx(Some(pointer)), &plan).unwrap().counter, 8);
    }
}
