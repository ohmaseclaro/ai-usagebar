---
phase: 01-encrypted-bundle-core
verified: 2026-08-19T18:55:00Z
status: human_needed
score: 6/6 roadmap success criteria verified (11/11 must-haves including the five format-defect probes)
behavior_unverified: 0
overrides_applied: 0
requirements:
  - id: CRYPTO-01
    status: satisfied
  - id: CRYPTO-02
    status: satisfied
  - id: CRYPTO-03
    status: satisfied
  - id: CRYPTO-05
    status: satisfied
  - id: CRYPTO-06
    status: satisfied
  - id: CRYPTO-07
    status: satisfied
deferred:
  - truth: "The generated-passphrase path is the *user-facing* default"
    addressed_in: "Phase 3"
    evidence: "ROADMAP Phase 3: `ai-usagebar sync init` — guided end to end: choose repo -> set password -> choose categories; SC3 'sync init completes on a mock private repo: repo chosen, password set'. Phase 1 delivers generate()/check()/NO_RECOVERY and the argv/env prohibition; the surface that defaults to them is Phase 3."
  - truth: "`--allow-rollback` is passed as a CLI flag"
    addressed_in: "Phase 5"
    evidence: "ROADMAP Phase 5 (Pull and Restore) owns the restore surface. Phase 1 ships `anchor::accept(..., allow_rollback: bool)` as a pure, tested decision; the flag that supplies the bool is a later surface."
  - truth: "src/sync/ is called by a command"
    addressed_in: "Phases 3, 4, 6"
    evidence: "ROADMAP Phase 1 Scope-out: 'any network call; sync CLI subcommands'. Phase 3 auth, Phase 4 push, Phase 6 surfaces. Confirmed by grep: no production consumer of crate::sync exists, which is the declared design, not an omission."
human_verification:
  - test: "CAL-1 — create a throwaway private GitHub repo with a release asset >1 MiB, export GSD_CAL1_TOKEN and GSD_CAL1_REPO, then run `cargo test --test live -- --ignored --nocapture cal1_range_on_private_release_asset`"
    expected: "The probe prints whether the 302 to signed storage honours `Range:`. If honoured, Phase 3 may raise `sync::pack::PACK_TARGET` above 32 MiB; if not, the shipped 32 MiB fallback is already correct and nothing changes."
    why_human: "Needs a real private repo and a token. Phase 1 is deliberately credential-free and offline, so this cannot run in the hermetic suite. The fallback is already shipped in `src/sync/pack.rs:69`, so this blocks nothing — it is an optimisation input, not a correctness gate."
  - test: "CAL-3 aarch64-Linux leg — on a slow aarch64 Linux box (ideally ~1 GB RAM class), run `cargo test --release --test live -- --ignored --nocapture cal3_argon2id_timing_at_production_parameters`"
    expected: "A wall-clock number for m=1 GiB/t=3/p=1 on constrained aarch64 Linux, and confirmation that a box that cannot afford the working set gets the `--kdf-memory` refusal from `check_memory_budget` rather than an OOM kill."
    why_human: "Requires physical access to slow aarch64 Linux hardware. Deliberately not faked with Docker on this M3 Max: a Linux VM on the same silicon answers the OS question, not the slow-hardware question, and would read as a clearance for constrained targets. Measured on M3 Max (1503/1492/1548 ms), documented as the fast end of the range in docs/sync-format.md §7."
---

# Phase 1: Encrypted Bundle Core — Verification Report

**Phase Goal:** The bundle format — key hierarchy, chunker, pack, snapshot — exists and survives an
attacker who controls the remote, proven entirely by hermetic tests with no network, no `$HOME`,
and no GitHub credentials.

**Verified:** 2026-08-19
**Status:** human_needed — no gaps, no blockers; two live-gated calibration items await hardware/credentials
**Re-verification:** No — initial verification

## Verdict

**The format is safe to build the rest of the milestone on.** Every property whose defect would be
permanent — chunk addressing, sealing addresses, version negotiation, object sizing, and failure
distinguishability — was checked against the code and against a passing test, not against SUMMARY
prose. All five defects flagged as "caught during the phase" have fixes that genuinely hold; each is
enumerated by consumer below rather than spot-checked.

Status is `human_needed` rather than `passed` solely because two calibration items are live-gated.
Neither is a gap: both have a shipped fallback, and neither can change a format constant that is
already load-bearing.

## Independently measured gates

Not taken from SUMMARY.md — re-run on the integrated tree during this verification.

| Gate | Command | Result |
|---|---|---|
| Full suite, `$HOME` unset, offline | `env -u HOME cargo test --offline` | **1095 passed, 0 failed, 11 ignored** (1066 + 3 + 13 + 13) |
| Adversarial suite | `--test sync_adversarial` | 13 passed, 0 failed |
| Vector suite | `--test sync_vectors` | 13 passed, 0 failed |
| Lint | `cargo clippy --offline --all-targets -- -D warnings` | clean |
| Format | `cargo fmt --check` | clean |
| Runtime | sync suites | ~3.7 s total — comfortably inside the AUR `check()` budget |

The 11 ignored are `tests/live.rs` probes, confirmed excluded from the default set so the AUR
`check()` never touches the network.

`cargo machete` is **not installed on this verifier machine**, so I substituted the check it
performs: every one of the six new crates is referenced in `src/` — `blake3` (7), `zeroize` (5),
`chacha20poly1305` (2), `zstd` (2), `getrandom` (2), `argon2` (1). The single `argon2` reference is
the containment invariant, not an unused dep.

## Goal Achievement — Roadmap Success Criteria

| # | Success Criterion | Status | Evidence |
|---|---|---|---|
| 1 | Multi-MB fixture round-trips byte-exactly; sealing twice is identical | ✓ VERIFIED | `the_full_stack_round_trips_a_multi_megabyte_fixture_byte_exactly` and `sealing_one_fixture_twice_produces_identical_packs_and_identical_chunk_ids` both pass. Determinism is structural: chunk nonce = `derive_key(CTX_NONCE, id)` and `id` = keyed hash of plaintext, so identical plaintext yields identical ciphertext — convergent, with no nonce-reuse XOR leak. |
| 2 | Five attacks each fail with one distinct error and zero plaintext | ✓ VERIFIED | Nine attacks, not five. All nine pass; `the_nine_refusals_carry_nine_distinct_messages` asserts pairwise distinctness. `assert_secret_free` scans each message against key material and plaintext. |
| 3 | A snapshot below the high-water mark is refused unless rollback is allowed | ✓ VERIFIED | `anchor::accept` (`src/sync/anchor.rs:67`) refuses on `remote_counter < local.counter && !allow_rollback`, and separately refuses a `repo_id` mismatch — a counter is meaningless across bundles. `attack_9_a_rolled_back_snapshot_is_refused_unless_explicitly_allowed` passes. CLI flag deferred to Phase 5 (see `deferred`). |
| 4 | Password <12 refused; generated path default + no-recovery warning; nothing from argv or env | ✓ VERIFIED | `MIN_CHARS=12` rejects, `RECOMMENDED_CHARS=20` warns, `GENERATED_CHARS=20` Crockford base32 = exactly 100 bits. `the_no_recovery_text_states_the_consequence_without_hedging` pins the text and bans hedging; a second test bans jargon. `no_password_input_path_reads_the_process_environment` scans shipped (non-test, non-comment) source for `std::env`/`env::var`/`var_os`/`clap`/`Arg::new`. |
| 5 | `cargo test` passes with `$HOME` unset and no network, inside the AUR budget | ✓ VERIFIED | Re-run by me under `env -u HOME --offline`: 1095 pass. |
| 6 | clippy `-D warnings` and machete clean; no system `-dev` package needed | ✓ VERIFIED | clippy clean (run by me). machete substituted (tool absent — see above). `argon2 default-features=false`, `blake3` no-default + `std`, `chacha20poly1305` no-default + `alloc`, `zstd` vendors its C — nothing to detect at build time. |

**Score: 6/6 roadmap success criteria verified.**

## The five format-defect probes

Each was reported as caught-and-fixed during the phase. Each is verified by enumerating consumers,
not by reading the fix.

### 1. Chunk ids address raw plaintext, never the compressed frame — ✓ HOLDS

`src/sync/chunk.rs:154-162`:

```rust
pub fn seal_chunk(keys: &Keys, data: &[u8]) -> Result<Blob> {
    let id = keys.chunk_id(data);   // raw plaintext, line 155
    let framed = frame(data)?;      // compression happens after, line 156
```

The id is computed on `data` before `frame()` is ever called. The read side mirrors it:
`open_chunk` (line 171-180) unframes first, then rechecks `keys.chunk_id(&plaintext) != *id` against
the *decompressed* plaintext. The module docs (lines 4-17) state the failure mode explicitly, and
the invariant is pinned by two passing tests whose names encode it:
`the_chunk_id_of_a_fixed_plaintext_is_pinned_and_must_survive_a_zstd_upgrade` and
`the_multi_chunk_bundles_manifest_id_is_pinned_and_must_survive_a_zstd_upgrade`. Their siblings
`..._is_zstd_sensitive` pin the ciphertext, so the two axes cannot drift into each other unnoticed.
A zstd bump re-ids nothing.

### 2. Nothing is sealed under an unkeyed address — ✓ HOLDS

I grepped every `content_address` call site across `src/`, `tests/`, and `docs/`. There is exactly
**one production call**:

- `src/sync/pack.rs:176` — `Ok((content_address(&out), out))`, where `out` is the finished pack:
  blob ciphertexts + sealed header ciphertext + trailer. All public ciphertext. Naming only.

The remaining hits are the definition (`crypto.rs:504`), doc comments, and test assertions. The pack
*header* — the list of chunk ids an attacker already holds — is sealed via `seal_chunk` at
`pack.rs:170`, so its address is `keys.chunk_id(json)`, a **keyed** hash written to the trailer at
line 174. The oracle is closed. `content_address_is_unkeyed_and_therefore_key_independent`
(`crypto.rs:797`) pins the distinction by asserting a keyed id and an unkeyed address differ, and the
doc comment at `crypto.rs:499` states the prohibition in the imperative for the next reader.

### 3. Version checks are at-or-below a ceiling, never equality — ✓ HOLDS

One implementation, `sync::check_version` (`mod.rs:87`), using `if found <= ceiling`. Four call
sites, all routed through it:

| Site | Object |
|---|---|
| `crypto.rs:312` | keyfile |
| `model.rs:110` (via `probe_version`) | manifest / root / index object |
| `pack.rs:212` | pack header |

A regex sweep for `version ==`, `format ==`, `!= *VERSION` across `src/sync/` returns **zero**
matches in production code. The only `assert_eq!` on a format field is `pack.rs:329`, a test
asserting what a writer wrote. `model.rs`'s `probe_version` deserializes a minimal `VersionProbe`
*before* the full object so a v2 object carrying unknown required fields reports "your client is
old", not "missing field" — the correct message for CRYPTO-02. `mod.rs:45-50` records why equality
would be wrong. `version_check_accepts_at_or_below_the_ceiling_and_refuses_only_above` passes.

### 4. Manifest and index objects are multi-chunk; the refusal was deleted — ✓ HOLDS

Confirmed against the 1-09 diff (`git show 6aa621e`), not the summary. The removed lines include:

```
-    if json.len() > CHUNK_SIZE {
-            "this {object} serializes to {} bytes, past the {CHUNK_SIZE}-byte single-chunk \
-    fn a_four_thousand_file_manifest_exceeds_one_chunk_and_is_refused_by_name() {
```

The guard is **deleted**, not raised — the current `seal_object` (`model.rs:121-127`) has no length
check at all; it serializes and hands off to `chunk::seal_all`, which splits. `CHUNK_SIZE` no longer
appears in model.rs's imports.

**`IndexObject` got the fix by construction, not by a parallel edit.** Both objects route through
the same two helpers:

| Object | seals via | opens via |
|---|---|---|
| `Manifest` | `seal_object` (`model.rs:190`) | `open_object` (`model.rs:207`) |
| `IndexObject` | `seal_object` (`model.rs:265`) | `open_object` (`model.rs:269`) |

No `seal_chunk`/`open_chunk` call survives in `model.rs`. `Root.manifest_id: ChunkId` became
`manifest_chunks: Vec<ChunkId>` with `ROOT_VERSION`/`MANIFEST_VERSION` at 2 and ceilings raised in
step — so a v2 client still reads a v1 bundle, which is the point of probe 3.

Ordering is closed rather than assumed: `reassemble` documents that a chunk carries no position, and
the order lives inside the root's *sealed* plaintext, so transposing it requires the key.
`transposing_manifest_chunks_after_the_root_is_sealed_yields_zero_entries` proves a reordered list
errors (`"manifest is malformed"`) and leaks no path fragment;
`dropping_the_last_manifest_chunk_is_refused_rather_than_read_short` covers truncation.

*Note, not a gap:* the **pack header** still seals as a single chunk (`pack.rs:170`) and retains
`seal_chunk`'s `CHUNK_SIZE` ceiling. `pack.rs:163-167` reasons this out — a 32 MiB pack of 256 KiB
chunks holds ~128 entries against a limit of some thousands, and the failure is a clean error, not
truncation. That is slack, not the latent ceiling 1-09 closed. Worth re-checking only if
`PACK_TARGET` rises after CAL-1.

### 5. CRYPTO-03 "unambiguous" — messages distinct, forced collision pinned — ✓ HOLDS

`the_nine_refusals_carry_nine_distinct_messages` (`sync_adversarial.rs:797`) collects one message per
attack and asserts pairwise inequality across all 36 pairs, with a failure message that names which
two attacks collapsed. It passes.

The one **forced** collision is pinned by a test, not left to a comment. `attack_2` runs two legs:

- Leg 1 — an in-range downgrade (`m_kib` 64 → 8) rewritten in the *serialized* keyfile, so the AAD
  binding is genuinely exercised. `assert_eq!(shared, "wrong password or corrupted keyfile")` at
  line 435 — deliberately byte-identical to attack 1, because both produce a wrong KEK and any
  message distinguishing them would be an oracle.
- Leg 2 — a downgrade below Argon2's own floor (`m_kib` 4), refused by name before allocation:
  `"invalid Argon2id parameters (m_kib=4, t=1, p=1)"`. This is the message the distinctness test
  collects, since leg 1's belongs to attack 1 by cryptographic necessity.

Line 445 then asserts the untouched keyfile still opens, so the refusals are the AAD binding rather
than a keyfile that never worked. A future "helpful" split of the shared message is caught by leg 1's
`assert_eq!`, not merely discouraged by prose.

## Key Link Verification

| From | To | Via | Status |
|---|---|---|---|
| `src/lib.rs` | `src/sync/` | `pub mod sync;` (line 40) | ✓ WIRED |
| `src/sync/mod.rs` | six submodules | `pub mod` × 6 | ✓ WIRED |
| `chunk.rs` | `crypto.rs` | `Keys::chunk_id` / `seal` / `open`; constructs no cipher | ✓ WIRED |
| `pack.rs` | `chunk.rs` + `crypto::content_address` | seals through `seal_chunk`, names through `content_address` | ✓ WIRED |
| `model.rs` | `chunk.rs` | `seal_all` / `reassemble` (multi-chunk) | ✓ WIRED |
| root → manifest_chunks → manifest → chunk ids → chunks | — | every hop names the next and binds it as AAD | ✓ WIRED |
| crypto-crate containment | `crypto.rs` only | `only_the_crypto_module_imports_the_cryptographic_crates` scans `src/sync/*.rs` from `CARGO_MANIFEST_DIR` | ✓ ENFORCED BY TEST |

## Requirements Coverage

| Req | Description | Status | Evidence |
|---|---|---|---|
| CRYPTO-01 | Whole bundle encrypted client-side; remote never sees plaintext or password | ✓ SATISFIED | `no_fixture_plaintext_and_no_file_path_survives_into_the_pack_bytes` scans finished pack bytes for a planted marker and for file paths. Manifest (paths, modes, sizes) is itself sealed. |
| CRYPTO-02 | Memory-hard KDF, params stored alongside data, raisable without breaking bundles | ✓ SATISFIED | Argon2id params live in the keyfile's `kdf` block and are read from it, not from the compiled-in default; canonical `format`+`kdf` serialization is the AAD. At-or-below version ceilings (probe 3) are what make raising them non-breaking. `rewrap` (`crypto.rs:277`) rewraps the same master key under a new password — the CRYPTO-04 primitive the roadmap assigns here even though the command is Phase 4. |
| CRYPTO-03 | Wrong password fails cleanly and unambiguously; never partial or garbage | ✓ SATISFIED | Probe 5. Plus `reassemble` returns `Result` with a `Zeroizing` accumulator dropped on `?`, so a failure yields no partial buffer — "zero entries" is structural. |
| CRYPTO-05 | Tampering, reordering, truncation, rollback detected on pull; refuses to restore | ✓ SATISFIED | Attacks 3-9: chunk swap both directions, flipped bit, truncated pack, truncated manifest, transposed manifest ids, rolled-back snapshot. Bounds-checked before every slice (`checked_sub` at `pack.rs:202`, `checked_add` at `chunk.rs:112`, `entries_within` at `pack.rs:224`). |
| CRYPTO-06 | Password strength enforced at set time, offline-attack risk in plain language | ✓ SATISFIED | 12-char hard floor, 20-char warning, `OFFLINE_ATTACK_NOTE` with a test banning the jargon "entropy", "Argon2", "KDF", "bits of", "brute-force". |
| CRYPTO-07 | Key material zeroized, never in argv, env, logs, or error messages | ✓ SATISFIED | `Zeroizing` on all three subkeys and every derived value; hand-written `Debug for Keys` printing `Keys { <redacted> }` (`crypto.rs:162`); explicit `.zeroize()` on the AEAD's returned `Vec` at `crypto.rs:336,347`; `assert_secret_free` on every refusal message; the argv/env source scan. |

**Orphans:** none. CRYPTO-04 maps to Phase 4 in REQUIREMENTS.md and is not claimed here; its Phase 1
primitive (`rewrap`) is nonetheless present and tested, matching the roadmap's assignment note.

## Anti-Patterns Found

**None.** Scanning the 15 files this phase touched:

- Zero `TBD` / `FIXME` / `XXX` / `TODO` / `HACK` / `PLACEHOLDER` / "not yet implemented".
- Zero `.unwrap()`, `panic!`, or `unreachable!` in production code across all six `src/sync/`
  modules. The sole raw slice, `&hex[..2]` (`pack.rs:262`), indexes a 64-char hex string generated
  internally from a 32-byte id.
- Every `expect()` in production sits behind a `try_into()` on a statically-sized slice already
  bounds-checked above it.

## Deliberately descoped — confirmed, not missing by accident

Verified rather than accepted on assertion:

- **No production consumer of `crate::sync`.** `grep -rn "sync::" src/ tests/` outside `src/sync/`
  returns only `std::sync` / `tokio::sync` (unrelated) and the `tests/live.rs` CAL-3 probe. The
  module is `pub` and compiles into the crate; nothing calls it. This is ROADMAP Phase 1
  Scope-out verbatim, and wiring lands in Phases 3/4/6.
- **No password recovery.** Zero-knowledge is the threat model; `NO_RECOVERY` states it without
  hedging and a test enforces the absence of hedging.
- **TOFU on first contact.** `anchor::accept` returns `Ok(())` when `local` is `None` (line 73-75),
  documented at the function and in `docs/sync-format.md` §9 "Residual risk: trust-on-first-use on
  the rollback anchor". An accepted, recorded residual risk.
- **Scope discipline.** The phase touched no frontend file — `Cargo.{toml,lock}`, `README.md`,
  `docs/sync-format.md`, `src/lib.rs`, six `src/sync/*.rs`, three test files. The GNOME/KDE/Omarchy
  contract suites are untouched, which is why the reported `make test` pass is credible.

## Documentation

`docs/sync-format.md` (612 lines) carries all nine sections the deliverable named: key hierarchy,
keyfile, chunking (with "The chunk id addresses the raw plaintext" as its own subsection), frame
layout, pack files, object graph, versioning and evolution, both calibrations (§7 — CAL-3 measured
with the machine named and the missing aarch64 leg stated as missing; CAL-1 marked "Not measured"
with the fallback labelled a fallback), §8 "What this format does not hide", and §9 "Honest limits".
The accepted metadata leakage is stated plainly rather than omitted.

## Human Verification Required

Two items, both live-gated. Neither is a gap; both have a shipped fallback that blocks nothing.

### 1. CAL-1 — `Range:` on a private-repo release asset

**Test:** Create a throwaway private repo with a release asset >1 MiB, export `GSD_CAL1_TOKEN` and
`GSD_CAL1_REPO`, then run
`cargo test --test live -- --ignored --nocapture cal1_range_on_private_release_asset`.
**Expected:** The probe reports whether the 302 to signed storage honours `Range:`. If yes, Phase 3
may raise `sync::pack::PACK_TARGET` above 32 MiB; if no, the shipped value is already right.
**Why human:** Needs a real private repo and a token, which this phase deliberately does not have.
The 32 MiB fallback is already in the constants (`pack.rs:69`), so this is an optimisation input.

### 2. CAL-3 — the aarch64-Linux leg

**Test:** On a slow aarch64 Linux box, run
`cargo test --release --test live -- --ignored --nocapture cal3_argon2id_timing_at_production_parameters`.
**Expected:** A wall-clock figure at m=1 GiB/t=3/p=1 on constrained hardware, plus confirmation that
an under-provisioned box gets `check_memory_budget`'s `--kdf-memory` refusal rather than an OOM kill.
**Why human:** Requires physical slow aarch64 Linux hardware. Docker was deliberately refused — a
Linux VM on this same M3 Max answers the OS question, not the slow-hardware question, and would read
as a clearance for constrained targets. Parameters are configurable and travel in the keyfile, which
is the documented fallback.

## Gaps Summary

**None.** No must-have failed, no artifact is a stub, no key link is unwired, and no blocker-class
anti-pattern exists. The two open items are calibration measurements requiring hardware and
credentials this phase deliberately excluded; each has a shipped, documented fallback and neither can
invalidate a format constant already in use.

The five defects caught during the phase were re-derived from the code and from passing tests rather
than accepted from SUMMARY prose. Each fix is enforced at the shared helper — one `check_version`,
one `seal_object`, one `content_address` call site, one id computation — so the invariants hold
across every consumer rather than at the site where the bug was first noticed.

---

_Verified: 2026-08-19_
_Verifier: Claude (gsd-verifier)_
