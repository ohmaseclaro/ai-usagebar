//! The snapshot object graph: the root (fresh random nonce, monotonic counter),
//! the manifest carried as an ordinary sealed chunk, and the index object with
//! its `supersedes` link.
//!
//! # The chain, and why every hop names the next one
//!
//! ```text
//! root  --manifest_id-->  manifest  --chunk ids-->  chunks
//! ```
//!
//! Each hop's identifier is bound as associated data into the object it names:
//! [`crate::sync::crypto::Keys::seal`] takes the [`ChunkId`] as AAD *and*
//! derives the nonce from it, and [`crate::sync::chunk::open_chunk`] rechecks
//! that the plaintext really hashes to the id it was served under. Substituting
//! a manifest or a chunk therefore fails its tag rather than quietly restoring
//! something else.
//!
//! # Ordering integrity lives here, not in the chunker
//!
//! A chunk carries no position. Plan 1-02 proved with a test that transposing
//! two whole `(id, ciphertext)` pairs cannot be detected at the chunk layer:
//! each pair still decrypts cleanly and still hashes to its own id, and the only
//! observable is a reordered buffer. CRYPTO-05 names reordering as something
//! that must be detected, so the detection is this module's job — the manifest's
//! ordered chunk list is the only place position exists at all.
//!
//! It is closed by construction rather than by a check: the list of ids sits
//! inside the manifest's sealed plaintext, so transposing two of them either
//! breaks the manifest's Poly1305 tag (if edited in place) or changes the
//! manifest's own id (if re-sealed), and the root names the old id. Either way
//! the reader gets an error and zero entries, never a reordered manifest.
//!
//! # A large bundle's manifest exceeds one chunk — a known Phase 2 boundary
//!
//! [`crate::sync::chunk::seal_chunk`] seals exactly one buffer of at most
//! [`CHUNK_SIZE`], and the milestone's chat-session-index category alone is
//! several thousand files. At roughly 135 bytes of JSON per entry — a path, a
//! mode, a length, and a 64-hex chunk id — a manifest passes 256 KiB somewhere
//! around 2,000 files, so a ~4,000-file bundle does **not** fit in one chunk.
//!
//! Phase 1 does not implement that split: [`Manifest::seal`] refuses an
//! oversized manifest **by name**, so Phase 2 hits a loud error rather than
//! inheriting a silent single-chunk assumption. Splitting it means a list of
//! ids where [`Root::manifest_id`] currently holds one, which is a format
//! change and therefore a Phase 2 decision, not a Phase 1 improvisation.
//!
//! Owned by plan 1-04.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::error::{AppError, Result};
use crate::sync::chunk::{open_chunk, seal_chunk};
use crate::sync::crypto::{ChunkId, KdfParams, Keys};
use crate::sync::{
    CHUNK_SIZE, CHUNKER_ID, INDEX_VERSION, MANIFEST_VERSION, MAX_SUPPORTED_INDEX,
    MAX_SUPPORTED_MANIFEST, MAX_SUPPORTED_ROOT, ROOT_VERSION, check_version,
};

/// Every chunker this build knows how to read.
///
/// Membership, deliberately, not equality with [`CHUNKER_ID`]: a build that
/// introduces a second chunker must still read the bundles it wrote with the
/// first. An equality check is the same unevolvable-format mistake as an
/// equality check on `format`.
const KNOWN_CHUNKERS: &[&str] = &[CHUNKER_ID];

fn check_chunker(found: &str) -> Result<()> {
    if KNOWN_CHUNKERS.contains(&found) {
        return Ok(());
    }
    // `found` comes from a remote, so it is bounded and `{:?}`-escaped before it
    // reaches a message. It is not secret — it is a chunker name, and the bytes
    // it arrived in have already authenticated.
    let shown: String = found.chars().take(32).collect();
    Err(AppError::Other(format!(
        "this bundle was split by an unknown chunker {shown:?} — \
         upgrade ai-usagebar to read it"
    )))
}

/// Just enough of any versioned object to read its `format`.
#[derive(Deserialize)]
struct VersionProbe {
    format: u32,
}

/// Check the version *before* deserializing the whole object.
///
/// A v2 object may carry fields this build has never heard of, and a required
/// one would make full deserialization fail with a confusing message about a
/// missing field. Probing first means the user is told the true problem: their
/// client is older than the bundle.
fn probe_version(json: &[u8], ceiling: u32, object: &str) -> Result<()> {
    let probe: VersionProbe = serde_json::from_slice(json)
        .map_err(|_| AppError::Other(format!("{object} is malformed")))?;
    check_version(probe.format, ceiling, object)
}

/// Serialize to JSON and seal it as an ordinary chunk.
///
/// No compression here: [`seal_chunk`] already runs zstd inside the frame, and
/// compressing twice costs CPU for nothing while adding a second determinism
/// surface to keep byte-stable.
fn seal_object<T: Serialize>(keys: &Keys, value: &T, object: &str) -> Result<(ChunkId, Vec<u8>)> {
    let json = Zeroizing::new(
        serde_json::to_vec(value)
            .map_err(|_| AppError::Other(format!("{object} serialization failed")))?,
    );
    if json.len() > CHUNK_SIZE {
        return Err(AppError::Other(format!(
            "this {object} serializes to {} bytes, past the {CHUNK_SIZE}-byte single-chunk \
             limit — splitting a {object} across chunks is not implemented",
            json.len()
        )));
    }
    let blob = seal_chunk(keys, &json)?;
    Ok((blob.id, blob.ciphertext))
}

/// Open a sealed object: AEAD tag, id recheck, version ceiling, then the shape.
fn open_object<T: DeserializeOwned>(
    keys: &Keys,
    id: &ChunkId,
    ciphertext: &[u8],
    ceiling: u32,
    object: &str,
) -> Result<T> {
    let json = open_chunk(keys, id, ciphertext)?;
    probe_version(&json, ceiling, object)?;
    serde_json::from_slice(&json).map_err(|_| AppError::Other(format!("{object} is malformed")))
}

/// One file in a snapshot, and the chunks it is made of **in order**.
///
/// `true_len` is the file's real length, which the sealed chunks do not carry:
/// the last one is padded to a power of two so its size reveals only a bucket.
///
/// `Debug` is derived and prints `path`. A path is metadata, not key material,
/// and no manifest ever leaves this process unsealed — the D5 rule is about key
/// material and plaintext file *contents*, neither of which lives here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub mode: u32,
    pub true_len: u64,
    pub chunks: Vec<ChunkId>,
}

/// The map from files to their ordered chunk ids.
///
/// Sealed as an ordinary chunk, deliberately: paths, sizes, modes, and the whole
/// directory shape are exactly the metadata a hostile remote would like, so none
/// of it may sit in the clear beside the data it describes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    pub chunker: String,
    pub files: Vec<FileEntry>,
}

impl Manifest {
    /// Stamp the version and chunker this build writes.
    ///
    /// Both come from the constants in [`crate::sync`] rather than a re-typed
    /// literal, so there is one source of truth for each.
    pub fn new(files: Vec<FileEntry>) -> Self {
        Self {
            format: MANIFEST_VERSION,
            chunker: CHUNKER_ID.to_string(),
            files,
        }
    }

    /// Seal to `(id, ciphertext)`. The id is what [`Root::manifest_id`] names.
    pub fn seal(&self, keys: &Keys) -> Result<(ChunkId, Vec<u8>)> {
        seal_object(keys, self, "manifest")
    }

    /// Open the manifest `id` names, refusing a version above this build's
    /// ceiling and a chunker it does not know.
    pub fn open(keys: &Keys, id: &ChunkId, ciphertext: &[u8]) -> Result<Manifest> {
        Self::open_with_ceiling(keys, id, ciphertext, MAX_SUPPORTED_MANIFEST)
    }

    /// The ceiling as a parameter, following the same seam as [`KdfParams`]:
    /// nothing below this line reads a constant it could instead be handed.
    fn open_with_ceiling(
        keys: &Keys,
        id: &ChunkId,
        ciphertext: &[u8],
        ceiling: u32,
    ) -> Result<Manifest> {
        let manifest: Manifest = open_object(keys, id, ciphertext, ceiling, "manifest")?;
        check_chunker(&manifest.chunker)?;
        Ok(manifest)
    }

    /// Every referenced chunk id that is not in `available`.
    ///
    /// Phase 5 restores through this so a dropped chunk is reported as a missing
    /// chunk. Skipping it and concatenating whatever arrived would write a
    /// *shorter* credential file and call it a success, which is the difference
    /// between a detected truncation and a silently corrupted secret.
    pub fn missing_chunks(&self, available: &HashSet<ChunkId>) -> Vec<ChunkId> {
        self.files
            .iter()
            .flat_map(|f| f.chunks.iter())
            .filter(|id| !available.contains(id))
            .copied()
            .collect()
    }
}

/// Where one sealed chunk lives inside a pack.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexEntry {
    pub id: ChunkId,
    pub pack: ChunkId,
    pub offset: u64,
    pub clen: u32,
    pub true_len: u32,
}

/// The chunk-id → pack-location map, sealed as an ordinary chunk like the
/// manifest.
///
/// `supersedes` names the index objects a repack replaced. Phase 4 deletes in
/// that order — an index must stop referencing a pack *before* the pack is
/// deleted, or a concurrent reader follows a pointer to nothing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexObject {
    pub format: u32,
    pub entries: Vec<IndexEntry>,
    pub supersedes: Vec<ChunkId>,
}

impl IndexObject {
    pub fn new(entries: Vec<IndexEntry>, supersedes: Vec<ChunkId>) -> Self {
        Self {
            format: INDEX_VERSION,
            entries,
            supersedes,
        }
    }

    pub fn seal(&self, keys: &Keys) -> Result<(ChunkId, Vec<u8>)> {
        seal_object(keys, self, "index object")
    }

    pub fn open(keys: &Keys, id: &ChunkId, ciphertext: &[u8]) -> Result<IndexObject> {
        open_object(keys, id, ciphertext, MAX_SUPPORTED_INDEX, "index object")
    }

    /// Linear scan. `ponytail:` fine at Phase 1 sizes; if a restore ever
    /// resolves thousands of ids against one index, build a `HashMap` once at
    /// the call site rather than growing a cache in here.
    pub fn resolve(&self, id: &ChunkId) -> Option<&IndexEntry> {
        self.entries.iter().find(|e| e.id == *id)
    }
}

/// The snapshot pointer: the one mutable object in the format.
///
/// `chunker` and `kdf` are informational duplicates — the authoritative copy of
/// the KDF parameters lives in the keyfile, where they are bound as associated
/// data and cannot be edited in transit. They are repeated here because the root
/// is the *first* object a reader touches, so an unknown chunker or an
/// unsupported KDF configuration can be refused before a single pack is fetched.
/// A mismatch between the two copies is a signal worth reporting rather than
/// silently preferring one: the keyfile wins, but the disagreement itself means
/// somebody rewrote something.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Root {
    pub format: u32,
    pub counter: u64,
    pub created_at: DateTime<Utc>,
    pub repo_id: String,
    pub manifest_id: ChunkId,
    pub chunker: String,
    pub kdf: KdfParams,
}

impl Root {
    /// `now` is a parameter and never a wall-clock read. Nothing in this module
    /// reads the clock, which is what lets every test pin a fixed timestamp and
    /// stay hermetic.
    pub fn new(
        counter: u64,
        now: DateTime<Utc>,
        repo_id: String,
        manifest_id: ChunkId,
        kdf: KdfParams,
    ) -> Self {
        Self {
            format: ROOT_VERSION,
            counter,
            created_at: now,
            repo_id,
            manifest_id,
            chunker: CHUNKER_ID.to_string(),
            kdf,
        }
    }

    /// Seal under the root subkey with a **fresh random** nonce.
    ///
    /// This is the one place the format's deterministic-nonce rule is inverted,
    /// and deliberately: every other object's nonce is derived from its content
    /// address because identical plaintext *must* seal identically or dedup
    /// dies. The root's plaintext changes on every sync, and a content-derived
    /// nonce would publish whether two consecutive snapshots are identical.
    pub fn seal(&self, keys: &Keys) -> Result<Vec<u8>> {
        let json = Zeroizing::new(
            serde_json::to_vec(self)
                .map_err(|_| AppError::Other("snapshot root serialization failed".into()))?,
        );
        keys.seal_root(&json)
    }

    /// Open a framed root, refusing a version above this build's ceiling and a
    /// chunker it does not know.
    ///
    /// `repo_id` pins the repository's identity inside the plaintext, so
    /// swapping the whole repository for a different one is detectable on top of
    /// the wrong keyfile simply failing to unwrap.
    pub fn open(keys: &Keys, framed: &[u8]) -> Result<Root> {
        let json = keys.open_root(framed)?;
        probe_version(&json, MAX_SUPPORTED_ROOT, "snapshot root")?;
        let root: Root = serde_json::from_slice(&json)
            .map_err(|_| AppError::Other("snapshot root is malformed".into()))?;
        check_chunker(&root.chunker)?;
        Ok(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::crypto::Keyfile;

    /// Microseconds instead of ~1.5 s and a gibibyte. Never use production
    /// parameters in a unit test: the AUR `check()` runs these on an
    /// installer's machine.
    const CHEAP: KdfParams = KdfParams {
        m_kib: 8,
        t: 1,
        p: 1,
    };

    fn keys_from(password: &[u8]) -> Keys {
        Keyfile::create(password, CHEAP)
            .expect("keyfile creation")
            .1
    }

    fn keys() -> Keys {
        keys_from(b"correct horse battery staple")
    }

    /// Fixed, injected, never `Utc::now()`.
    fn fixed_time() -> DateTime<Utc> {
        "2026-08-19T12:00:00Z".parse().expect("a valid timestamp")
    }

    fn id(n: u8) -> ChunkId {
        ChunkId::from_bytes([n; 32])
    }

    fn three_files() -> Vec<FileEntry> {
        vec![
            FileEntry {
                path: ".claude/.credentials.json".into(),
                mode: 0o600,
                true_len: 812,
                chunks: vec![id(1)],
            },
            FileEntry {
                path: ".config/ai-usagebar/config.toml".into(),
                mode: 0o600,
                true_len: 300_000,
                chunks: vec![id(2), id(3)],
            },
            FileEntry {
                path: ".local/share/ai-usagebar/history.jsonl".into(),
                mode: 0o644,
                true_len: 0,
                chunks: vec![],
            },
        ]
    }

    // ---- manifest -------------------------------------------------------

    #[test]
    fn a_three_file_manifest_seals_and_reopens_identically() {
        let keys = keys();
        let manifest = Manifest::new(three_files());
        let (id, ciphertext) = manifest.seal(&keys).expect("seal");
        assert_eq!(
            Manifest::open(&keys, &id, &ciphertext).expect("open"),
            manifest
        );
    }

    #[test]
    fn a_manifest_opened_under_the_wrong_id_is_refused() {
        let keys = keys();
        let (id, ciphertext) = Manifest::new(three_files()).seal(&keys).expect("seal");
        let mut wrong = *id.as_bytes();
        wrong[0] ^= 1;
        let wrong = ChunkId::from_bytes(wrong);
        assert!(Manifest::open(&keys, &wrong, &ciphertext).is_err());
    }

    #[test]
    fn a_manifest_written_by_an_older_client_opens_when_the_ceiling_is_raised() {
        let keys = keys();
        let manifest = Manifest::new(three_files());
        assert_eq!(manifest.format, 1);
        let (id, ciphertext) = manifest.seal(&keys).expect("seal");
        // A future build with MAX_SUPPORTED_MANIFEST = 2 must still read this
        // v1 bundle. Equality against the current version would break it.
        let reopened = Manifest::open_with_ceiling(&keys, &id, &ciphertext, 2).expect("open");
        assert_eq!(reopened, manifest);
    }

    #[test]
    fn a_manifest_one_version_above_the_ceiling_is_refused() {
        let keys = keys();
        let future = Manifest {
            format: MAX_SUPPORTED_MANIFEST + 1,
            chunker: CHUNKER_ID.to_string(),
            files: three_files(),
        };
        let (id, ciphertext) = future.seal(&keys).expect("seal");
        let err = Manifest::open(&keys, &id, &ciphertext).expect_err("must refuse");
        assert!(err.to_string().contains("upgrade ai-usagebar"));
    }

    #[test]
    fn a_manifest_naming_an_unknown_chunker_is_refused_and_the_message_names_it() {
        let keys = keys();
        let odd = Manifest {
            format: MANIFEST_VERSION,
            chunker: "rolling-cdc-v9".into(),
            files: three_files(),
        };
        let (id, ciphertext) = odd.seal(&keys).expect("seal");
        let err = Manifest::open(&keys, &id, &ciphertext).expect_err("must refuse");
        assert!(err.to_string().contains("rolling-cdc-v9"));
    }

    #[test]
    fn the_sealed_manifest_carries_no_file_path_in_the_clear() {
        let keys = keys();
        let (_, ciphertext) = Manifest::new(three_files()).seal(&keys).expect("seal");
        for needle in [
            b".credentials.json".as_slice(),
            b"config.toml".as_slice(),
            b"history.jsonl".as_slice(),
        ] {
            assert!(
                !ciphertext.windows(needle.len()).any(|w| w == needle),
                "a file path leaked into the sealed manifest"
            );
        }
    }

    #[test]
    fn the_chunker_reads_back_as_the_constant_this_build_writes() {
        let keys = keys();
        let (id, ciphertext) = Manifest::new(three_files()).seal(&keys).expect("seal");
        let reopened = Manifest::open(&keys, &id, &ciphertext).expect("open");
        assert_eq!(reopened.chunker, "fixed-256k");
        assert_eq!(reopened.chunker, CHUNKER_ID);
    }

    /// Ordering integrity is this module's own requirement — the chunk layer
    /// demonstrably cannot provide it (1-02 Deviation 2). This is the shape
    /// 1-06 Attack 8 asserts against.
    #[test]
    fn transposing_two_chunk_ids_inside_a_sealed_manifest_yields_zero_entries() {
        let keys = keys();
        let ordered = Manifest::new(vec![FileEntry {
            path: "a.jsonl".into(),
            mode: 0o600,
            true_len: 900_000,
            chunks: vec![id(1), id(2), id(3), id(4)],
        }]);
        let (manifest_id, ciphertext) = ordered.seal(&keys).expect("seal");

        let mut transposed = ordered.clone();
        transposed.files[0].chunks.swap(1, 2);
        let (evil_id, evil_ciphertext) = transposed.seal(&keys).expect("seal");

        // Re-sealing a reordered list produces a *different* address, so the
        // root that names `manifest_id` never points at it...
        assert_ne!(evil_id, manifest_id);
        // ...and serving it under the original id fails outright.
        assert!(Manifest::open(&keys, &manifest_id, &evil_ciphertext).is_err());

        // Editing the ordered list in place breaks the tag just as hard.
        let mut flipped = ciphertext.clone();
        flipped[0] ^= 1;
        assert!(Manifest::open(&keys, &manifest_id, &flipped).is_err());

        // And the honest manifest still opens with its order intact.
        let reopened = Manifest::open(&keys, &manifest_id, &ciphertext).expect("open");
        assert_eq!(reopened.files[0].chunks, vec![id(1), id(2), id(3), id(4)]);
    }

    #[test]
    fn a_referenced_chunk_that_is_absent_is_reported_missing_not_skipped() {
        let manifest = Manifest::new(three_files());
        let available: HashSet<ChunkId> = [id(1), id(3)].into_iter().collect();
        assert_eq!(manifest.missing_chunks(&available), vec![id(2)]);

        let all: HashSet<ChunkId> = [id(1), id(2), id(3)].into_iter().collect();
        assert!(manifest.missing_chunks(&all).is_empty());
    }

    fn many_files(n: u32) -> Vec<FileEntry> {
        (0..n)
            .map(|i| FileEntry {
                path: format!("chat-sessions/2026-08-19/session-{i:06}.jsonl"),
                mode: 0o600,
                true_len: 4096,
                chunks: vec![id((i % 251) as u8)],
            })
            .collect()
    }

    #[test]
    fn a_thousand_file_manifest_still_fits_in_one_chunk_and_round_trips() {
        let keys = keys();
        let manifest = Manifest::new(many_files(1_000));
        let (id, ciphertext) = manifest.seal(&keys).expect("seal");
        assert_eq!(
            Manifest::open(&keys, &id, &ciphertext).expect("open"),
            manifest
        );
    }

    /// The Phase 2 boundary, exercised now rather than discovered later: a
    /// realistic chat-session-index manifest does not fit in one chunk, and the
    /// refusal says so by name instead of truncating.
    #[test]
    fn a_four_thousand_file_manifest_exceeds_one_chunk_and_is_refused_by_name() {
        let keys = keys();
        let manifest = Manifest::new(many_files(4_000));
        assert!(
            serde_json::to_vec(&manifest).expect("serialize").len() > CHUNK_SIZE,
            "the fixture must actually exceed one chunk for this test to mean anything"
        );
        let err = manifest.seal(&keys).expect_err("must refuse");
        assert!(err.to_string().contains("single-chunk"));
    }

    // ---- snapshot root --------------------------------------------------

    fn a_root() -> Root {
        Root::new(
            7,
            fixed_time(),
            "usagebar-sync-abc123".into(),
            id(9),
            KdfParams::default(),
        )
    }

    #[test]
    fn a_root_seals_and_reopens_with_every_field_intact() {
        let keys = keys();
        let root = a_root();
        let reopened = Root::open(&keys, &root.seal(&keys).expect("seal")).expect("open");
        assert_eq!(reopened, root);
        assert_eq!(reopened.counter, 7);
        assert_eq!(reopened.manifest_id, id(9));
        assert_eq!(reopened.chunker, CHUNKER_ID);
        assert_eq!(reopened.kdf, KdfParams::default());
        assert_eq!(reopened.created_at, fixed_time());
        assert_eq!(reopened.repo_id, "usagebar-sync-abc123");
    }

    #[test]
    fn two_seals_of_one_root_differ_yet_both_reopen() {
        let keys = keys();
        let root = a_root();
        let first = root.seal(&keys).expect("seal");
        let second = root.seal(&keys).expect("seal");
        assert_ne!(
            first, second,
            "the root nonce must be random, unlike a chunk's"
        );
        assert_eq!(Root::open(&keys, &first).expect("open"), root);
        assert_eq!(Root::open(&keys, &second).expect("open"), root);
    }

    #[test]
    fn a_root_does_not_open_under_a_different_master_key() {
        let framed = a_root().seal(&keys()).expect("seal");
        let stranger = keys_from(b"a completely different passphrase");
        assert!(Root::open(&stranger, &framed).is_err());
    }

    #[test]
    fn one_flipped_bit_anywhere_in_a_sealed_root_fails_to_open() {
        let keys = keys();
        let framed = a_root().seal(&keys).expect("seal");
        for byte in 0..framed.len() {
            let mut tampered = framed.clone();
            tampered[byte] ^= 1;
            assert!(
                Root::open(&keys, &tampered).is_err(),
                "a flipped bit at byte {byte} was accepted"
            );
        }
    }

    #[test]
    fn a_root_naming_an_unknown_chunker_is_refused_and_the_message_names_it() {
        let keys = keys();
        let mut root = a_root();
        root.chunker = "cdc-gear-64k".into();
        let err = Root::open(&keys, &root.seal(&keys).expect("seal")).expect_err("must refuse");
        assert!(err.to_string().contains("cdc-gear-64k"));
    }

    #[test]
    fn a_root_below_the_ceiling_opens_and_one_above_it_is_refused() {
        let keys = keys();
        let mut older = a_root();
        older.format = MAX_SUPPORTED_ROOT - 1;
        assert!(Root::open(&keys, &older.seal(&keys).expect("seal")).is_ok());

        let mut newer = a_root();
        newer.format = MAX_SUPPORTED_ROOT + 1;
        let err = Root::open(&keys, &newer.seal(&keys).expect("seal")).expect_err("must refuse");
        assert!(err.to_string().contains("upgrade ai-usagebar"));
    }

    // ---- index object ---------------------------------------------------

    #[test]
    fn an_index_object_round_trips_and_resolves_a_known_chunk() {
        let keys = keys();
        let index = IndexObject::new(
            vec![
                IndexEntry {
                    id: id(1),
                    pack: id(200),
                    offset: 0,
                    clen: 4_096,
                    true_len: 3_000,
                },
                IndexEntry {
                    id: id(2),
                    pack: id(200),
                    offset: 4_096,
                    clen: 8_192,
                    true_len: 262_144,
                },
            ],
            vec![id(50), id(51)],
        );
        let (chunk_id, ciphertext) = index.seal(&keys).expect("seal");
        let reopened = IndexObject::open(&keys, &chunk_id, &ciphertext).expect("open");
        assert_eq!(reopened, index);
        assert_eq!(reopened.supersedes, vec![id(50), id(51)]);

        let found = reopened.resolve(&id(2)).expect("a known chunk resolves");
        assert_eq!(found.pack, id(200));
        assert_eq!(found.offset, 4_096);
        assert_eq!(found.clen, 8_192);
        assert_eq!(found.true_len, 262_144);
        assert!(reopened.resolve(&id(99)).is_none());
    }

    #[test]
    fn an_index_object_above_the_ceiling_is_refused() {
        let keys = keys();
        let future = IndexObject {
            format: MAX_SUPPORTED_INDEX + 1,
            entries: vec![],
            supersedes: vec![],
        };
        let (chunk_id, ciphertext) = future.seal(&keys).expect("seal");
        let err = IndexObject::open(&keys, &chunk_id, &ciphertext).expect_err("must refuse");
        assert!(err.to_string().contains("upgrade ai-usagebar"));
    }
}
