---
phase: 01-encrypted-bundle-core
plan: 01
type: execute
wave: 1
depends_on: []
files_modified:
  - Cargo.toml
  - src/lib.rs
  - src/sync/mod.rs
  - src/sync/crypto.rs
  - src/sync/chunk.rs
  - src/sync/pack.rs
  - src/sync/model.rs
  - src/sync/passphrase.rs
  - src/sync/anchor.rs
autonomous: true
requirements: [CRYPTO-01, CRYPTO-02, CRYPTO-03, CRYPTO-07]
must_haves:
  truths:
    - "A password plus a keyfile yields the three subkeys; a wrong password yields an error and zero plaintext bytes."
    - "The same plaintext sealed twice under the same id produces byte-identical ciphertext."
    - "A keyfile whose KDF parameters were edited in transit fails to unwrap."
    - "Opening a keyfile uses the parameters recorded inside it, not the compiled-in default."
    - "No key, password, or plaintext can reach an error string, a log line, or a Debug impl."
  artifacts:
    - Cargo.toml carrying blake3, chacha20poly1305, argon2, zstd, zeroize, getrandom at resolvable versions
    - src/sync/mod.rs with the shared format constants, the per-object version ceilings, and the six module declarations
    - src/sync/crypto.rs implementing the whole key hierarchy and every AEAD call in the phase
  key_links:
    - "src/lib.rs declares `pub mod sync;` — without it nothing in the phase compiles"
    - "src/sync/mod.rs declares all six submodules, so each later plan owns exactly one file and never touches mod.rs"
    - "crypto.rs is the sole importer of argon2/chacha20poly1305/blake3 — the containment invariant every later audit relies on"
    - "seal_root/open_root use root_key, not chunk — a fork here is invisible until 1-07 pins it"
---

<objective>
Land the crate skeleton and the complete key hierarchy: password → Argon2id KEK → unwrapped random
master key → BLAKE3 `derive_key` subkeys → sealed/opened buffers. This is the tracer for the whole
phase — one thin path from a password to a sealed-and-reopened buffer, wired end to end, production
quality.

It also fixes the module boundaries for the rest of Phase 1. Every later plan fills files this plan
creates, so parallel worktree branches merge without a conflict.

Implements **D-01 (D1)** pure-and-offline, **D-02 (D2)** mandatory format versioning,
**D-05 (D5)** testable secret hygiene, **D-06 (D6)** derive once per process.

Purpose: nothing else in the milestone can exist until the key hierarchy does.
Output: `Cargo.toml` deps, `src/sync/` module tree, a complete `src/sync/crypto.rs`.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/01-encrypted-bundle-core/1-CONTEXT.md
@.planning/research/encryption.md
@CLAUDE.md
@src/safe_storage.rs
@src/error.rs
@src/lib.rs
@Cargo.toml
</context>

<source_audit>
Phase-wide coverage audit. Every source item maps to a plan; no item is deferred or reduced.

| Source | Item | Covered by |
|---|---|---|
| GOAL | Bundle format exists and survives an attacker controlling the remote | 1-01 … 1-08 |
| REQ | CRYPTO-01 client-side encryption, remote never sees plaintext or password | 1-01, 1-02, 1-03 |
| REQ | CRYPTO-02 memory-hard KDF, params stored alongside the data, raisable later | 1-01, 1-04 |
| REQ | CRYPTO-03 wrong password fails cleanly and unambiguously | 1-01, 1-06 |
| REQ | CRYPTO-05 tamper / reorder / truncate / rollback detected | 1-03, 1-04, 1-05, 1-06 |
| REQ | CRYPTO-06 password strength enforced, offline attack explained | 1-05 |
| REQ | CRYPTO-07 zeroized, never in argv, env, logs, or errors | 1-01, 1-05, 1-06 |
| RESEARCH | Argon2id m=1 GiB t=3 p=1; BLAKE3 subkeys; keyed chunk ids over plaintext; XChaCha derived nonce | 1-01 |
| RESEARCH | Fixed 256 KiB chunks, explicit tail, zstd before encrypt, tail padding | 1-02 |
| RESEARCH | restic-shaped pack: blobs, sealed header, u32 LE header length, sharded name | 1-03 |
| RESEARCH | Snapshot root with random nonce, manifest as a sealed chunk, index `supersedes` | 1-04 |
| RESEARCH | Generate-by-default passphrase, 12-char floor, local monotonic rollback anchor | 1-05 |
| RESEARCH | The adversarial assertions run against the §8 prototype | 1-06 |
| CONTEXT | D1 pure and offline | every plan |
| CONTEXT | D2 format versioning from the first commit, evolvable | 1-01, 1-04, 1-08 |
| CONTEXT | D3 pinned compatibility vectors, `safe_storage.rs` precedent | 1-07 |
| CONTEXT | D4 adversarial tests are part of done | 1-06 |
| CONTEXT | D5 secret hygiene is testable | 1-01, 1-05, 1-06 |
| CONTEXT | D6 Argon2 at 1 GiB is a real UX event | 1-01, 1-08 |
| CONTEXT | CAL-3 Argon2id timing on slow aarch64 Linux | 1-08 |
| CONTEXT | CAL-1 `Range:` on private-repo release assets | 1-08 |

No MISSING items. Nothing deferred.
</source_audit>

<tasks>

<task type="tracer">
  <name>Task 1: Password to sealed buffer and back — one path, every layer</name>
  <precondition>`cargo --version` reports a toolchain at or above 1.88 with edition 2024 support.</precondition>
  <precondition>All six new dependencies resolve. Before writing any code run `cargo add --dry-run` for each of `blake3`, `chacha20poly1305`, `argon2`, `zstd`, `zeroize`, `getrandom` at the pinned versions with their feature flags. This plan forbids substituting a different crate, so an unresolvable version is a halt, not a judgement call — report it and stop.</precondition>
  <files>Cargo.toml, src/lib.rs, src/sync/mod.rs, src/sync/crypto.rs, src/sync/chunk.rs, src/sync/pack.rs, src/sync/model.rs, src/sync/passphrase.rs, src/sync/anchor.rs</files>
  <reversibility rating="costly">The context strings, the keyfile JSON shape, and the derived-nonce
  construction become the on-disk format. `format_version` plus the per-object ceilings are what make
  changing them survivable, which is why D2 requires them from this commit. No user holds a keyfile
  yet, so the cost is bounded to rewriting this module.</reversibility>
  <action>
Add exactly six dependencies to `Cargo.toml`, with the feature flags from `research/encryption.md`
§6: `argon2` at 0.5.3 with `default-features = false` and features `alloc` and `zeroize`;
`chacha20poly1305` at 0.11 with `default-features = false` and feature `alloc`; `blake3` at 1.8 with
`default-features = false` and feature `std`; `zstd` at 0.13; `zeroize` at 1.9; `getrandom` at 0.4.
Add no others, and substitute nothing. `argon2` must not gain the `password-hash` feature — this
format stores KDF parameters but never a verifier, because the AEAD tag is the verifier. Group them
under a comment block explaining that they exist for encrypted sync and that all six are pure-Rust or
vendored so the AUR source build stays hermetic. Record the exact resolved versions from
`Cargo.lock` in the summary; 1-07 pins vectors against them.

Add `pub mod sync;` to `src/lib.rs` in alphabetical position.

Create `src/sync/mod.rs` declaring, in alphabetical order, `pub mod anchor; pub mod chunk;
pub mod crypto; pub mod model; pub mod pack; pub mod passphrase;` and holding the constants the whole
phase reads: `pub const CHUNK_SIZE: usize = 256 * 1024;`, `pub const CHUNKER_ID: &str = "fixed-256k";`,
and the four BLAKE3 context strings as `pub const` items — `CTX_CHUNK`, `CTX_NAME`, `CTX_ROOT`,
`CTX_NONCE` — using the exact literals from `research/encryption.md` §3
(`"ai-usagebar.sync.v1 chunk-encryption-key"` and its three siblings). The version token inside each
literal is load-bearing per D2: a v2 hierarchy must not be able to collide with v1.

Versioning constants, and read this part carefully because it is the mechanism CRYPTO-02 rests on.
Give each versioned object *two* numbers, not one: the version this build **writes**, and the highest
version it can **read**. Declare `pub const KEYFILE_VERSION: u32 = 1;` and
`pub const MAX_SUPPORTED_KEYFILE: u32 = 1;`, and the same pair for the manifest, the root, the index
object, and the pack header — 1-03 and 1-04 use theirs. Every reader accepts any version *at or
below* its ceiling and refuses only what is greater, with a message telling the user their client is
too old for this bundle. An equality check would mean a v2 client could not read a v1 bundle, which
is precisely the "raise the parameters without breaking existing bundles" promise inverted. This
project already shipped a format that could not evolve — the routines registry, which had no
`updatedAt` and forced a file-mtime tie-break. Do not do it twice.

Create the five sibling module files — `chunk.rs`, `pack.rs`, `model.rs`, `passphrase.rs`,
`anchor.rs` — each containing only a `//!` module doc comment naming what will live there and which
plan owns it. They are deliberately empty here: `chunk.rs` lands in wave 2, and `pack.rs` and
`model.rs` in wave 3 *after* it, precisely because both call into `chunk.rs` and would not compile
against a stub.

Implement `src/sync/crypto.rs` with the following public surface, transcribing the compiled-and-run
sketch in `research/encryption.md` §3 rather than reinventing it:

`pub struct KdfParams { pub m_kib: u32, pub t: u32, pub p: u32 }` with `Default` returning
`{ m_kib: 1_048_576, t: 3, p: 1 }`, deriving `Clone, Copy, Serialize, Deserialize, PartialEq, Debug`.
Taking `KdfParams` as an argument everywhere is the cheap-KDF test seam — tests pass
`{ m_kib: 8, t: 1, p: 1 }`, which runs in microseconds and keeps the AUR `check()` inside its time
budget. No derivation function may read a default from inside itself.

`pub fn derive_kek(pw: &[u8], salt: &[u8; 16], k: KdfParams) -> Result<Zeroizing<[u8; 32]>>` —
Argon2id, `Version::V0x13`, 32-byte output, no secret and no associated data.

`pub struct ChunkId([u8; 32])` with `from_bytes`, `as_bytes`, lowercase-hex `Display`, hex `FromStr`,
and serde as a hex string. A chunk id is an address, not a secret, so deriving `Debug` on it is fine.

`pub struct Keys` holding `chunk`, `name`, and `root` as `Zeroizing<[u8; 32]>`, all private.
Hand-write `impl std::fmt::Debug for Keys` printing `Keys { <redacted> }` — do not derive it (D5).
`fn subkeys(mk: &[u8; 32]) -> Keys` applies `blake3::derive_key` under `CTX_CHUNK`, `CTX_NAME`,
`CTX_ROOT` respectively — one context string per field, and getting that mapping wrong is invisible
to every round-trip test, which is why 1-07 pins all three.

`pub struct Keyfile` serializing to the JSON in `research/encryption.md` §1: `format` (u32),
`kdf` (`{ algo, version, m_kib, t, p, salt }` with the 16-byte salt base64), `nonce` (24 bytes,
base64) and `wrapped_master_key` (48 bytes, base64). Use the crate's existing `base64` 0.23 engine.
Methods: `pub fn create(pw: &[u8], k: KdfParams) -> Result<(Keyfile, Keys)>` — draw a 16-byte salt,
a 24-byte nonce, and a 32-byte master key with `getrandom::fill`, derive the KEK, wrap the master
key, return both the keyfile and the live `Keys`; and `pub fn open(&self, pw: &[u8]) -> Result<Keys>`,
which derives the KEK using **the parameters stored in this keyfile**, never `KdfParams::default()`.
A private `fn aad(&self) -> Vec<u8>` returns the canonical serialization of `format` plus `kdf`
(serialize a dedicated struct with fields in a fixed declared order — never a map, whose order is not
guaranteed). Binding that as AEAD associated data is what makes a parameter downgrade fail to unwrap
rather than succeed weakly. `open` refuses a `format` above `MAX_SUPPORTED_KEYFILE` before doing any
cryptographic work.

`open` copies the master key out of the `Vec<u8>` the AEAD returns and then calls `.zeroize()` on
that `Vec` explicitly — the allocating AEAD API does not zeroize its own output, and that Vec holds
the master key (§7.1).

`impl Keys` with: `pub fn chunk_id(&self, plaintext: &[u8]) -> ChunkId` via
`blake3::keyed_hash(&self.name, plaintext)`; a private `fn nonce_for(id: &ChunkId) -> [u8; 24]`
taking the first 24 bytes of `blake3::derive_key(CTX_NONCE, id.as_bytes())`;
`pub fn seal(&self, id: &ChunkId, plaintext: &[u8]) -> Result<Vec<u8>>` encrypting under the `chunk`
subkey with that derived nonce and `aad = id.as_bytes()`; and
`pub fn open(&self, id: &ChunkId, ciphertext: &[u8]) -> Result<Zeroizing<Vec<u8>>>` which verifies
the tag and returns the recovered bytes.

`Keys::open` performs **no** identity recheck, and the reason belongs in a comment. The id is a keyed
hash of the caller's *plaintext*, while the bytes this function returns are the caller's *framed and
compressed* form — different things, so a recheck here would compare apples to oranges. The
`chunk_id(plaintext) == id` belt-and-braces check therefore lives one layer up, in `chunk::open_chunk`
after unframing and in `pack::read_header` after deserializing. Point at both.

Every failure returns `AppError::Other` with a message that names only the operation — "wrong
password or corrupted keyfile", "chunk failed authentication" — and never interpolates a key, a
password, a nonce, a plaintext, or a byte count of any of them (D5, CRYPTO-07). Do not add a new
`AppError` variant; the existing `Other` is enough.

Follow the `aead 0.6` / `hybrid-array 0.4` idiom noted in §3: `Array::from_slice` is deprecated,
so pass `key.into()` and `nonce.into()` with an explicit reference. This is the exact spot the
project's `-D warnings` gate bites.

End the task with an inline `#[cfg(test)] mod tests` carrying the tracer assertion: build a keyfile
with the cheap parameters, seal a short buffer under its own `chunk_id`, reopen it, and assert the
bytes match; then assert that sealing the same buffer under the same id twice yields identical
ciphertext, because deterministic sealing is what makes dedup possible at all.
  </action>
  <verify>
    <automated>cargo test --lib sync::crypto</automated>
  </verify>
  <done>`cargo test --lib sync::crypto` passes. A password round-trips to a sealed-and-reopened buffer, and identical plaintext under the same id yields identical ciphertext. The six crates resolve with no duplicate-version conflict, and their resolved versions are recorded.</done>
</task>

<task type="auto" tdd="true">
  <name>Task 2: Root framing, rewrap, memory budget, and the containment gate</name>
  <files>src/sync/crypto.rs</files>
  <behavior>
    - A root sealed twice over identical plaintext yields different bytes (fresh random nonce) yet both reopen to the same plaintext.
    - A root sealed by one `Keys` does not open under a `Keys` whose root subkey differs — proving the root path uses the root subkey and not the chunk subkey.
    - `open_root` on a buffer shorter than 24 + 16 bytes errors instead of indexing out of bounds.
    - `rewrap` under a new password produces a keyfile that opens to the same three subkeys as the original.
    - `rewrap` with the wrong old password errors and produces no keyfile.
    - A keyfile written at `{m_kib: 16, t: 2, p: 1}` opens correctly while `KdfParams::default()` is the production 1 GiB value — the stored parameters are what get used.
    - A keyfile whose serialized `format` is one above `MAX_SUPPORTED_KEYFILE` is refused; one at or below it is accepted.
    - `check_memory_budget(1_048_576, 900_000)` errors and names `--kdf-memory`; `check_memory_budget(1_048_576, 4_000_000)` is `Ok`.
    - No file under `src/sync/` other than `crypto.rs` has a `use` line importing argon2 or chacha20poly1305.
  </behavior>
  <action>
Extend `src/sync/crypto.rs`.

`pub fn seal_root(&self, plaintext: &[u8]) -> Result<Vec<u8>>` and
`pub fn open_root(&self, framed: &[u8]) -> Result<Zeroizing<Vec<u8>>>`, both operating under the
**`root` subkey** — the one derived with `CTX_ROOT`, not the chunk subkey. Naming it explicitly here
matters: reaching for `chunk` would work, leave `root_key` dead, and produce a hierarchy that passes
every round-trip test while diverging from the design 1-07 pins.

The snapshot root is the one object whose plaintext changes on every sync, so its nonce cannot be
content-derived without leaking equality between snapshots. Draw a fresh 24-byte nonce with
`getrandom::fill`, store it inline as the first 24 bytes of the framed output, and bind the literal
context string `b"ai-usagebar.sync.v1 root"` as associated data. `open_root` validates the length
before slicing.

`pub fn rewrap(&self, old_pw: &[u8], new_pw: &[u8], k: KdfParams) -> Result<Keyfile>` on `Keyfile`:
unwrap the master key with the old password, draw a fresh salt and nonce, wrap the same master key
under the new password, return the new keyfile. This is the CRYPTO-04 primitive; Phase 4 owns the
observable requirement, which also needs the old keyfile asset deleted from the remote. Phase 1
delivers the primitive only.

`pub fn check_memory_budget(m_kib: u32, available_kib: u64) -> Result<()>` — a pure function that
refuses when the requested Argon2 working set does not fit, with an actionable message naming
`--kdf-memory` and stating the requested and available sizes in MiB. Add a thin non-test wrapper
`pub fn available_memory_kib() -> Option<u64>` reading `/proc/meminfo` `MemAvailable` on Linux and
`sysctl hw.memsize` on macOS, returning `None` elsewhere. Taking `available_kib` as an argument is
the seam: no test may call the wrapper, exactly as `Cache::at` exists so no test calls
`Cache::for_vendor`. A 1 GB box must get an actionable refusal, never an OOM abort (D6, CAL-3
fallback).

`pub fn content_address(bytes: &[u8]) -> ChunkId` — unkeyed `blake3::hash`, for naming an object
whose bytes are already public ciphertext, such as a pack file. Document the one rule that goes with
it: **never seal anything under an unkeyed address.** An unkeyed hash of a guessable plaintext is a
confirmation oracle for anyone holding the repository, which is exactly the property the keyed chunk
id exists to deny. Exposing it here rather than in `pack.rs` means no consumer needs to import a hash
crate, which keeps the containment invariant below crisp.

Add a `#[test]` named `only_the_crypto_module_imports_the_cryptographic_crates` that walks
`src/sync/` — resolved from `env!("CARGO_MANIFEST_DIR")`, not a relative path, so the test is
independent of the working directory and survives the AUR `srcdir` layout — skips `crypto.rs`, and
asserts no remaining file contains a line whose trimmed start is an import of `argon2` or
`chacha20poly1305`. Match on the leading `use` token so prose in a doc comment can never trip the
gate. This invariant is what lets a security auditor read one file instead of six.

Add tests for each line of `<behavior>`, all using the cheap KDF parameters except the one that
deliberately asserts a stored-parameter open against the production default.
  </action>
  <verify>
    <automated>cargo test --lib sync::crypto</automated>
  </verify>
  <done>All `<behavior>` assertions pass. `crypto.rs` is the only module in `src/sync/` importing argon2 or chacha20poly1305, enforced by a test that resolves its path from `CARGO_MANIFEST_DIR`.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|---|---|
| password → process | User-supplied secret enters memory and must never leave it except as a derived key |
| keyfile JSON → process | Attacker-controlled bytes: parameters, salt, nonce, and wrapped key all arrive untrusted |
| process memory → any output | Errors, logs, and `Debug` output are an exfiltration path for key material |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|---|---|---|---|---|---|
| T-01-01 | Tampering | `Keyfile::open` | critical | mitigate | Canonical `{format, kdf}` serialization bound as AEAD associated data, so a downgraded `m_kib` yields a KEK that cannot unwrap |
| T-01-02 | Information disclosure | error and `Debug` paths | critical | mitigate | Hand-written `Debug for Keys`; error strings name the operation only; the AEAD's output `Vec` is explicitly `.zeroize()`d |
| T-01-03 | Information disclosure | sealing under an unkeyed address | high | mitigate | `content_address` is documented as naming-only; every sealed object is addressed by keyed `chunk_id`, denying the confirmation-of-plaintext oracle |
| T-01-04 | Denial of service | `derive_kek` at m = 1 GiB | medium | mitigate | `check_memory_budget` refuses actionably before allocating, naming `--kdf-memory` |
| T-01-05 | Repudiation | a future client unable to read today's bundles | high | mitigate | Separate write-version and read-ceiling constants per object; readers refuse only versions above their ceiling |
| T-01-06 | Information disclosure | swap of the 1 GiB Argon2 working set | low | accept | `mlock` is impossible under a default `RLIMIT_MEMLOCK` and argon2 0.5 exposes no hook; documented as deliberate in 1-08 |
| T-01-SC | Tampering | six new cargo dependencies | high | accept | Legitimacy established in `research/encryption.md` §6: per-crate version, maintainer, and maintenance status; RustSec advisory-DB scanned 2026-08-19 with zero hits across the recommended set; all six compiled and executed in §8 against this toolchain. Resolution re-verified by the `cargo add --dry-run` precondition |
</threat_model>

<verification>
- `cargo test --lib sync::crypto` passes with `$HOME` unset.
- `cargo tree -i argon2` and `-i chacha20poly1305` show a single version each.
- No test in this plan reads a path other than through `CARGO_MANIFEST_DIR`, an environment variable, or the clock.
</verification>

<success_criteria>
1. A keyfile created under a password reopens to the same three subkeys under that password and errors under any other.
2. Editing `m_kib` in a serialized keyfile makes it fail to open, and a keyfile with non-default parameters opens using its own.
3. Sealing identical bytes under the same id twice produces identical ciphertext; sealing a root twice does not.
4. The root path uses the root subkey, proven by a test that varies only that subkey.
5. `crypto.rs` is the only module in `src/sync/` importing the cryptographic crates, proven by a test.
6. Every versioned object has a write version and a read ceiling, and readers refuse only what exceeds the ceiling.
</success_criteria>

<output>
Create `.planning/phases/01-encrypted-bundle-core/1-01-SUMMARY.md` when done. Record the exact public
signatures of `crypto.rs` and the resolved dependency versions — later plans code against them
without reading the module.
</output>
