---
phase: 01-encrypted-bundle-core
plan: 01
subsystem: crypto
tags: [argon2id, xchacha20poly1305, blake3, zeroize, key-hierarchy, format-versioning]

# Dependency graph
requires: []
provides:
  - "src/sync/ module tree — six files, one per remaining Phase 1 plan, so parallel worktrees merge without conflict"
  - "src/sync/crypto.rs — the complete key hierarchy: password → Argon2id KEK → unwrapped random master key → BLAKE3 subkeys → sealed/opened buffers"
  - "Keyed chunk addressing (blake3::keyed_hash over raw plaintext) and id-derived deterministic nonces"
  - "Snapshot-root framing under the root subkey with a fresh random inline nonce"
  - "Keyfile::rewrap — the CRYPTO-04 password-change primitive"
  - "check_memory_budget — actionable refusal instead of an Argon2 OOM"
  - "content_address — unkeyed naming for already-public ciphertext"
  - "Per-object write-version + read-ceiling constants and sync::check_version"
affects: [1-02, 1-03, 1-04, 1-05, 1-06, 1-07, 1-08, phase-2, phase-3, phase-4]

# Tech tracking
tech-stack:
  added:
    - "argon2 0.5.3 (default-features = false, features alloc + zeroize)"
    - "chacha20poly1305 0.11.0 (default-features = false, feature alloc)"
    - "blake3 1.8.6 (default-features = false, feature std)"
    - "zstd 0.13.3"
    - "zeroize 1.9.0"
    - "getrandom 0.4.2"
  patterns:
    - "Cheap-KDF seam: KdfParams passed by argument everywhere; no derivation function reads a default from inside itself"
    - "Containment invariant enforced by a test that walks src/sync/ from CARGO_MANIFEST_DIR"
    - "Write-version + read-ceiling pairs per versioned object; readers refuse only what exceeds the ceiling"
    - "Canonical AAD via a dedicated #[derive(Serialize)] struct, never a map"

key-files:
  created:
    - src/sync/mod.rs
    - src/sync/crypto.rs
    - src/sync/chunk.rs
    - src/sync/pack.rs
    - src/sync/model.rs
    - src/sync/passphrase.rs
    - src/sync/anchor.rs
  modified:
    - Cargo.toml
    - Cargo.lock
    - src/lib.rs

key-decisions:
  - "Chunk ids address the raw plaintext, never the compressed frame — hashing the frame would tie every id to the zstd version, so a crate bump would re-id every chunk in every user's bundle"
  - "The chunk_id(plaintext) == id recheck lives in chunk::open_chunk and pack::read_header, deliberately NOT in Keys::open"
  - "All sealed objects are addressed by keyed hash; content_address (unkeyed) is documented as naming-only for already-public ciphertext"
  - "Version checks are at-or-below a per-object ceiling, never equality, so a v2 client can still read a v1 bundle"
  - "The keyfile version gate runs before derive_kek, so refusing a too-new bundle costs nothing instead of 1.5 s and a gibibyte"
  - "Keyfile stores a KdfDoc whose field declaration order IS the canonical AAD byte order"
  - "argon2 without the password-hash feature: the format stores KDF parameters but never a verifier, because the AEAD tag is the verifier"

patterns-established:
  - "Secret hygiene is structural: Zeroizing key material, hand-written Debug for Keys, explicit .zeroize() on the AEAD's output Vec, and error messages that name only the operation"
  - "Pure-and-offline crypto layer: no Path, no env, no clock, no network anywhere in src/sync/crypto.rs"
  - "Negative-checked invariant tests: the containment gate was verified to go red before being trusted"

requirements-completed: [CRYPTO-01, CRYPTO-02, CRYPTO-03, CRYPTO-07]

coverage:
  - id: D1
    description: "Password plus keyfile yields the three subkeys; a wrong password yields an error and zero plaintext bytes"
    requirement: CRYPTO-01
    verification:
      - kind: unit
        ref: "cargo test --lib sync::crypto — password_round_trips_to_a_sealed_and_reopened_buffer, a_wrong_password_yields_an_error_and_no_plaintext"
        status: pass
    human_judgment: false
  - id: D2
    description: "The same plaintext sealed twice under the same id produces byte-identical ciphertext (dedup precondition)"
    requirement: CRYPTO-01
    verification:
      - kind: unit
        ref: "cargo test --lib sync::crypto — identical_plaintext_under_the_same_id_seals_to_identical_bytes"
        status: pass
    human_judgment: false
  - id: D3
    description: "A keyfile whose KDF parameters were edited in transit fails to unwrap"
    requirement: CRYPTO-02
    verification:
      - kind: unit
        ref: "cargo test --lib sync::crypto — kdf_parameters_edited_in_transit_fail_to_unwrap"
        status: pass
    human_judgment: false
  - id: D4
    description: "Opening a keyfile uses the parameters recorded inside it, not the compiled-in default"
    requirement: CRYPTO-02
    verification:
      - kind: unit
        ref: "cargo test --lib sync::crypto — a_keyfile_opens_with_its_own_stored_parameters_not_the_compiled_default"
        status: pass
    human_judgment: false
  - id: D5
    description: "Every versioned object has a write version and a read ceiling; readers refuse only what exceeds the ceiling, before any cryptographic work"
    requirement: CRYPTO-02
    verification:
      - kind: unit
        ref: "cargo test --lib sync:: — version_check_accepts_at_or_below_the_ceiling_and_refuses_only_above, a_keyfile_above_the_read_ceiling_is_refused_before_any_cryptographic_work"
        status: pass
    human_judgment: false
  - id: D6
    description: "No key, password, or plaintext can reach an error string, a log line, or a Debug impl"
    requirement: CRYPTO-07
    verification:
      - kind: unit
        ref: "cargo test --lib sync::crypto — debug_for_keys_redacts_the_key_material, a_wrong_password_yields_an_error_and_no_plaintext"
        status: pass
    human_judgment: false
  - id: D7
    description: "The root path uses the root subkey, and a root sealed twice differs while both copies reopen"
    requirement: CRYPTO-01
    verification:
      - kind: unit
        ref: "cargo test --lib sync::crypto — the_root_path_uses_the_root_subkey_and_not_the_chunk_subkey, a_root_sealed_twice_differs_but_both_copies_reopen, a_root_shorter_than_a_nonce_and_a_tag_errors_instead_of_panicking"
        status: pass
    human_judgment: false
  - id: D8
    description: "rewrap changes the password without re-encrypting data, and fails under a wrong old password"
    requirement: CRYPTO-03
    verification:
      - kind: unit
        ref: "cargo test --lib sync::crypto — rewrap_under_a_new_password_preserves_the_subkeys, rewrap_with_the_wrong_old_password_produces_no_keyfile"
        status: pass
    human_judgment: false
  - id: D9
    description: "crypto.rs is the only module in src/sync/ importing argon2 or chacha20poly1305"
    requirement: CRYPTO-07
    verification:
      - kind: unit
        ref: "cargo test --lib sync::crypto — only_the_crypto_module_imports_the_cryptographic_crates (negative-checked: verified to fail when a sibling module imports argon2)"
        status: pass
    human_judgment: false
  - id: D10
    description: "A 1 GB box gets an actionable refusal naming --kdf-memory rather than an OOM abort"
    requirement: CRYPTO-02
    verification:
      - kind: unit
        ref: "cargo test --lib sync::crypto — the_memory_budget_refuses_actionably_instead_of_letting_argon2_oom"
        status: pass
    human_judgment: false

# Metrics
duration: 35min
completed: 2026-08-19
status: complete
---

# Phase 1 Plan 01: Encrypted Bundle Core — Key Hierarchy Summary

**A complete, hermetically testable key hierarchy — password → Argon2id KEK → unwrapped random master key → three BLAKE3 subkeys → deterministically sealed chunks and randomly-nonced snapshot roots — plus the `src/sync/` module boundaries the rest of Phase 1 fills in parallel.**

## Performance

- **Duration:** ~35 min
- **Tasks:** 2/2
- **Files modified:** 10 (7 created, 3 modified)
- **Test suite:** 18 tests, 0.01 s wall clock (cheap-KDF seam holding)

## Accomplishments

- **The whole key hierarchy is implemented and exercised end to end** in `src/sync/crypto.rs`: `derive_kek` → `Keyfile::create`/`open` → `subkeys` → `Keys::seal`/`open` and `seal_root`/`open_root`. The tracer path (password to sealed-and-reopened buffer) passes.
- **The on-disk format is fixed and evolvable.** Every versioned object carries a write version *and* a read ceiling (`KEYFILE_VERSION` / `MAX_SUPPORTED_KEYFILE`, and the same pair for manifest, root, index, and pack header), and `sync::check_version` accepts at-or-below rather than testing equality. The keyfile's gate runs *before* `derive_kek`, so refusing a too-new bundle is instant.
- **The containment invariant is enforced, not asserted.** `only_the_crypto_module_imports_the_cryptographic_crates` walks `src/sync/` resolved from `CARGO_MANIFEST_DIR` and fails if any sibling module imports `argon2` or `chacha20poly1305`. It was negative-checked: appending `use argon2::Argon2;` to `chunk.rs` makes it fail with a pointed message.
- **Module boundaries for the whole phase are set.** `chunk.rs`, `pack.rs`, `model.rs`, `passphrase.rs`, and `anchor.rs` exist as doc-only stubs naming their owner plan, so 1-02 … 1-05 each touch exactly one file and never `mod.rs`.

## Resolved dependency versions

Recorded from `Cargo.lock` for 1-07 to pin compatibility vectors against.

| Crate | Requirement in `Cargo.toml` | Resolved |
|---|---|---|
| `argon2` | `0.5.3`, `default-features = false`, features `alloc` + `zeroize` | **0.5.3** |
| `chacha20poly1305` | `0.11`, `default-features = false`, feature `alloc` | **0.11.0** |
| `blake3` | `1.8`, `default-features = false`, feature `std` | **1.8.6** |
| `zstd` | `0.13` | **0.13.3** (`zstd-safe` 7.2.4, `zstd-sys` 2.0.16+zstd.1.5.7) |
| `zeroize` | `1.9` | **1.9.0** |
| `getrandom` | `0.4` | **0.4.2** |

17 new transitive crates. `cargo tree -i argon2` and `cargo tree -i chacha20poly1305` each show exactly one version, as the plan requires.

**One version note:** research §6 named `getrandom` 0.4.3; the registry's current 0.4 line resolves to **0.4.2**. The plan pinned the requirement as `"0.4"`, which is what was written, so nothing was substituted — 0.4.3 simply is not the head of that line right now. `getrandom::fill` is the API used and is unchanged. Three semver-incompatible `getrandom` majors now coexist in the lock (0.2 via the `pbkdf2`/`aes` tree, 0.3 via `rustls`, 0.4 direct); that is pre-existing and expected across major versions, and the plan's single-version requirement covers only `argon2` and `chacha20poly1305`.

## Task Commits

1. **Task 1 (tracer): Password to sealed buffer and back** — `acf688f` (feat)
2. **Task 2: Root framing, rewrap, memory budget, containment gate** — `be589ea` (feat)

## Public surface of `src/sync/crypto.rs`

Later plans code against these without reading the module.

```rust
// Parameters — passed by argument everywhere; this is the cheap-KDF test seam.
pub struct KdfParams { pub m_kib: u32, pub t: u32, pub p: u32 }
impl Default for KdfParams              // { m_kib: 1_048_576, t: 3, p: 1 }
// derives: Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize

pub fn derive_kek(pw: &[u8], salt: &[u8; 16], k: KdfParams) -> Result<Zeroizing<[u8; 32]>>

// A chunk's address. An address, not a secret.
pub struct ChunkId(/* private [u8; 32] */);
// derives: Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord
// plus: Display (lowercase hex), FromStr (Err = AppError), Serialize/Deserialize as a hex string
impl ChunkId {
    pub fn from_bytes(bytes: [u8; 32]) -> Self;
    pub fn as_bytes(&self) -> &[u8; 32];
}

// The three subkeys. All fields PRIVATE. Debug is hand-written: "Keys { <redacted> }".
pub struct Keys { /* chunk, name, root: Zeroizing<[u8; 32]> */ }
impl Keys {
    pub fn chunk_id(&self, plaintext: &[u8]) -> ChunkId;          // blake3::keyed_hash(name_key, plaintext)
    pub fn seal(&self, id: &ChunkId, plaintext: &[u8]) -> Result<Vec<u8>>;
    pub fn open(&self, id: &ChunkId, ciphertext: &[u8]) -> Result<Zeroizing<Vec<u8>>>;
    pub fn seal_root(&self, plaintext: &[u8]) -> Result<Vec<u8>>;   // random 24B nonce, stored inline first
    pub fn open_root(&self, framed: &[u8]) -> Result<Zeroizing<Vec<u8>>>;
}

// The KDF block of the keyfile. Field DECLARATION ORDER is the canonical AAD byte order.
pub struct KdfDoc { pub algo: String, pub version: u32, pub m_kib: u32, pub t: u32, pub p: u32, pub salt: String }
impl KdfDoc { pub fn params(&self) -> KdfParams; }

// The on-disk keyfile. Holds no plaintext key material, so Debug is derived.
pub struct Keyfile { pub format: u32, pub kdf: KdfDoc, pub nonce: String, pub wrapped_master_key: String }
impl Keyfile {
    pub fn create(pw: &[u8], k: KdfParams) -> Result<(Keyfile, Keys)>;
    pub fn open(&self, pw: &[u8]) -> Result<Keys>;                 // uses self.kdf.params(), never the default
    pub fn rewrap(&self, old_pw: &[u8], new_pw: &[u8], k: KdfParams) -> Result<Keyfile>;
}

pub fn content_address(bytes: &[u8]) -> ChunkId;                   // UNKEYED — naming only, never seal under it
pub fn check_memory_budget(m_kib: u32, available_kib: u64) -> Result<()>;   // pure; the seam
pub fn available_memory_kib() -> Option<u64>;                      // the wrapper; no test may call it
```

And from `src/sync/mod.rs`:

```rust
pub const CHUNK_SIZE: usize = 256 * 1024;
pub const CHUNKER_ID: &str = "fixed-256k";
pub const CTX_CHUNK: &str = "ai-usagebar.sync.v1 chunk-encryption-key";
pub const CTX_NAME:  &str = "ai-usagebar.sync.v1 chunk-name-key";
pub const CTX_ROOT:  &str = "ai-usagebar.sync.v1 snapshot-root-key";
pub const CTX_NONCE: &str = "ai-usagebar.sync.v1 chunk-nonce";

pub const KEYFILE_VERSION: u32 = 1;      pub const MAX_SUPPORTED_KEYFILE: u32 = 1;
pub const MANIFEST_VERSION: u32 = 1;     pub const MAX_SUPPORTED_MANIFEST: u32 = 1;
pub const ROOT_VERSION: u32 = 1;         pub const MAX_SUPPORTED_ROOT: u32 = 1;
pub const INDEX_VERSION: u32 = 1;        pub const MAX_SUPPORTED_INDEX: u32 = 1;
pub const PACK_HEADER_VERSION: u32 = 1;  pub const MAX_SUPPORTED_PACK_HEADER: u32 = 1;

pub fn check_version(found: u32, ceiling: u32, object: &str) -> Result<()>;
```

The snapshot root's AAD is the private literal `b"ai-usagebar.sync.v1 root"` inside `crypto.rs`; callers never supply it.

## Files Created/Modified

- `Cargo.toml` — six new dependencies under a commented `--- encrypted sync ---` block explaining why each exists and why all six keep the AUR source build hermetic
- `Cargo.lock` — 17 new transitive crates
- `src/lib.rs` — `pub mod sync;` in alphabetical position (between `supergrok` and `theme`)
- `src/sync/mod.rs` — six module declarations, format constants, the four BLAKE3 context strings, five write-version/read-ceiling pairs, and `check_version`
- `src/sync/crypto.rs` — the entire key hierarchy and every AEAD call in Phase 1 (≈840 lines including tests)
- `src/sync/{chunk,pack,model,passphrase,anchor}.rs` — doc-only stubs naming the owning plan

## Decisions Made

- **`ChunkId::from_str` guards on `is_ascii()` as well as length.** Slicing a 64-*byte* string at 2-byte offsets panics if any char boundary is mid-multibyte; `is_ascii` makes the indexing provably safe. The bytes arrive from a remote filename an attacker controls, so a panic here would be a denial of service.
- **`Keyfile::create` and `rewrap` share a private `wrap`, and `open`/`rewrap` share a private `unwrap_master_key`.** `open` returns `Keys` (subkeys), but `rewrap` needs the master key itself; splitting the unwrap out avoids either duplicating the AEAD call or widening the public surface to expose the master key.
- **`KdfDoc` declares its fields explicitly rather than `#[serde(flatten)]`-ing `KdfParams`.** Flatten routes through map serialization, and `serde_json`'s default map is a `BTreeMap` — an AAD whose byte order depends on map iteration is an AAD that can intermittently fail to authenticate. A plain struct serializes in declaration order.
- **`unwrap_master_key` checks the AEAD output length before copying, and zeroizes on that failure path too.** A 32-byte copy from a shorter `Vec` would panic; a longer one would silently truncate.
- **The version-ceiling test deliberately sets the tampered keyfile to production KDF parameters.** If the gate ever moves below `derive_kek`, that test allocates a gibibyte instead of returning instantly — which is the point: the ordering is the assertion.

## Deviations from Plan

None — plan executed as written. Two clarifications rather than deviations:

1. **`getrandom` resolved to 0.4.2, not the 0.4.3 named in research §6.** The plan pinned `"0.4"` and that is what was written; nothing was substituted. Recorded above so 1-07's vectors reference the real version.
2. **`zstd` 0.13.3 is added but not yet imported** — `chunk.rs` (plan 1-02) is its first consumer. It is in this commit because the plan specifies all six dependencies land together. `cargo machete` (release-gate only, not this plan's verification) would flag it until 1-02 lands.

## Issues Encountered

One compile error, fixed immediately: `subkeys(&self.unwrap_master_key(pw)?)` — the `?` operator would not deref-coerce `Zeroizing<[u8; 32]>` to `&[u8; 32]` at that position; `&*` fixes it. No design impact.

## Verification

All commands run in the worktree at `.claude/worktrees/1-01`.

| Check | Result |
|---|---|
| `cargo test --lib sync::` | 18 passed, 0 failed, 0.01 s |
| `env -u HOME cargo test --lib sync::crypto` | 17 passed, 0 failed — hermetic with `$HOME` unset |
| `cargo tree -i argon2` | single version, 0.5.3 |
| `cargo tree -i chacha20poly1305` | single version, 0.11.0 |
| `cargo clippy --all-targets -- -D warnings` | clean |
| `cargo fmt -- --check` | clean |
| Containment gate negative check | appending `use argon2::Argon2;` to `chunk.rs` makes the test fail as intended |

No test in this plan reads a path other than through `CARGO_MANIFEST_DIR`, an environment variable, or the clock. Every test uses the cheap KDF parameters `{ m_kib: 8, t: 1, p: 1 }` except `a_keyfile_opens_with_its_own_stored_parameters_not_the_compiled_default` (which uses `{16, 2, 1}`, also cheap, and only *reads* the production default to prove they differ) and `a_keyfile_above_the_read_ceiling_is_refused_before_any_cryptographic_work` (which writes the production default into a keyfile that is refused before the KDF ever runs).

## Known Stubs

`src/sync/{chunk,pack,model,passphrase,anchor}.rs` contain only module documentation. This is **intentional and specified by the plan**: fixing the module boundaries here is what lets plans 1-02 … 1-05 run in parallel worktrees, each owning exactly one file and never touching `mod.rs`. Resolution:

| File | Resolved by |
|---|---|
| `src/sync/chunk.rs` | 1-02 |
| `src/sync/pack.rs` | 1-03 |
| `src/sync/model.rs` | 1-04 |
| `src/sync/passphrase.rs` | 1-05 |
| `src/sync/anchor.rs` | 1-05 |

Nothing in the phase's goal is blocked by these — no code path reaches them yet, and the crate compiles, tests, and lints clean.

## Threat Flags

None. No new network endpoint, auth path, file access pattern, or schema at a trust boundary was introduced beyond those already enumerated in the plan's `<threat_model>`. `available_memory_kib` reads `/proc/meminfo` (Linux) and shells out to `sysctl -n hw.memsize` (macOS); both are read-only, take no user input, and are unreachable from any test.

## User Setup Required

None — no external service configuration required. The whole plan is pure and offline.

## Next Phase Readiness

**Ready.** Wave 2 (1-02, `chunk.rs`) can start immediately: it needs `Keys::chunk_id`, `Keys::seal`, `Keys::open`, and `CHUNK_SIZE`, all of which exist and are tested. Wave 3 (1-03 `pack.rs`, 1-04 `model.rs`) additionally needs `content_address`, `seal_root`/`open_root`, and `check_version` with the pack-header and root ceilings — also all present.

The one thing wave 2 must not do: re-hash the compressed frame to produce a chunk id. Ids address the **raw plaintext**; the `chunk_id(plaintext) == id` recheck belongs in `chunk::open_chunk` *after* unframing, which is why `Keys::open` deliberately does not perform it.

## Self-Check: PASSED

All seven created files exist on disk; both task commits (`acf688f`, `be589ea`) exist in git; `src/sync/crypto.rs` is 836 lines.
