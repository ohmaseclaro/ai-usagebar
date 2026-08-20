//! Encrypted sync bundle format — client-side encryption for pushing local
//! state to a private git remote that is treated as fully hostile.
//!
//! Everything under this module is **pure and offline**: no network, no `$HOME`,
//! no Keychain, no git — with the single exception of [`github`], which is the
//! transport and says so. The whole format is exercisable by `cargo test` on a
//! machine with none of those, which is what lets it be adversary-tested before
//! anything can transmit it.
//!
//! Layout — one file per plan, so parallel work never collides:
//! - [`crypto`] — key hierarchy and every AEAD call. The only module here that
//!   imports `argon2` or `chacha20poly1305`.
//! - [`chunk`] — fixed-size chunking, compression, framing.
//! - [`pack`] — packing sealed blobs into remote-sized files.
//! - [`model`] — snapshot root, manifest, and index objects.
//! - [`passphrase`] — passphrase generation and strength floor.
//! - [`anchor`] — the local monotonic rollback anchor.
//! - [`scope`] — the one bounded, symlink-refusing walker plus the D2
//!   exclusion predicate every category funnels through.
//! - [`transcripts`] — the bounded transcript selector.
//! - [`index`] — the local `(path, size, mtime_ns, inode)` change-detection db.
//! - [`plan`] — dry-run planning over a scan.
//! - [`github`] — the GitHub transport: auth, the private-repo gate, and the
//!   pairing record. The only module here that opens a socket — and in Phase 3
//!   it can only `GET`.
//! - [`push`] — the outbound path: packing, uploading, and the one
//!   compare-and-swap that publishes a snapshot. The only module that can
//!   *change* the remote.
//! - [`report`] — the pure `sync status` model and its renderer.
//! - [`cli`] — the `ai-usagebar sync …` entry point.
//!
//! The scanning half of this module touches the filesystem, but only through
//! [`SyncRoots`], whose every root is injected — no test here reads a real
//! `$HOME`.

pub mod anchor;
pub mod chunk;
pub mod cli;
pub mod crypto;
pub mod github;
pub mod index;
pub mod model;
pub mod pack;
pub mod passphrase;
pub mod plan;
pub mod push;
pub mod report;
pub mod scope;
pub mod transcripts;

use std::path::PathBuf;

use crate::config::Config;
use crate::error::{AppError, Result};

/// Fixed chunk size. Fixed-size, *not* content-defined: CDC boundary positions
/// are visible as ciphertext lengths and fingerprint the plaintext
/// (arXiv:2504.02095). The payload here is append-only JSONL and page-aligned
/// SQLite, so fixed blocks aligned to each file's start dedup just as well.
pub const CHUNK_SIZE: usize = 256 * 1024;

/// Recorded in the snapshot header so a future chunker can be introduced
/// without guessing how existing bundles were split.
pub const CHUNKER_ID: &str = "fixed-256k";

// BLAKE3 `derive_key` context strings. BLAKE3's contract is that these are
// hardcoded, application-specific and globally unique. The `v1` token is
// load-bearing: a v2 key hierarchy must not be able to collide with v1's.
pub const CTX_CHUNK: &str = "ai-usagebar.sync.v1 chunk-encryption-key";
pub const CTX_NAME: &str = "ai-usagebar.sync.v1 chunk-name-key";
pub const CTX_ROOT: &str = "ai-usagebar.sync.v1 snapshot-root-key";
pub const CTX_NONCE: &str = "ai-usagebar.sync.v1 chunk-nonce";

// Versioning. Every versioned object carries *two* numbers: the version this
// build writes, and the highest version it can read. Readers accept anything at
// or below their ceiling and refuse only what is greater — see `check_version`.
// An equality check would mean a v2 client could not read a v1 bundle, which
// inverts the "raise the KDF parameters without breaking existing bundles"
// promise the format exists to keep.

/// Keyfile version written by this build.
pub const KEYFILE_VERSION: u32 = 1;
/// Highest keyfile version this build can read.
pub const MAX_SUPPORTED_KEYFILE: u32 = 1;

/// Manifest version written by this build.
///
/// v2 carries the manifest across as many chunks as it needs; v1 assumed one.
pub const MANIFEST_VERSION: u32 = 2;
/// Highest manifest version this build can read.
pub const MAX_SUPPORTED_MANIFEST: u32 = 2;

/// Snapshot-root version written by this build.
///
/// v2 names the manifest with an ordered list of chunk ids; v1 named a single
/// id, which could not express a manifest past [`CHUNK_SIZE`].
pub const ROOT_VERSION: u32 = 2;
/// Highest snapshot-root version this build can read.
pub const MAX_SUPPORTED_ROOT: u32 = 2;

/// Index-object version written by this build.
pub const INDEX_VERSION: u32 = 1;
/// Highest index-object version this build can read.
pub const MAX_SUPPORTED_INDEX: u32 = 1;

/// Pack-header version written by this build.
pub const PACK_HEADER_VERSION: u32 = 1;
/// Highest pack-header version this build can read.
pub const MAX_SUPPORTED_PACK_HEADER: u32 = 1;

/// Accept any format version at or below `ceiling`; refuse only what is above
/// it, and say plainly that the *client* is the old thing, not the data.
///
/// `object` names the thing being read ("keyfile", "manifest", …) and is a
/// compile-time string, never user or attacker data.
pub fn check_version(found: u32, ceiling: u32, object: &str) -> Result<()> {
    if found <= ceiling {
        return Ok(());
    }
    Err(AppError::Other(format!(
        "this {object} was written at format version {found}, but this build of \
         ai-usagebar reads at most version {ceiling} — upgrade ai-usagebar to read it"
    )))
}

/// Every filesystem root the collectors are allowed to look at.
///
/// The same seam as [`crate::claude_desktop::Paths`]: [`SyncRoots::at`] is what
/// every test constructs, [`SyncRoots::resolve`] is the one production wrapper
/// that touches `$HOME`. Nothing under [`scope`] resolves a path itself, so a
/// collector physically cannot wander outside what it was handed.
#[derive(Debug, Clone)]
pub struct SyncRoots {
    /// The effective `config.toml`.
    pub config_file: PathBuf,
    /// Its parent — where `accounts/<label>/.credentials.json` lives.
    pub config_dir: PathBuf,
    /// Claude Desktop's data dir, parent of `claude-code-sessions`.
    pub desktop_data_dir: PathBuf,
    /// The claude-acc profile store, `~/.claude-acc/profiles`.
    pub desktop_profiles_dir: PathBuf,
    /// `~/.claude`, parent of `scheduled-tasks/` and `projects/`.
    pub claude_home: PathBuf,
    /// The change-detection index db. In the *cache* dir in production — it is
    /// a wipeable hint, not durable state — and inside the injected tree under
    /// [`SyncRoots::at`], so no test writes to an installer's real `$XDG`.
    pub index_file: PathBuf,
}

impl SyncRoots {
    /// Test seam: every root explicit.
    ///
    /// [`index_file`](SyncRoots::index_file) is the one exception — derived
    /// from `config_dir` rather than passed, so the six existing callers keep
    /// compiling and every one of them still lands inside its own `TempDir`.
    /// Production never takes this path; [`resolve`](SyncRoots::resolve) puts
    /// the index in the cache directory where plan 2-01 put it.
    pub fn at(
        config_file: PathBuf,
        config_dir: PathBuf,
        desktop_data_dir: PathBuf,
        desktop_profiles_dir: PathBuf,
        claude_home: PathBuf,
    ) -> Self {
        Self {
            config_file,
            index_file: config_dir.join("sync").join("index.sqlite3"),
            config_dir,
            desktop_data_dir,
            desktop_profiles_dir,
            claude_home,
        }
    }

    /// Production paths, all derived from resolvers that already exist. No new
    /// config knob: a second path to the same tree is a second thing to get
    /// wrong.
    pub fn resolve(config: &Config) -> Result<Self> {
        let config_file = crate::config::resolved_path().ok_or_else(|| {
            AppError::Other(
                "could not resolve the ai-usagebar config directory (no HOME?) — \
                 sync needs to know where config.toml lives"
                    .into(),
            )
        })?;
        let config_dir = config_file
            .parent()
            .ok_or_else(|| {
                AppError::Other(format!(
                    "config path has no parent directory: {}",
                    config_file.display()
                ))
            })?
            .to_path_buf();
        let desktop = crate::claude_desktop::Paths::resolve(&config.anthropic)?;
        Ok(Self {
            config_file,
            config_dir,
            desktop_data_dir: desktop.data_dir,
            desktop_profiles_dir: desktop.profiles_dir,
            claude_home: crate::cache::home_dir()?.join(".claude"),
            index_file: index::default_path()?,
        })
    }
}

/// The one recursive source walk every structural guard in this module tree
/// uses.
///
/// **It exists because two guards were blind and nobody could see it.** Phase
/// 4's audit found `crypto.rs`'s crypto-import invariant walking `src/sync` with
/// a non-recursive `read_dir`, leaving `push/` and `github/` — 11,500 lines —
/// entirely outside the invariant whose stated value is "what lets a security
/// auditor read one file instead of six"; and `passphrase.rs`'s
/// environment-read guard iterating a **hand-maintained list** of three files
/// that had not been extended to `push/rekey.rs`, the file its own critical
/// threat is about. That second one was Phase 3's F-8 recurring verbatim, one
/// phase later, because the remediation had added a file to the list rather than
/// making the list unnecessary.
///
/// A guard that enumerates what to check fails open on everything added after
/// it. A guard that walks fails *closed*: a new file is scanned by default and
/// an exemption has to be written down.
#[cfg(test)]
pub(crate) mod guard {
    use std::path::{Path, PathBuf};

    /// Every `.rs` file under `dir`, recursively.
    pub(crate) fn rs_files(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        walk(dir, &mut out);
        out
    }

    /// Every `.rs` file under `CARGO_MANIFEST_DIR`-relative `rel`.
    ///
    /// Resolved from the manifest directory rather than from a relative path, so
    /// a guard is independent of the working directory and survives the AUR
    /// `srcdir` layout.
    pub(crate) fn rs_files_in(rel: &str) -> Vec<PathBuf> {
        rs_files(&Path::new(env!("CARGO_MANIFEST_DIR")).join(rel))
    }

    /// Everything before a file's own `#[cfg(test)]`. A test that names a needle
    /// is a test, not a violation.
    pub(crate) fn production_code(source: &str) -> &str {
        source.split("#[cfg(test)]").next().unwrap_or_default()
    }

    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("a readable source directory") {
            let path = entry.expect("a readable directory entry").path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_check_accepts_at_or_below_the_ceiling_and_refuses_only_above() {
        assert!(check_version(0, 1, "keyfile").is_ok());
        assert!(check_version(1, 1, "keyfile").is_ok());
        let err = check_version(2, 1, "keyfile").expect_err("above the ceiling must be refused");
        assert!(err.to_string().contains("upgrade ai-usagebar"));
    }
}
