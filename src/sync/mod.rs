//! Encrypted sync bundle format — client-side encryption for pushing local
//! state to a private git remote that is treated as fully hostile.
//!
//! Everything under this module is **pure and offline**: no network, no `$HOME`,
//! no Keychain, no git. The whole format is exercisable by `cargo test` on a
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

pub mod anchor;
pub mod chunk;
pub mod crypto;
pub mod model;
pub mod pack;
pub mod passphrase;

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
pub const MANIFEST_VERSION: u32 = 1;
/// Highest manifest version this build can read.
pub const MAX_SUPPORTED_MANIFEST: u32 = 1;

/// Snapshot-root version written by this build.
pub const ROOT_VERSION: u32 = 1;
/// Highest snapshot-root version this build can read.
pub const MAX_SUPPORTED_ROOT: u32 = 1;

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
