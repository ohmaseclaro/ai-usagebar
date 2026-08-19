//! The key hierarchy and every AEAD call in the encrypted-sync bundle format.
//!
//! ```text
//!     password + salt(16B, in the keyfile)
//!         |  Argon2id  m=1 GiB  t=3  p=1
//!     kek [32B]                                  ephemeral, zeroized
//!         |  XChaCha20-Poly1305 unwrap
//!         |  aad = canonical {format, kdf}       binds params, blocks downgrade
//!     master key [32B]                           random at init, never on disk
//!         |  BLAKE3 derive_key, one context string each
//!     chunk_key        name_key        root_key
//! ```
//!
//! **Containment invariant:** this is the only module under `src/sync/` that
//! imports `argon2` or `chacha20poly1305`. The test
//! `only_the_crypto_module_imports_the_cryptographic_crates` enforces it, so a
//! security review of the sync format is a review of this one file.
//!
//! Everything here is pure — no path, no env, no clock, no network. Every entry
//! point takes [`KdfParams`] by argument rather than reading a default inside
//! itself; that is the cheap-KDF test seam. Tests pass `{ m_kib: 8, t: 1, p: 1 }`
//! and run in microseconds, which keeps the AUR `check()` inside its time budget.
//! Production passes [`KdfParams::default`].
//!
//! Secret hygiene is structural, not aspirational: key material lives in
//! `Zeroizing`, [`Keys`] has a hand-written `Debug` that prints `<redacted>`,
//! and no error message here interpolates a key, a password, a nonce, or a
//! plaintext.

use std::fmt;
use std::str::FromStr;

use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine;
use chacha20poly1305::XChaCha20Poly1305;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use zeroize::{Zeroize, Zeroizing};

use crate::error::{AppError, Result};
use crate::sync::{
    CTX_CHUNK, CTX_NAME, CTX_NONCE, CTX_ROOT, KEYFILE_VERSION, MAX_SUPPORTED_KEYFILE, check_version,
};

/// XChaCha20-Poly1305 nonce width.
const NONCE_LEN: usize = 24;
/// Poly1305 tag width.
const TAG_LEN: usize = 16;

const B64: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::STANDARD;

/// Argon2id parameters. Stored in cleartext in the keyfile *and* bound into the
/// wrap as associated data, so downgrading `m_kib` in transit does not produce a
/// KEK that unwraps.
///
/// Passed by argument everywhere. No derivation function may read a default
/// from inside itself — that is what makes the cheap-KDF test seam possible.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfParams {
    /// Memory cost in KiB.
    pub m_kib: u32,
    /// Time cost (passes).
    pub t: u32,
    /// Lanes. `1` because `argon2` 0.5 has no threading, so `p > 1` measures
    /// ~10% *worse* for the defender while handing a wide-SIMD attacker free
    /// intra-hash parallelism.
    pub p: u32,
}

impl Default for KdfParams {
    fn default() -> Self {
        Self {
            m_kib: 1_048_576,
            t: 3,
            p: 1,
        }
    }
}

/// Password + salt -> key-encryption key.
pub fn derive_kek(pw: &[u8], salt: &[u8; 16], k: KdfParams) -> Result<Zeroizing<[u8; 32]>> {
    let params = Params::new(k.m_kib, k.t, k.p, Some(32)).map_err(|_| {
        // The parameters are public (they live in cleartext in the keyfile), so
        // naming them is safe and is the only actionable thing to say.
        AppError::Other(format!(
            "invalid Argon2id parameters (m_kib={}, t={}, p={})",
            k.m_kib, k.t, k.p
        ))
    })?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut out = Zeroizing::new([0u8; 32]);
    argon
        .hash_password_into(pw, salt, out.as_mut())
        .map_err(|_| AppError::Other("key derivation failed".into()))?;
    Ok(out)
}

/// A chunk's address. An address, not a secret — deriving `Debug` is fine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChunkId([u8; 32]);

impl ChunkId {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for ChunkId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl FromStr for ChunkId {
    type Err = AppError;

    fn from_str(s: &str) -> Result<Self> {
        // `is_ascii` also guarantees every 2-byte slice below lands on a char
        // boundary, so the indexing cannot panic.
        if s.len() != 64 || !s.is_ascii() {
            return Err(AppError::Other("chunk id must be 64 hex characters".into()));
        }
        let mut out = [0u8; 32];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = u8::from_str_radix(&s[2 * i..2 * i + 2], 16)
                .map_err(|_| AppError::Other("chunk id is not valid hex".into()))?;
        }
        Ok(Self(out))
    }
}

impl Serialize for ChunkId {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ChunkId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

/// The three subkeys derived from the master key. Private fields, and a
/// hand-written `Debug`: deriving it would put key material one `{:?}` away
/// from a log line.
pub struct Keys {
    chunk: Zeroizing<[u8; 32]>,
    name: Zeroizing<[u8; 32]>,
    root: Zeroizing<[u8; 32]>,
}

impl fmt::Debug for Keys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Keys { <redacted> }")
    }
}

/// One context string per field. Getting this mapping wrong is invisible to
/// every round-trip test — it stays self-consistent under any wrong-but-stable
/// permutation — which is why 1-07 pins all three as known-answer vectors.
fn subkeys(mk: &[u8; 32]) -> Keys {
    Keys {
        chunk: Zeroizing::new(blake3::derive_key(CTX_CHUNK, mk)),
        name: Zeroizing::new(blake3::derive_key(CTX_NAME, mk)),
        root: Zeroizing::new(blake3::derive_key(CTX_ROOT, mk)),
    }
}

/// The KDF block of the keyfile, in cleartext. Field order here *is* the
/// canonical AAD byte order (see [`aad_bytes`]), so do not reorder it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfDoc {
    pub algo: String,
    pub version: u32,
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
    /// base64 of the 16 random salt bytes.
    pub salt: String,
}

impl KdfDoc {
    fn new(salt: &[u8; 16], k: KdfParams) -> Self {
        Self {
            algo: "argon2id".into(),
            version: 19,
            m_kib: k.m_kib,
            t: k.t,
            p: k.p,
            salt: B64.encode(salt),
        }
    }

    /// The parameters recorded in *this* keyfile. Callers must use these, never
    /// [`KdfParams::default`] — that is the whole point of storing them.
    pub fn params(&self) -> KdfParams {
        KdfParams {
            m_kib: self.m_kib,
            t: self.t,
            p: self.p,
        }
    }

    fn salt_bytes(&self) -> Result<[u8; 16]> {
        let raw = B64
            .decode(&self.salt)
            .map_err(|_| AppError::Other("keyfile salt is not valid base64".into()))?;
        raw.try_into()
            .map_err(|_| AppError::Other("keyfile salt is not 16 bytes".into()))
    }
}

/// Canonical AEAD associated data for the master-key wrap: `{format, kdf}`
/// serialized as a *struct*, so the byte order is the declaration order above.
/// A map would not guarantee ordering, and an AAD whose bytes depend on hash
/// iteration order is an AAD that intermittently fails to authenticate.
fn aad_bytes(format: u32, kdf: &KdfDoc) -> Result<Vec<u8>> {
    #[derive(Serialize)]
    struct KeyfileAad<'a> {
        format: u32,
        kdf: &'a KdfDoc,
    }
    serde_json::to_vec(&KeyfileAad { format, kdf })
        .map_err(|_| AppError::Other("failed to canonicalize the keyfile parameters".into()))
}

/// The on-disk keyfile: KDF parameters in cleartext plus the wrapped master
/// key. It holds no plaintext key material — the wrapped key is ciphertext and
/// the salt is public — so deriving `Debug` here is safe.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Keyfile {
    pub format: u32,
    pub kdf: KdfDoc,
    /// base64 of the 24-byte wrap nonce.
    pub nonce: String,
    /// base64 of the 48-byte wrapped master key (32 ciphertext + 16 tag).
    pub wrapped_master_key: String,
}

impl Keyfile {
    /// Draw a fresh master key and wrap it under `pw`. Returns both the keyfile
    /// to persist and the live subkeys, so the caller never pays the KDF twice.
    pub fn create(pw: &[u8], k: KdfParams) -> Result<(Keyfile, Keys)> {
        let mut mk = Zeroizing::new([0u8; 32]);
        fill(&mut mk[..])?;
        let keyfile = Self::wrap(&mk, pw, k)?;
        Ok((keyfile, subkeys(&mk)))
    }

    /// Unwrap the master key with `pw` and derive the subkeys.
    ///
    /// Uses **the parameters stored in this keyfile**, never
    /// [`KdfParams::default`]: a bundle initialised at a lower `--kdf-memory`
    /// must stay openable, and a bundle initialised higher must stay strong.
    pub fn open(&self, pw: &[u8]) -> Result<Keys> {
        Ok(subkeys(&*self.unwrap_master_key(pw)?))
    }

    /// Password change without re-encrypting any data: unwrap under the old
    /// password, rewrap the *same* master key under the new one. 48 bytes
    /// rewritten instead of the whole bundle.
    ///
    /// This is the CRYPTO-04 primitive only. The observable requirement also
    /// needs the old keyfile asset deleted from the remote, which Phase 4 owns —
    /// and even then, a keyfile already in git history stays unwrappable with
    /// the old password forever. Password change is not revocation.
    pub fn rewrap(&self, old_pw: &[u8], new_pw: &[u8], k: KdfParams) -> Result<Keyfile> {
        let mk = self.unwrap_master_key(old_pw)?;
        Self::wrap(&mk, new_pw, k)
    }

    fn wrap(mk: &[u8; 32], pw: &[u8], k: KdfParams) -> Result<Keyfile> {
        let mut salt = [0u8; 16];
        let mut nonce = [0u8; NONCE_LEN];
        fill(&mut salt)?;
        fill(&mut nonce)?;

        let kdf = KdfDoc::new(&salt, k);
        let aad = aad_bytes(KEYFILE_VERSION, &kdf)?;
        let kek = derive_kek(pw, &salt, k)?;
        let wrapped = XChaCha20Poly1305::new((&*kek).into())
            .encrypt(
                &nonce.into(),
                Payload {
                    msg: &mk[..],
                    aad: &aad,
                },
            )
            .map_err(|_| AppError::Other("failed to wrap the master key".into()))?;

        Ok(Keyfile {
            format: KEYFILE_VERSION,
            kdf,
            nonce: B64.encode(nonce),
            wrapped_master_key: B64.encode(&wrapped),
        })
    }

    fn unwrap_master_key(&self, pw: &[u8]) -> Result<Zeroizing<[u8; 32]>> {
        // Version gate *before* any cryptographic work: refusing a too-new
        // bundle must not cost 1.5 s and a gibibyte first.
        check_version(self.format, MAX_SUPPORTED_KEYFILE, "keyfile")?;

        let salt = self.kdf.salt_bytes()?;
        let nonce = self.nonce_bytes()?;
        let wrapped = B64
            .decode(&self.wrapped_master_key)
            .map_err(|_| AppError::Other("keyfile wrapped key is not valid base64".into()))?;
        let aad = self.aad()?;

        let kek = derive_kek(pw, &salt, self.kdf.params())?;
        let mut opened = XChaCha20Poly1305::new((&*kek).into())
            .decrypt(
                &nonce.into(),
                Payload {
                    msg: &wrapped,
                    aad: &aad,
                },
            )
            // A wrong password and a downgraded `m_kib` both land here, and both
            // get the same message: there is nothing useful to distinguish, and
            // an attacker learns nothing from the difference.
            .map_err(|_| AppError::Other("wrong password or corrupted keyfile".into()))?;

        if opened.len() != 32 {
            opened.zeroize();
            return Err(AppError::Other(
                "wrong password or corrupted keyfile".into(),
            ));
        }
        let mut mk = Zeroizing::new([0u8; 32]);
        mk.copy_from_slice(&opened);
        // The allocating AEAD API hands back a plain `Vec` holding the master
        // key and does not zeroize it for us (research §7.1). A `Vec` that
        // reallocated during construction also leaves an unreachable copy
        // behind — unavoidable with this API, and documented as accepted.
        opened.zeroize();
        Ok(mk)
    }

    fn nonce_bytes(&self) -> Result<[u8; NONCE_LEN]> {
        let raw = B64
            .decode(&self.nonce)
            .map_err(|_| AppError::Other("keyfile nonce is not valid base64".into()))?;
        raw.try_into()
            .map_err(|_| AppError::Other("keyfile nonce is not 24 bytes".into()))
    }

    fn aad(&self) -> Result<Vec<u8>> {
        aad_bytes(self.format, &self.kdf)
    }
}

impl Keys {
    /// A chunk's address: a **keyed** hash of the plaintext.
    ///
    /// Keyed, not plain BLAKE3, so read access to the repository does not let an
    /// attacker recompute the address of a guessed plaintext and confirm it is
    /// present. Restic and Borg use unkeyed content hashes for dedup and accept
    /// that confirmation-of-file oracle; keying costs nothing.
    pub fn chunk_id(&self, plaintext: &[u8]) -> ChunkId {
        ChunkId(*blake3::keyed_hash(&self.name, plaintext).as_bytes())
    }

    /// Seal `message` under `id`, with the nonce derived from **the message
    /// itself** and stored inline as the first [`NONCE_LEN`] bytes — the same
    /// framing [`Keys::seal_root`] uses.
    ///
    /// # Safety contract: the nonce binds to the message, the AAD binds to the id
    ///
    /// A derived nonce is only safe under **nonce ↔ message** injectivity. This
    /// used to derive the nonce from `id`, which addresses the *pre-image* the
    /// caller framed and compressed rather than the bytes handed to the AEAD —
    /// and zstd guarantees format compatibility across versions, not
    /// byte-identical output. Two builds compressing one plaintext differently
    /// therefore sealed two distinct messages under one
    /// `(chunk_key, nonce, aad)`: Poly1305 one-time-key recovery and keystream
    /// recovery, both reachable from an ordinary heterogeneous fleet. The nonce
    /// is now `derive_key(CTX_NONCE, keyed_hash(name_key, message))[..24]`, so a
    /// message that differs by one bit is sealed under a different nonce whether
    /// or not its id moved.
    ///
    /// **Any future caller must preserve that.** The nonce may be derived only
    /// from the exact bytes passed as `message`; the id may be anything that
    /// addresses them, because it is authenticated as associated data rather
    /// than trusted to be unique per message.
    ///
    /// `pub(crate)` because `chunk::seal_chunk` is the one caller that gets the
    /// framing right. Callers outside this crate go through
    /// [`crate::sync::chunk::seal_chunk`].
    ///
    /// Deterministic within a build: the same message yields the same nonce and
    /// therefore byte-identical output, which is what makes dedup work at all
    /// and what keeps re-syncing unchanged data from creating new git objects
    /// forever.
    pub(crate) fn seal(&self, id: &ChunkId, message: &[u8]) -> Result<Vec<u8>> {
        let nonce = self.chunk_nonce(message);
        let sealed = XChaCha20Poly1305::new((&*self.chunk).into())
            .encrypt(
                &nonce.into(),
                Payload {
                    msg: message,
                    aad: id.as_bytes(),
                },
            )
            .map_err(|_| AppError::Other("chunk encryption failed".into()))?;

        let mut framed = Vec::with_capacity(NONCE_LEN + sealed.len());
        framed.extend_from_slice(&nonce);
        framed.extend_from_slice(&sealed);
        Ok(framed)
    }

    /// A chunk's nonce: derived from the bytes actually encrypted, keyed so
    /// nobody without `name_key` can predict it for a guessed message.
    ///
    /// See [`Keys::seal`] for why this takes the message rather than the id.
    fn chunk_nonce(&self, message: &[u8]) -> [u8; NONCE_LEN] {
        let derived = blake3::derive_key(
            CTX_NONCE,
            blake3::keyed_hash(&self.name, message).as_bytes(),
        );
        let mut nonce = [0u8; NONCE_LEN];
        nonce.copy_from_slice(&derived[..NONCE_LEN]);
        nonce
    }

    /// Open a chunk sealed under `id`. `framed` is `nonce ‖ ciphertext ‖ tag`,
    /// and the length is validated before the split: the bytes arrive from a
    /// remote an attacker may control, so a truncated chunk must be an error,
    /// never a panic.
    ///
    /// This performs **no** `chunk_id(plaintext) == id` identity recheck, and
    /// that is deliberate. The id addresses the caller's *raw plaintext*, while
    /// the bytes returned here are that plaintext's *framed and compressed*
    /// form — different things, so a recheck at this layer would compare apples
    /// to oranges. It would also tie every chunk id to the zstd version: a crate
    /// bump would re-id every chunk in every user's bundle, meaning a full
    /// re-upload and zero dedup.
    ///
    /// The belt-and-braces recheck therefore lives one layer up, in
    /// [`crate::sync::chunk`]'s `open_chunk` after unframing and in
    /// [`crate::sync::pack`]'s `read_header` after deserializing.
    ///
    /// **The refusal names the chunk.** Every tampering attack that reaches this
    /// line — a swapped ciphertext, a flipped bit, a truncated blob, a manifest
    /// chunk served under the id the root names — is one and the same event to
    /// Poly1305: the tag did not verify. Without the id in the message they all
    /// read identically, and an operator cannot tell which object is bad; the
    /// adversarial suite found exactly that collapse across five of its nine
    /// attacks. The id is safe to say: it is written in the clear in every pack
    /// trailer and listed in every index object, and it is a *keyed* hash, so
    /// nobody without `name_key` can invert it or confirm a guess against it. It
    /// is an address, not a secret — CRYPTO-07 is about keys, passwords, and
    /// plaintext, none of which is here.
    pub fn open(&self, id: &ChunkId, framed: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        if framed.len() < NONCE_LEN + TAG_LEN {
            return Err(AppError::Other(format!("chunk {id} is truncated")));
        }
        let (nonce, sealed) = framed.split_at(NONCE_LEN);
        let nonce: [u8; NONCE_LEN] = nonce.try_into().expect("length checked above");
        XChaCha20Poly1305::new((&*self.chunk).into())
            .decrypt(
                &nonce.into(),
                Payload {
                    msg: sealed,
                    aad: id.as_bytes(),
                },
            )
            .map(Zeroizing::new)
            .map_err(|_| AppError::Other(format!("chunk {id} failed authentication")))
    }

    /// Seal the snapshot root under the **root** subkey — the one derived with
    /// `CTX_ROOT`. Reaching for `chunk` here would pass every round-trip test,
    /// leave `root_key` dead, and silently diverge from the hierarchy 1-07 pins.
    ///
    /// The root is the one object whose plaintext changes on every sync, so its
    /// nonce cannot be content-derived without leaking equality between
    /// snapshots. It gets a fresh random 24-byte nonce, stored inline as the
    /// first `NONCE_LEN` bytes of the framed output; XChaCha's 192-bit nonce
    /// makes random generation safe with no counter accounting.
    ///
    /// `repo_id` is bound into the associated data, so a root belonging to
    /// another bundle fails the tag here rather than depending on local state to
    /// notice — see [`Keys::open_root`].
    pub fn seal_root(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let mut nonce = [0u8; NONCE_LEN];
        fill(&mut nonce)?;
        let sealed = XChaCha20Poly1305::new((&*self.root).into())
            .encrypt(
                &nonce.into(),
                Payload {
                    msg: plaintext,
                    aad: ROOT_AAD,
                },
            )
            .map_err(|_| AppError::Other("snapshot root encryption failed".into()))?;

        let mut framed = Vec::with_capacity(NONCE_LEN + sealed.len());
        framed.extend_from_slice(&nonce);
        framed.extend_from_slice(&sealed);
        Ok(framed)
    }

    /// Open a framed snapshot root. Validates the length before slicing: the
    /// bytes arrive from a remote an attacker may control, and a truncated root
    /// must be an error, not a panic.
    pub fn open_root(&self, framed: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        if framed.len() < NONCE_LEN + TAG_LEN {
            return Err(AppError::Other("snapshot root is truncated".into()));
        }
        let (nonce, sealed) = framed.split_at(NONCE_LEN);
        let nonce: [u8; NONCE_LEN] = nonce.try_into().expect("length checked above");
        XChaCha20Poly1305::new((&*self.root).into())
            .decrypt(
                &nonce.into(),
                Payload {
                    msg: sealed,
                    aad: ROOT_AAD,
                },
            )
            .map(Zeroizing::new)
            .map_err(|_| AppError::Other("snapshot root failed authentication".into()))
    }
}

/// Associated data for the snapshot root. A fixed literal, not the root's own
/// address: the root is the mutable entry point and has no content address.
const ROOT_AAD: &[u8] = b"ai-usagebar.sync.v1 root";

/// Unkeyed BLAKE3, for naming an object whose bytes are already public
/// ciphertext — a pack file, say.
///
/// **Never seal anything under an unkeyed address.** An unkeyed hash of a
/// guessable plaintext is a confirmation oracle for anyone holding the
/// repository, which is exactly the property [`Keys::chunk_id`]'s keyed hash
/// exists to deny. This lives here rather than in `pack.rs` so no other module
/// needs to import a hash crate.
pub fn content_address(bytes: &[u8]) -> ChunkId {
    ChunkId(*blake3::hash(bytes).as_bytes())
}

/// Refuse an Argon2 working set that does not fit, before allocating it.
///
/// Pure, and takes `available_kib` by argument: that is the seam, exactly as
/// `Cache::at` exists so no test calls `Cache::for_vendor`. A 1 GB box must get
/// an actionable refusal naming `--kdf-memory`, never an OOM abort.
pub fn check_memory_budget(m_kib: u32, available_kib: u64) -> Result<()> {
    if u64::from(m_kib) <= available_kib {
        return Ok(());
    }
    Err(AppError::Other(format!(
        "key derivation needs {} MiB of memory but only {} MiB is available — \
         re-run with a lower --kdf-memory, which weakens every future restore \
         of this bundle",
        m_kib / 1024,
        available_kib / 1024
    )))
}

/// Best-effort available physical memory. The non-test wrapper around
/// [`check_memory_budget`]'s second argument; no test may call it.
#[cfg(target_os = "linux")]
pub fn available_memory_kib() -> Option<u64> {
    let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
    meminfo.lines().find_map(|line| {
        line.strip_prefix("MemAvailable:")?
            .split_whitespace()
            .next()?
            .parse()
            .ok()
    })
}

/// See the Linux variant. macOS has no `MemAvailable`; `hw.memsize` (total
/// physical) is the closest honest answer.
#[cfg(target_os = "macos")]
pub fn available_memory_kib() -> Option<u64> {
    let out = std::process::Command::new("sysctl")
        .args(["-n", "hw.memsize"])
        .output()
        .ok()?;
    let bytes: u64 = String::from_utf8(out.stdout).ok()?.trim().parse().ok()?;
    Some(bytes / 1024)
}

/// See the Linux variant. Elsewhere the budget check is simply skipped.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn available_memory_kib() -> Option<u64> {
    None
}

/// The OS CSPRNG. No `rand`, no `ThreadRng`, no reseeding.
fn fill(buf: &mut [u8]) -> Result<()> {
    getrandom::fill(buf)
        .map_err(|_| AppError::Other("the operating system random source is unavailable".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Microseconds instead of ~1.5 s and a gibibyte. Never use production
    /// parameters in a unit test: the AUR `check()` runs these on an
    /// installer's machine.
    const CHEAP: KdfParams = KdfParams {
        m_kib: 8,
        t: 1,
        p: 1,
    };

    fn keys() -> Keys {
        Keyfile::create(b"correct horse battery staple", CHEAP)
            .expect("keyfile creation")
            .1
    }

    #[test]
    fn password_round_trips_to_a_sealed_and_reopened_buffer() {
        let (keyfile, keys) = Keyfile::create(b"correct horse battery staple", CHEAP).unwrap();

        let plaintext = b"the quick brown fox".as_slice();
        let id = keys.chunk_id(plaintext);
        let sealed = keys.seal(&id, plaintext).unwrap();
        assert_eq!(&*keys.open(&id, &sealed).unwrap(), plaintext);

        // …and the same password, through the persisted keyfile, gets there too.
        let reopened = keyfile.open(b"correct horse battery staple").unwrap();
        assert_eq!(reopened.chunk_id(plaintext), id);
        assert_eq!(&*reopened.open(&id, &sealed).unwrap(), plaintext);
    }

    #[test]
    fn identical_plaintext_under_the_same_id_seals_to_identical_bytes() {
        // Deterministic sealing is what makes dedup possible; with a random
        // nonce every re-upload of an unchanged chunk would be a new git object.
        let keys = keys();
        let plaintext = b"dedup me".as_slice();
        let id = keys.chunk_id(plaintext);
        assert_eq!(
            keys.seal(&id, plaintext).unwrap(),
            keys.seal(&id, plaintext).unwrap()
        );
    }

    /// The regression guard for the nonce-reuse blocker.
    ///
    /// `seal_chunk` addresses a chunk by its *raw plaintext* and encrypts that
    /// plaintext's compressed frame. zstd guarantees format compatibility across
    /// versions, not byte-identical output, so two builds can hand one id two
    /// different messages. If the nonce were still derived from the id, those
    /// two messages would be sealed under one `(chunk_key, nonce, aad)` — a
    /// solvable Poly1305 key and `C_A ⊕ C_B = F_A ⊕ F_B`.
    ///
    /// Two zstd versions are not installable in a unit test, so the differing
    /// framing is simulated directly: the same id, two different messages, which
    /// is exactly the shape a zstd bump produces.
    #[test]
    fn two_framings_of_one_plaintext_are_sealed_under_different_nonces() {
        let keys = keys();
        let id = keys.chunk_id(b"one plaintext, two zstd builds");

        // Same declared `true_len`, different compressed bytes — the collision a
        // heterogeneous fleet produces.
        let mut a = 23u32.to_le_bytes().to_vec();
        a.extend_from_slice(b"\x09\x00\x00\x00 frame as vN wrote it");
        let mut b = 23u32.to_le_bytes().to_vec();
        b.extend_from_slice(b"\x0a\x00\x00\x00 frame as vN+1 wrote");

        let (sealed_a, sealed_b) = (keys.seal(&id, &a).unwrap(), keys.seal(&id, &b).unwrap());
        assert_ne!(
            sealed_a[..NONCE_LEN],
            sealed_b[..NONCE_LEN],
            "two distinct messages were sealed under one nonce — the AEAD's \
             one-time-key assumption is broken and both are recoverable"
        );

        // …and both still open under the id they were sealed with, so the nonce
        // really did travel inline rather than being re-derived from the id.
        assert_eq!(&**keys.open(&id, &sealed_a).unwrap(), &a[..]);
        assert_eq!(&**keys.open(&id, &sealed_b).unwrap(), &b[..]);
    }

    #[test]
    fn a_chunk_shorter_than_a_nonce_and_a_tag_errors_instead_of_panicking() {
        let keys = keys();
        let id = keys.chunk_id(b"addressable");
        for len in [0, 1, NONCE_LEN, NONCE_LEN + TAG_LEN - 1] {
            let err = keys
                .open(&id, &vec![0u8; len])
                .expect_err("a {len}-byte chunk must be refused, not indexed into")
                .to_string();
            assert!(err.contains("is truncated"), "at {len} bytes: {err}");
        }
    }

    #[test]
    fn the_three_subkeys_are_distinct() {
        let keys = keys();
        assert_ne!(*keys.chunk, *keys.name);
        assert_ne!(*keys.chunk, *keys.root);
        assert_ne!(*keys.name, *keys.root);
    }

    #[test]
    fn a_wrong_password_yields_an_error_and_no_plaintext() {
        let (keyfile, _) = Keyfile::create(b"the real password", CHEAP).unwrap();
        let err = keyfile
            .open(b"not the real password")
            .expect_err("a wrong password must not open the keyfile")
            .to_string();
        assert!(err.contains("wrong password or corrupted keyfile"));
        // The message names the operation and nothing else (D5, CRYPTO-07).
        assert!(!err.contains("the real password"));
        assert!(!err.contains(&keyfile.wrapped_master_key));
    }

    #[test]
    fn a_chunk_id_round_trips_through_hex_and_serde() {
        let id = keys().chunk_id(b"addressable");
        let hex = id.to_string();
        assert_eq!(hex.len(), 64);
        assert_eq!(hex.parse::<ChunkId>().unwrap(), id);
        assert_eq!(
            serde_json::from_str::<ChunkId>(&serde_json::to_string(&id).unwrap()).unwrap(),
            id
        );
        assert!("nothex".parse::<ChunkId>().is_err());
        assert!("zz".repeat(32).parse::<ChunkId>().is_err());
    }

    #[test]
    fn debug_for_keys_redacts_the_key_material() {
        let keys = keys();
        assert_eq!(format!("{keys:?}"), "Keys { <redacted> }");
    }

    #[test]
    fn a_root_sealed_twice_differs_but_both_copies_reopen() {
        let keys = keys();
        let plaintext = b"counter=7".as_slice();
        let first = keys.seal_root(plaintext).unwrap();
        let second = keys.seal_root(plaintext).unwrap();

        // Fresh random nonce per seal: the root's plaintext changes every sync,
        // so deterministic sealing would leak equality between snapshots.
        assert_ne!(first, second);
        assert_eq!(&*keys.open_root(&first).unwrap(), plaintext);
        assert_eq!(&*keys.open_root(&second).unwrap(), plaintext);
    }

    #[test]
    fn the_root_path_uses_the_root_subkey_and_not_the_chunk_subkey() {
        let alice = keys();
        // Same chunk and name subkeys, different root subkey. If `seal_root`
        // reached for `chunk`, this would open — and `root_key` would be dead.
        let mallory = Keys {
            chunk: alice.chunk.clone(),
            name: alice.name.clone(),
            root: Zeroizing::new([0x5a; 32]),
        };
        let sealed = alice.seal_root(b"counter=7").unwrap();
        assert!(mallory.open_root(&sealed).is_err());
        assert!(alice.open_root(&sealed).is_ok());
    }

    #[test]
    fn a_root_shorter_than_a_nonce_and_a_tag_errors_instead_of_panicking() {
        let keys = keys();
        for len in [0, 1, NONCE_LEN, NONCE_LEN + TAG_LEN - 1] {
            assert!(
                keys.open_root(&vec![0u8; len]).is_err(),
                "a {len}-byte root must be refused, not indexed into"
            );
        }
    }

    #[test]
    fn rewrap_under_a_new_password_preserves_the_subkeys() {
        let (keyfile, original) = Keyfile::create(b"first password", CHEAP).unwrap();
        let rewrapped = keyfile
            .rewrap(b"first password", b"second password", CHEAP)
            .unwrap();

        // A fresh salt and nonce, so the bytes differ …
        assert_ne!(keyfile.kdf.salt, rewrapped.kdf.salt);
        assert_ne!(keyfile.wrapped_master_key, rewrapped.wrapped_master_key);

        // … but the same master key is underneath, so no data needs re-encrypting.
        let opened = rewrapped.open(b"second password").unwrap();
        assert_eq!(*opened.chunk, *original.chunk);
        assert_eq!(*opened.name, *original.name);
        assert_eq!(*opened.root, *original.root);

        // The old password no longer opens the *new* keyfile.
        assert!(rewrapped.open(b"first password").is_err());
    }

    #[test]
    fn rewrap_with_the_wrong_old_password_produces_no_keyfile() {
        let (keyfile, _) = Keyfile::create(b"first password", CHEAP).unwrap();
        assert!(
            keyfile
                .rewrap(b"guessed wrong", b"second password", CHEAP)
                .is_err()
        );
    }

    #[test]
    fn a_keyfile_opens_with_its_own_stored_parameters_not_the_compiled_default() {
        let stored = KdfParams {
            m_kib: 16,
            t: 2,
            p: 1,
        };
        // The point of the test: the production default is 1 GiB / t=3, so a
        // keyfile that opens here proves the stored parameters are what got used.
        assert_eq!(KdfParams::default().m_kib, 1_048_576);
        assert_ne!(stored, KdfParams::default());

        let (keyfile, _) = Keyfile::create(b"stored params please", stored).unwrap();
        assert_eq!(keyfile.kdf.params(), stored);
        assert!(keyfile.open(b"stored params please").is_ok());
    }

    #[test]
    fn kdf_parameters_edited_in_transit_fail_to_unwrap() {
        let (keyfile, _) = Keyfile::create(b"in transit", CHEAP).unwrap();

        // An attacker downgrading the work factor on the wire. The parameters
        // are bound as AEAD associated data, so the KEK they now describe is not
        // the KEK the master key was wrapped under.
        let mut tampered = keyfile.clone();
        tampered.kdf.m_kib = 16;
        assert!(tampered.open(b"in transit").is_err());

        let mut retagged = keyfile.clone();
        retagged.kdf.algo = "argon2i".into();
        assert!(retagged.open(b"in transit").is_err());

        // Untouched, it still opens — so the failures above are the binding, not
        // a broken round trip.
        assert!(keyfile.open(b"in transit").is_ok());
    }

    #[test]
    fn a_keyfile_above_the_read_ceiling_is_refused_before_any_cryptographic_work() {
        let (keyfile, _) = Keyfile::create(b"future bundle", CHEAP).unwrap();

        // At the ceiling: accepted.
        assert_eq!(keyfile.format, MAX_SUPPORTED_KEYFILE);
        assert!(keyfile.open(b"future bundle").is_ok());

        // One above, and written by a future client at the production 1 GiB
        // parameters. If the version gate ever moved below `derive_kek`, this
        // test would allocate a gibibyte instead of returning instantly.
        let mut from_the_future = keyfile.clone();
        from_the_future.format = MAX_SUPPORTED_KEYFILE + 1;
        from_the_future.kdf.m_kib = KdfParams::default().m_kib;
        from_the_future.kdf.t = KdfParams::default().t;
        let err = from_the_future
            .open(b"future bundle")
            .expect_err("a newer format must be refused")
            .to_string();
        assert!(err.contains("upgrade ai-usagebar"));
    }

    #[test]
    fn the_memory_budget_refuses_actionably_instead_of_letting_argon2_oom() {
        let err = check_memory_budget(1_048_576, 900_000)
            .expect_err("1 GiB must not be attempted on a 900 MiB budget")
            .to_string();
        assert!(err.contains("--kdf-memory"));
        assert!(err.contains("1024 MiB"));
        assert!(err.contains("878 MiB"));

        assert!(check_memory_budget(1_048_576, 4_000_000).is_ok());
        // Exactly enough is enough.
        assert!(check_memory_budget(1_048_576, 1_048_576).is_ok());
    }

    #[test]
    fn content_address_is_unkeyed_and_therefore_key_independent() {
        // Naming-only: it must never address a *sealed* object, or it becomes a
        // confirmation-of-plaintext oracle for anyone holding the repository.
        let public_bytes = b"already-public ciphertext".as_slice();
        assert_eq!(content_address(public_bytes), content_address(public_bytes));
        // Two unrelated key hierarchies produce the same address, which is the
        // whole difference from `chunk_id`.
        assert_ne!(keys().chunk_id(public_bytes), content_address(public_bytes));
        assert_ne!(keys().chunk_id(public_bytes), keys().chunk_id(public_bytes));
    }

    #[test]
    fn only_the_crypto_module_imports_the_cryptographic_crates() {
        // Resolved from CARGO_MANIFEST_DIR, not a relative path, so the test is
        // independent of the working directory and survives the AUR `srcdir`
        // layout. This invariant is what lets a security auditor read one file
        // instead of six.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/sync");
        let mut checked = 0;
        for entry in std::fs::read_dir(&dir).expect("src/sync must exist") {
            let path = entry.expect("readable directory entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            if path.file_name().and_then(|n| n.to_str()) == Some("crypto.rs") {
                continue;
            }
            checked += 1;
            let source = std::fs::read_to_string(&path).expect("readable module");
            for line in source.lines() {
                let trimmed = line.trim_start();
                // Match on the leading `use` token, so prose in a doc comment
                // naming these crates can never trip the gate.
                let Some(imported) = trimmed.strip_prefix("use ") else {
                    continue;
                };
                for crate_name in ["argon2", "chacha20poly1305"] {
                    assert!(
                        !imported.starts_with(crate_name),
                        "{} imports {crate_name} directly; every cryptographic \
                         call belongs in src/sync/crypto.rs",
                        path.display()
                    );
                }
            }
        }
        assert!(
            checked >= 6,
            "expected mod.rs plus the five sibling modules"
        );
    }
}
