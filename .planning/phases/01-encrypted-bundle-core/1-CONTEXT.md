# Phase 1 Context — Encrypted Bundle Core

**Decisions locked by the orchestrator.** The cryptographic design is already settled by
research — see `.planning/research/SUMMARY.md` (reconciled) and `research/encryption.md` (full
argument, including a key hierarchy that was compiled and run, not just sketched). Do **not**
re-open the primitives or parameters. This file adds the project-specific constraints the
research could not know.

## Locked parameters (from research — restated so a planner needn't re-derive them)

- Argon2id **m = 1 GiB, t = 3, p = 1** → KEK; KEK unwraps a random 32-byte master key held in a
  keyfile. `p = 1` because `argon2` 0.5.3 has no threading: p > 1 measured ~10% *worse* for the
  defender while handing a parallel attacker free speedup.
- BLAKE3 `derive_key` splits the master into `chunk_key` / `name_key` / `root_key`.
- Chunk id = `blake3::keyed_hash(name_key, plaintext)` — **keyed**, so repo read access does not
  permit confirmation-of-file attacks (borg's choice, not restic's).
- XChaCha20-Poly1305, **nonce derived from the chunk id**, id bound as AAD. Deterministic by
  design: identical plaintext must produce identical ciphertext or dedup dies.
- Fixed **256 KiB** chunks + an explicit unsealed tail. No content-defined chunking — appends
  displace no bytes, so CDC buys nothing here (proven in `chunking-storage.md`).
- zstd before encryption.
- Crates: `blake3` 1.8.6, `chacha20poly1305` 0.11.0, `argon2` 0.5.3, `zstd` 0.13.3, `zeroize`
  1.9.0, `getrandom` 0.4.3.

## Locked decisions specific to this codebase

### D1 — Everything in this phase is pure and offline

No network, no `$HOME`, no Keychain, no GitHub. The whole phase must be exercisable by
`cargo test` on a machine with none of those. This is what lets the format be adversary-tested
before anything can transmit it, and it matches how `src/safe_storage.rs` already separates its
pure transform from its one macOS-gated key read.

### D2 — Format versioning is mandatory from the first commit

The snapshot header records `format_version`, the chunker (`"fixed-256k"`), and the full KDF
parameter set. The project has already been bitten by a format that could not evolve — the
routines registry has no `updatedAt`, which forced a file-mtime tie-break — so the on-disk
format here must be able to change its mind later without breaking restore. A reader must
refuse a `format_version` it does not know rather than guessing.

### D3 — Compatibility vectors, following the existing precedent

`src/safe_storage.rs` pins Chromium compatibility with fixed test vectors
(`key_derivation_matches_the_chromium_compatibility_vector`,
`encryption_matches_the_chromium_compatibility_vector`) precisely so a crypto-crate upgrade
cannot silently change the on-disk format. Do the same here: pin at least one known-answer
vector for the KDF, one for the AEAD, and one full round-trip, generated independently.

### D4 — Adversarial tests are part of "done", not a nice-to-have

The phase is not complete without tests that *fail* correctly: wrong password, downgraded KDF
parameters, swapped chunks (both directions), a flipped ciphertext bit, a truncated pack, and a
rolled-back snapshot. The research already ran these against its prototype; they must exist as
project tests.

### D5 — Secret hygiene is testable, not aspirational

Key material is `zeroize`d on drop. No key, password, or plaintext is ever formatted into an
error, a log line, a panic message, or a `Debug` impl — implement `Debug` manually for any type
holding key material rather than deriving it. The project already forbids credentials in
process arguments; the same rule applies to anything in this module.

### D6 — Argon2 at m = 1 GiB is a real UX event

One GiB of memory and ~1.5 s is fine on the user's M3 Max, and much worse on a small Linux box.
Derive once per command and reuse within the process; never re-derive per chunk or per file.
Surface it in the UI as a deliberate wait ("deriving key…"), not a freeze.

## Calibrations owed by this phase

- **CAL-3** — real Argon2id timings on a slow aarch64 Linux target. The 1582 ms figure is M3
  Max. *Fallback if no such machine is available:* keep m = 1 GiB but document the measured
  macOS number and make the parameters configurable, so a user on constrained hardware can
  lower them knowingly rather than being locked out.
- **CAL-1** — whether private-repo Release assets honour `Range:` (decides pack sizing). May be
  answered in Phase 3 once HTTP plumbing exists; if not, assume no `Range:` support.
