---
phase: 01-encrypted-bundle-core
plan: 07
subsystem: sync
tags: [known-answer-vectors, regression-pins, argon2id, blake3, xchacha20poly1305, key-hierarchy, dedup, zstd]

# Dependency graph
requires: [1-01, 1-02, 1-03, 1-04, 1-06, 1-09]
provides:
  - "tests/sync_vectors.rs — four published primitive vectors plus nine composed-format regression pins"
  - "A crate upgrade that changes Argon2id, BLAKE3 or XChaCha20-Poly1305 semantics now fails a test instead of changing the on-disk format"
  - "Each of chunk_key / name_key / root_key pinned individually AND proved to be the key its own path uses"
  - "The id-pin vs ciphertext-pin distinction, stated in every failure message: a moved id means dedup broke for every user; a moved ciphertext means zstd changed"
  - "A keyfile built by hand from docs/sync-format.md §1–2 that Keyfile::open accepts — the only assertion that the documented wrap format and the implemented one agree"
affects: [phase-2, phase-3, phase-4, phase-5]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Primitive guards call the crate directly (argon2 / chacha20poly1305 / blake3), so they test the crate rather than our wrapper; the containment invariant walks src/sync/ and is unaffected"
    - "Every pinned literal carries a provenance comment naming its source — a pin whose provenance is unrecorded is indistinguishable from a snapshot of a bug"
    - "A subkey pin is a value pin plus a wiring proof: the value alone cannot catch a context string attached to the wrong field, because the hierarchy stays self-consistent under any permutation"
    - "Assertion messages classify the failure (five-alarm vs compatibility decision) so a red pin never invites regenerating until green"

key-files:
  created:
    - tests/sync_vectors.rs
  modified: []

key-decisions:
  - "The three published-value sources were fetched at authoring time and cited inline: RFC 9106 §5.3 (Argon2id tag), the BLAKE3 team's own test_vectors.json input_len=1024 case (keyed_hash and derive_key), and draft-arciszewski-xchacha-03 §A.3.1 (AEAD). No vector was invented and none was substituted"
  - "argon2 0.5.3 with default-features off DOES expose the RFC's secret and associated-data inputs (Argon2::new_with_secret, ParamsBuilder::data), so the RFC vector is encoded exactly as published rather than weakened to a self-comparison"
  - "The BLAKE3 case chosen is input_len = 1024 — BLAKE3's own chunk size, so it crosses the tree-hashing boundary a single-block vector never reaches"
  - "Pinning the subkeys required a Keys with a KNOWN master key, which Keyfile::create cannot give (it draws a random one) and no src/ seam exposes. The keyfile is therefore hand-wrapped in the test from docs/sync-format.md, which turned the obstacle into extra coverage: Keyfile::open accepting it proves the document and the implementation have not drifted apart"
  - "Each subkey pin asserts its wiring as well as its value: name_key via chunk_id, chunk_key via a seal reconstructed from the documented nonce/AAD rule, root_key via open_root accepting a root framed by hand. A value-only pin cannot catch CTX_* attached to the wrong field"
  - "The chunk-id pin and the manifest-id pin are labelled STABLE across zstd upgrades and told not to be regenerated; the chunk-ciphertext pin and the pack-address pin are labelled zstd-sensitive and told to check the id pins first. The distinction lives in the assertion messages, not only in the module doc"
  - "No src/ file was touched. Every pinned value matched a directly computed expectation on first run — no mismatch, no blocker, nothing handed to the verifier"

patterns-established:
  - "Composed-format pins are labelled as regression pins of current behaviour, never as independent vectors: nobody publishes a vector for this format, so their value is that they change loudly, not that they are externally correct"
  - "A failing pin's message names the consequence, not just the mismatch — the difference between 'dedup broke for every existing user' and 'zstd changed its output' is what stops the next maintainer re-pinning a five-alarm failure"

requirements-completed: [CRYPTO-01, CRYPTO-02, CRYPTO-03]

coverage:
  - id: D1
    description: "Argon2id is pinned against RFC 9106 §5.3's published tag, at the RFC's own parameters (m=32 KiB, t=3, p=4, 32-byte tag, version 19) with its secret and associated data — a value this codebase did not produce"
    requirement: CRYPTO-02
    verification:
      - kind: integration
        ref: "cargo test --test sync_vectors — argon2_crate_matches_the_rfc_9106_test_vector"
        status: pass
    human_judgment: false
  - id: D2
    description: "BLAKE3 keyed_hash and derive_key are each pinned against the official reference test vectors' input_len=1024 case, using the vectors' own key and context string"
    requirement: CRYPTO-01
    verification:
      - kind: integration
        ref: "cargo test --test sync_vectors — blake3_keyed_hash_matches_the_official_reference_test_vector, blake3_derive_key_matches_the_official_reference_test_vector"
        status: pass
    human_judgment: false
  - id: D3
    description: "XChaCha20-Poly1305 is pinned against draft-arciszewski-xchacha-03 §A.3.1 — key, 24-byte nonce, associated data, plaintext, ciphertext and tag all from the draft"
    requirement: CRYPTO-01
    verification:
      - kind: integration
        ref: "cargo test --test sync_vectors — xchacha20poly1305_matches_the_draft_arciszewski_xchacha_03_test_vector"
        status: pass
    human_judgment: false
  - id: D4
    description: "The KEK is pinned for a fixed password, a fixed 16-byte salt and fixed cheap parameters, so a change to the algorithm, version, output length or argument order fails loudly"
    requirement: CRYPTO-02
    verification:
      - kind: integration
        ref: "cargo test --test sync_vectors — the_key_encryption_key_is_pinned_for_a_fixed_password_and_salt"
        status: pass
    human_judgment: false
  - id: D5
    description: "chunk_key, name_key and root_key are pinned individually from a fixed master key, and each is additionally proved to be the key its own path uses — the failure a value pin alone cannot catch"
    requirement: CRYPTO-01
    verification:
      - kind: integration
        ref: "cargo test --test sync_vectors — the_chunk_subkey_is_pinned_and_is_the_key_the_seal_path_uses, the_name_subkey_is_pinned_and_is_the_key_the_chunk_id_uses, the_root_subkey_is_pinned_and_is_the_key_the_snapshot_root_uses"
        status: pass
    human_judgment: false
  - id: D6
    description: "A keyfile wrapped by hand from docs/sync-format.md §1–2 (canonical AAD field order, base64 salt/nonce, format version 1) opens through Keyfile::open using the parameters stored in it rather than the compiled 1 GiB default"
    requirement: CRYPTO-02
    verification:
      - kind: integration
        ref: "cargo test --test sync_vectors — a_keyfile_wrapped_by_hand_from_the_documented_format_opens"
        status: pass
    human_judgment: false
  - id: D7
    description: "A chunk id over a fixed short plaintext is pinned and labelled STABLE across zstd upgrades, with the assertion message stating that a move means dedup broke for every existing user and is not a vector to regenerate"
    requirement: CRYPTO-01
    verification:
      - kind: integration
        ref: "cargo test --test sync_vectors — the_chunk_id_of_a_fixed_plaintext_is_pinned_and_must_survive_a_zstd_upgrade"
        status: pass
    human_judgment: false
  - id: D8
    description: "The full sealed ciphertext of that same plaintext is pinned, labelled zstd-sensitive with instructions to check the id pin first, and the same input sealed twice is asserted byte-identical"
    requirement: CRYPTO-03
    verification:
      - kind: integration
        ref: "cargo test --test sync_vectors — the_sealed_chunk_ciphertext_is_pinned_and_is_zstd_sensitive"
        status: pass
    human_judgment: false
  - id: D9
    description: "A 700 KiB three-chunk bundle's manifest chunk id (id pin) and pack content address (ciphertext pin) are both pinned, catching a frame-layout drift or a serialization-order change in the manifest or pack header"
    requirement: CRYPTO-03
    verification:
      - kind: integration
        ref: "cargo test --test sync_vectors — the_multi_chunk_bundles_manifest_id_is_pinned_and_must_survive_a_zstd_upgrade, the_multi_chunk_bundles_pack_address_is_pinned_and_is_zstd_sensitive"
        status: pass
    human_judgment: false

# Metrics
duration: 40min
completed: 2026-08-19
status: complete
---

# Phase 1 Plan 07: Pinned Regression Vectors Summary

**A round-trip test stays self-consistent under any wrong-but-stable transform, so an `argon2` 0.6
bump or a mistyped BLAKE3 context string would pass every existing test while making every bundle on
a user's disk unreadable. `tests/sync_vectors.rs` closes that: four vectors from the primitives' own
specifications, nine regression pins of the composed format, and — the part that mattered most — a
labelling discipline that tells a future maintainer whether a red pin means "zstd changed its
output" or "dedup broke for every existing user".**

## Performance

- **Duration:** ~40 min
- **Tasks:** 2/2
- **Files created:** 1 (`tests/sync_vectors.rs`, 707 lines). **No `src/` file touched.**
- **Commits:** `40eb9e4` (primitive vectors), `7237f7f` (composed pins)
- **Runtime:** 0.17 s for the whole file — well inside the AUR `check()` budget

## The distinction the file is built around

Every pin says, in its own assertion message, which of two kinds it is. That is the deliverable as
much as the hex is.

| | **Id pin** | **Ciphertext pin** |
|---|---|---|
| What it covers | `blake3::keyed_hash(name_key, raw plaintext)` | sealed bytes over the *compressed* frame |
| zstd upgrade | **cannot** legitimately move it | **may** legitimately move it |
| A move means | every chunk in every existing bundle is re-addressed: dedup gone for every user, full re-upload, no old bundle resolves | the on-disk bytes changed; a compatibility decision is owed. Old bundles still open — what is lost is byte-identical re-sealing across the boundary |
| The right response | **do not re-pin.** Find out what re-addressed the world | check the id pins first; if they held, re-pin deliberately |

Id pins: the chunk id, and the multi-chunk bundle's manifest id. Ciphertext pins: the sealed chunk,
the pack content address, and the wrapped master key.

The two are deliberately taken over *the same input* where possible — one short fixed plaintext gets
both an id pin and a ciphertext pin — so a maintainer can read the two results side by side and
classify the failure in one glance instead of re-deriving the format.

## Part 1 — the published vectors

Fetched at authoring time and cited inline, in the style of `safe_storage.rs`'s "Independently
reproduced with OpenSSL's PBKDF2-HMAC-SHA1 implementation". Nothing was invented and nothing was
substituted; every source was reachable.

| Primitive | Source | What it guards |
|---|---|---|
| Argon2id | RFC 9106 §5.3 — m = 32 KiB, t = 3, p = 4, 32-byte tag, version 19, with the RFC's own 8-byte secret and 12-byte associated data | the `argon2` crate itself, independently of `KdfParams` |
| BLAKE3 `keyed_hash` | the BLAKE3 team's `test_vectors.json`, `input_len: 1024` | every chunk address in the format |
| BLAKE3 `derive_key` | the same file and case, using its own `context_string` | the whole subkey hierarchy — the mode where a typo is invisible to every round trip |
| XChaCha20-Poly1305 | draft-arciszewski-xchacha-03 §A.3.1 | every sealed object |

Two notes worth keeping:

- **The RFC vector is encoded exactly as published.** The plan allowed for `argon2`'s chosen feature
  set not exposing the secret and associated-data inputs, with a substitute vector as the fallback.
  It does expose them — `Argon2::new_with_secret` and `ParamsBuilder::data` are both available under
  `default-features = false, features = ["alloc", "zeroize"]` — so no fallback was needed and the
  vector was not weakened into a self-comparison.
- **`input_len = 1024` is BLAKE3's own chunk size**, so the case crosses the tree-hashing boundary
  that a single-block vector would never reach.

This file imports `argon2` and `chacha20poly1305` directly, and that is the point: calling the
primitive itself is what makes these guards on the *crates* rather than on our wrapper. The
containment invariant `only_the_crypto_module_imports_the_cryptographic_crates` walks `src/sync/`,
so it is unaffected.

## Part 2 — the composed pins

Labelled honestly as **regression pins of current behaviour**, generated by running the
implementation once against the tree after 1-06 and 1-09 and pasting the result. Nobody publishes a
vector for this format; their value is that they change loudly, not that they are externally correct.

### The subkeys are pinned individually — and wired

Three separate literals for `chunk_key`, `name_key` and `root_key` from a fixed master key. A value
pin alone is not enough: a context string attached to the wrong field yields a working system with a
silently different hierarchy, and the hierarchy stays self-consistent under any permutation of the
three, so no round-trip test anywhere can see it. Each pin therefore also proves the *wiring*, each
reconstructed from `docs/sync-format.md` rather than shared with `crypto.rs`:

- `name_key` → `keys.chunk_id(pt)` must equal `blake3::keyed_hash(name_key, pt)`;
- `chunk_key` → `seal_chunk` must equal a sealing built from the documented rule
  (`nonce = derive_key(CTX_NONCE, id)[..24]`, `aad = id`, `msg = frame(pt)`), which pins `CTX_NONCE`
  as a side effect;
- `root_key` → `Keys::open_root` must accept a root framed **by hand** as
  `nonce ‖ ciphertext ‖ tag` under `root_key` with the literal `ai-usagebar.sync.v1 root` as AAD.
  The root's nonce is random by design, so the proof has to run in this direction.

If `seal_root` reached for `chunk`, or `chunk_id` for `root`, one of these three fails while every
round trip in the repository stays green.

### The hand-wrapped keyfile, and what it accidentally bought

Pinning subkeys needs a `Keys` whose master key is *known*. `Keyfile::create` draws a random one and
there is no `src/` seam for a fixed master — and this plan has no authority to add one. So the test
wraps `MASTER` by hand: `derive_kek` at the cheap seam, then XChaCha20-Poly1305 under a canonical AAD
struct written **from `docs/sync-format.md` §1** rather than shared with `crypto.rs`.

That obstacle turned into coverage. `Keyfile::open` accepting that keyfile is the only assertion
anywhere in the repository that the *documented* wrap format and the *implemented* one are the same
thing — the AAD's field order, the base64 encodings, the version gate. And because the handcrafted
keyfile stores `m_kib = 8` while the compiled default is 1 GiB, its opening also re-proves CRYPTO-02's
"use the parameters stored in this keyfile, never your own default" from outside the crate.

### The full round trip

A deterministic 700 KiB xorshift payload — two full 256 KiB chunks and a tail — sealed into a pack,
with the manifest naming it sealed into the same pack, exactly as a sync performs it.

- **Manifest chunk id** (id pin). Transitively pins the three payload ids, the file entry's path,
  mode and length, and serde's field order for `Manifest` and `FileEntry` — all of it inside a keyed
  hash of raw JSON, so it is zstd-independent all the way down. The three payload ids are pinned
  alongside it so a failure says *which level* moved.
- **Pack content address** (ciphertext pin). Unkeyed BLAKE3 over the finished pack, so it covers
  every sealed frame, the sealed header and the trailer. It is the format's broadest drift detector
  and the one most likely to move for a legitimate reason.

Plus the determinism assertion the plan asked for, mirroring `safe_storage.rs`'s
`assert_eq!(encrypt(&k, b"same"), encrypt(&k, b"same"))` — security-relevant here rather than a
convenience, since a non-deterministic seal kills dedup.

## What the tree as it stands forced, versus the plan as written

The plan predates 1-06 and 1-09. Three things it could not have known, all pinned as the format
finally is:

1. **`Keys::open`'s refusal names the chunk** (`chunk {id} failed authentication`), because 1-06
   found five distinct attacks collapsing to one byte-identical string. Nothing here re-pins the old
   string; `tests/sync_adversarial.rs` owns that message and pins it by equality already.
2. **`Root.manifest_id` is `manifest_chunks: Vec<ChunkId>`** and both the manifest and the index
   object are multi-chunk (1-09). `MANIFEST_VERSION` and `ROOT_VERSION` are 2, and the single-chunk
   size refusal was deleted rather than raised. The round-trip pin is written against that shape —
   it asserts the manifest id list's length rather than assuming a single id.
3. **Resolved crate versions**, recorded in the module doc so a future reader knows what the hex was
   produced against: `argon2` 0.5.3, `chacha20poly1305` 0.11.0, `blake3` 1.8.6, `zstd` 0.13.3,
   `zeroize` 1.9.0, **`getrandom` 0.4.2** (the phase research named 0.4.3; 0.4.2 is what actually
   resolved — recorded as the fact it is, not silently substituted).

## Hermeticity and cost

No `$HOME`, no `$XDG`, no Keychain, no network at run time, no clock, no randomness. Verified by
running the compiled test binary with `HOME`, `XDG_CACHE_HOME`, `XDG_CONFIG_HOME` and `XDG_DATA_HOME`
all unset: 13 passed.

Every derivation that is not deliberately pinning a published parameter set uses the cheap-KDF seam
(`m_kib = 8, t = 1, p = 1`). The 700 KiB pack is built once behind a `OnceLock`, following 1-06's
precedent — everything is deterministic, so sharing changes no outcome and is purely for time. Whole
file: **0.17 s**.

## Blockers

**None.** Every published vector's source was reachable, so no `todo!` placeholder was left. Every
composed pin matched a directly computed expectation on the first run — no chunk id disagreed with
`keys.chunk_id(plaintext)`, no ciphertext disagreed with an independently reconstructed sealing, and
the hand-wrapped keyfile opened first time. Nothing is handed to the verifier as a finding.

## For whoever sees one of these go red

The module doc says it and every assertion message repeats it: **the question is which of the two
changed and why — never "regenerate it until green"**. This plan ran last, alone, in its own wave,
precisely so a red pin here is never routine. 1-06 held edit authority over `src/sync/` and used it;
a pin written before that landed would have gone red on merge and taught exactly the habit these
pins exist to prevent.
