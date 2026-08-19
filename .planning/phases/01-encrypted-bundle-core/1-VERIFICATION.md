---
phase: 01-encrypted-bundle-core
verified: 2026-08-19T21:40:00Z
status: gaps_found
score: 10/11 must-haves verified (6/6 roadmap success criteria; 4/5 post-audit change claims)
behavior_unverified: 0
overrides_applied: 0
re_verification:
  previous_status: human_needed
  previous_score: 6/6 roadmap success criteria (11/11 must-haves)
  previous_verified_at_tree: pre-1-10 (VERIFICATION.md written 15:25; 1-10 landed 15:55, 1-11 at 16:20)
  gaps_closed: []
  gaps_remaining: []
  regressions:
    - "docs/sync-format.md §5 line 390 still carries the pre-F-1 nonce rule verbatim — the exact behaviour the security audit removed as a nonce-reuse vulnerability. Present at commit ed56b39 (the audit commit) and unchanged through both remediation rounds."
  notes: "The previous report is superseded, not amended: it judged the tree at 1095 tests, before the on-disk format changed twice (inline 24-byte chunk nonce, repo_id in the root AAD). Every truth below was re-derived from the current tree; nothing was carried forward on trust."
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
gaps:
  - truth: "No documentation describes behaviour the code does not implement"
    status: failed
    reason: >-
      docs/sync-format.md §5 states that "every other object's nonce is derived from its
      content address". The code derives the chunk nonce from the framed bytes actually
      encrypted — §3 of the same document says so explicitly and in bold ("never from the
      id"). "Content address" is this document's own term for the chunk id (§3 heading,
      §4 "content-addressed pack names"), so §5 records the pre-F-1 rule: the precise
      AEAD nonce-reuse vulnerability the phase spent two remediation rounds removing.
      The sentence is verbatim identical at commit ed56b39 (the audit commit, before any
      fix) and at HEAD — it is stale text, not a new formulation. This is the third
      instance of the defect class the phase has already been bitten by twice (F-4b was
      escalated as a tracked finding for exactly this), and it sits in the one document a
      Phase-2 re-implementer and every later phase read as the format contract.
    artifacts:
      - path: "docs/sync-format.md"
        issue: >-
          Line 390: "every other object's nonce is derived from its content address" —
          contradicts §3 lines 213-233 and contradicts src/sync/crypto.rs:606-614
          (chunk_nonce takes the message). Secondary, same paragraph: line 394 "hence the
          fixed AAD literal" is now incomplete — post-F-3 the root AAD is
          `ROOT_AAD ‖ repo_id`, which lines 366-372 of the same section state correctly.
    missing:
      - "Rewrite docs/sync-format.md:389-394 so §5's justification for the root's random nonce names the rule §3 actually implements (nonce derived from the bytes sealed), not the id."
      - "Amend line 394's 'the fixed AAD literal' to the literal-plus-repo_id form §5's own opening paragraph already documents."
      - "No code, ciphertext, pin or format change is required — the implementation is correct. This is a documentation-only fix."
deferred:
  - truth: "src/sync/ is reachable from a command"
    addressed_in: "Phases 3, 4, 6"
    evidence: "ROADMAP Phase 1 Scope-out: 'any network call ... sync CLI subcommands'. Re-confirmed by grep at HEAD: `pub mod sync` in src/lib.rs:40 and no production consumer of crate::sync. Declared design, not omission."
  - truth: "`--allow-rollback` is supplied as a CLI flag"
    addressed_in: "Phase 5"
    evidence: "ROADMAP Phase 5 (Pull and Restore) owns the restore surface. Phase 1 ships `anchor::accept(local, repo_id, counter, allow_rollback)` as a pure tested decision (src/sync/anchor.rs:98-127); the flag supplying the bool is a later surface."
  - truth: "The generated passphrase is the *user-facing* default and prints NO_RECOVERY"
    addressed_in: "Phase 3"
    evidence: "ROADMAP Phase 3 SC3: 'sync init completes on a mock private repo: repo chosen, password set'. Phase 1 delivers generate() / check() / NO_RECOVERY and the argv-and-env prohibition; the surface that defaults to them is Phase 3."
  - truth: "The chunk AAD carries an object-type domain separator (NEW-3)"
    addressed_in: "Phase 2"
    evidence: "Recorded as a deliberate deferral with its trigger in src/sync/crypto.rs:556-574, docs/sync-format.md:240-251, 1-SECURITY.md and .planning/ROADMAP.md Phase 2 (commit 311532d). Re-confirmed to dead-end at HEAD: aad = id is bound, open_chunk rechecks chunk_id(plaintext) == id, pack::read_header bounds-checks before yielding — a confused object errors, it does not decrypt."
human_verification:
  - test: "CAL-1 — create a throwaway private GitHub repo with a release asset >1 MiB, export GSD_CAL1_TOKEN / GSD_CAL1_REPO / GSD_CAL1_ASSET, then `cargo test --test live -- --ignored --nocapture cal1_range_on_private_release_asset`"
    expected: "206 Partial Content with a Content-Range header means ranged reads work and a later phase may raise PACK_TARGET above 32 MiB; 200 OK with the whole asset means the shipped 32 MiB fallback is already correct and nothing changes."
    why_human: "Needs a real private repo and a token. Phase 1 is deliberately credential-free and offline. Probe exists and is #[ignore]d (tests/live.rs:685); fallback PACK_TARGET = 32 MiB is shipped (src/sync/pack.rs:69), so nothing is blocked."
  - test: "CAL-3 aarch64-Linux leg — on genuinely slow aarch64 Linux hardware (Raspberry Pi 4/5 or a small ARM VPS, not a VM on Apple silicon), `cargo test --release --test live -- --ignored --nocapture cal3_argon2id_timing_at_production_parameters`"
    expected: "A wall-clock number for m=1 GiB / t=3 / p=1 on constrained aarch64 Linux, and confirmation that a box that cannot afford the working set gets check_memory_budget's actionable --kdf-memory refusal rather than an OOM kill."
    why_human: "Requires physical access to slow aarch64 Linux hardware. Docker on this M3 Max was deliberately refused: a Linux VM on the same silicon answers the OS question, not the slow-hardware question, and the number would read as clearance for constrained targets. M3 Max measured 1503/1492/1548 ms and is documented as the fast end of the range (docs/sync-format.md §7)."
---

# Phase 1: Encrypted Bundle Core — Verification Report (re-verification)

**Phase Goal:** The bundle format — key hierarchy, chunker, pack, snapshot — exists and survives an
attacker who controls the remote, proven entirely by hermetic tests with no network, no `$HOME`,
and no GitHub credentials.

**Verified:** 2026-08-19 (final tree, post 1-10 and 1-11)
**Status:** gaps_found — one documentation gap; the implementation itself is sound
**Re-verification:** Yes — the previous report judged a tree that no longer exists

## Verdict

**The cryptography is safe to build the rest of the milestone on. The document that records it is
not yet, by one sentence.**

Every property whose defect would be permanent was re-derived from the current tree: all six AEAD
call sites enumerated by hand, every version comparison in `src/sync/` enumerated, every id pin
diffed byte-for-byte against the pre-remediation commit, and the four gates re-run rather than
read. The two format changes (inline chunk nonce, `repo_id` in the root AAD) hold as invariants
and not merely as state that happens to be set — the argument for each is below, by consumer.

The one gap is that `docs/sync-format.md` §5 still asserts the nonce rule the code deliberately
does not implement — and the rule it asserts is the removed vulnerability. The code is right; the
spec is self-contradictory on the single property two audit rounds were spent on. That is a
one-sentence fix, and it is filed as a gap rather than a note because this exact defect class has
already been escalated once inside this phase (F-4b), and because the format doc is what Phase 2
and any re-implementer build against.

## Goal Achievement

### Observable Truths — roadmap success criteria

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | Multi-MB fixture round-trips byte-exactly chunk→zstd→seal→pack→unpack→open→reassemble; sealing the same bytes twice is byte-identical | ✓ VERIFIED | `the_full_stack_round_trips_a_multi_megabyte_fixture_byte_exactly`, `sealing_one_fixture_twice_produces_identical_packs_and_identical_chunk_ids` (tests/sync_adversarial.rs:389,404), `two_seals_of_one_input_are_byte_identical` (chunk.rs:341). All pass in the re-run. Determinism survived the nonce change because the derived nonce is a function of the framed bytes, which are themselves deterministic within a build. |
| 2 | Wrong password, downgraded KDF, chunk-under-another-id, one flipped bit, truncated manifest each fail with one distinct error and zero plaintext | ✓ VERIFIED | `attack_1`…`attack_9` plus `the_nine_refusals_carry_nine_distinct_messages` (sync_adversarial.rs:831-876) and `no_fixture_plaintext_and_no_file_path_survives_into_the_pack_bytes` (:424). Wrong password and KDF downgrade share one message by design (crypto.rs:488-491) — nothing distinguishes them and an attacker learns nothing from the difference. |
| 3 | A snapshot below the local high-water mark is refused unless `--allow-rollback` | ✓ VERIFIED | `anchor::accept` (anchor.rs:116-124) refuses `remote_counter < local.counter` unless the flag; `attack_9_a_rolled_back_snapshot_is_refused_unless_explicitly_allowed` run individually — passed. A present-but-unparseable anchor is an error, never a reset (anchor.rs:141-148) — the "free rollback" path is closed. `accept` decides and does not persist. Flag plumbing deferred to Phase 5. |
| 4 | A supplied password under 12 chars is refused; generated path is default; no code path takes a password from argv or env | ✓ VERIFIED | `passphrase::check` (passphrase.rs:174-186): `>= 20` Strong; below default `m_kib` anything under 20 Rejected; otherwise `< 12` Rejected. `<12` is refused at every KDF cost, so the criterion holds under the new rule. `generate()` emits 20 Crockford base32 chars = exactly 100 bits. `no_password_input_path_reads_the_process_environment` (passphrase.rs:423) greps for `std::env`, `env::var`, `var_os`, `clap`, `Arg::new`. |
| 5 | `cargo test` passes with `$HOME` unset and no network, inside the AUR `check()` budget | ✓ VERIFIED | Re-run: **1104 passed, 0 failed, 11 ignored** (exact match to the claimed number). `env -u HOME -u XDG_CACHE_HOME -u XDG_CONFIG_HOME cargo test --test sync_adversarial --test sync_vectors` → 13 + 13 pass. Unit suite 2.09 s, adversarial 1.36 s, vectors 0.17 s — the cheap-KDF seam (`m_kib = 8`) is what keeps it there. |
| 6 | `cargo clippy --all-targets -- -D warnings` and `cargo machete` clean; AUR source build needs no system `-dev` package | ✓ VERIFIED (one leg by substitute) | `cargo clippy --all-targets -- -D warnings` → exit 0, zero issues. `cargo fmt --check` → exit 0. `make test` → exit 0, GNOME + KDE + Omarchy contract suites all pass. `cargo machete` is **not installed on this machine**; verified by substitute instead — each of the six new crates has live call sites (`blake3`, `chacha20poly1305`, `argon2`, `zeroize` in crypto.rs; `zstd` in chunk.rs; `getrandom` in passphrase.rs). Install `cargo-machete` before cutting a release. |

### Observable Truths — the five post-audit change claims

| # | Claim | Status | Evidence |
|---|-------|--------|----------|
| 1 | Chunk nonce derives from the bytes actually encrypted, stored inline; `Keys::seal` is `pub(crate)` | ✓ VERIFIED | crypto.rs:606-614 — `chunk_nonce(message)` = `derive_key(CTX_NONCE, keyed_hash(name_key, message))[..24]`, taking `message`, not `id`. crypto.rs:584 `pub(crate) fn seal`. Framing `nonce ‖ sealed` at :596-599. Pinned as a *property*, not just as bytes, in sync_vectors.rs — the pin recomputes the nonce from the frame and names the reason if it moves. |
| 2 | `repo_id` is bound into the root's AAD; a repo swap fails Poly1305 | ✓ VERIFIED | `root_aad` (crypto.rs:742-754) = `b"ai-usagebar.sync.v1 root" ‖ repo_id`, shared by `seal_root` and `open_root`, with an empty `repo_id` refused inside the shared function so no caller can switch the scoping off (NEW-2). `Root::open` also rechecks the plaintext `repo_id` against the caller's (model.rs:367-371). `sync_vectors.rs:534-543` asserts `open_root(&framed, "some-other-bundle")` errors. |
| 3 | The KDF memory ceiling lives inside `derive_kek`, not at callers | ✓ VERIFIED | crypto.rs:176 — `check_kdf_ceiling(k.m_kib)?` is the first line of `derive_kek`, ahead of `Params::new` and every allocation. `check_kdf_ceiling` is private (crypto.rs:149) so no caller can be handed the job. Both former call sites (`wrap`, `unwrap_master_key`) now carry only the floor / version gate; `wrap`'s comment at :434-436 names why the duplicate was removed. |
| 4 | A KDF floor binds on the write path; below default memory the accepted length rises 12 → 20 | ✓ VERIFIED | `check_kdf_floor` is called from `Keyfile::wrap` (crypto.rs:437), which is the sole constructor for both `create` and `rewrap` — so initialisation and password change are both bound, and reading is deliberately unbounded below so raising the floor cannot strand an existing bundle. `passphrase::check` (:179-181) returns `Rejected(WEAKENED_KDF)` for anything under 20 when `k.m_kib < KdfParams::default().m_kib`. The seams `create_with_floor` / `rewrap_with_floor` are `pub(crate)` (:383, :422) — an exported floor-as-argument would not be a floor. |
| 5 | Six documentation statements describing unimplemented behaviour were corrected | ✗ **FAILED** | The six named corrections did land — `passphrase.rs:24-38` and `docs/sync-format.md:653-662` now say plainly that the rule is a length rule and that a typed 20-char password is accepted, and the user-facing `WEAKENED_KDF` string (:104-109) matches the code. But a seventh statement of the same class survives, uncorrected, in the format spec: `docs/sync-format.md:390`. See Gaps. |

**Score:** 10/11 must-haves verified (6/6 roadmap SCs; 4/5 change claims).

### Invariant: no path can repeat a nonce under one key

Enumerated by call site rather than inferred from `Keys::seal`. `grep` over `src/` for `.encrypt(`,
`.decrypt(`, `XNonce`, `Nonce::` returns exactly six AEAD sites in `src/sync/`, matching the
independent auditor's count.

| Key | Site | Nonce source | Reuse reachable? |
|-----|------|--------------|------------------|
| KEK (per keyfile) | `Keyfile::wrap` crypto.rs:447 / `unwrap_master_key` :480 | fresh `fill()` 24 bytes per wrap, alongside a fresh 16-byte salt | No — random 192-bit nonce, and every wrap re-derives a KEK under a fresh salt |
| `chunk_key` | `Keys::seal` crypto.rs:586 / `Keys::open` :650 | `derive_key(CTX_NONCE, keyed_hash(name_key, message))[..24]` | No — injective in the sealed message; see the argument below |
| `root_key` | `Keys::seal_root` crypto.rs:679 / `open_root` :714 | fresh `fill()` 24 bytes per seal | No — XChaCha's 192-bit nonce makes random generation safe with no counter accounting |

The `chunk_key` argument, made structurally rather than by convention: the only production caller of
`Keys::seal` is `chunk::seal_chunk` (chunk.rs:166) — `Keys::seal` is `pub(crate)`, and the sole other
in-crate caller is a `#[cfg(test)]` case at chunk.rs:358. `seal_chunk` computes
`id = chunk_id(data)` and `message = frame(data)`. Framing is injective (`unframe(frame(d)) == d`,
chunk.rs:109-146), so the message determines `data`, which determines `id`. **The same message can
therefore never be sealed under two different ids**, which closes the subtler leg: two Poly1305 tags
under one `(key, nonce)` differing only in AAD would also give a solvable polynomial for `r`, and
that shape is unreachable. Two distinct messages get distinct nonces up to a BLAKE3 collision. Two
zstd builds framing one plaintext differently now get two nonces, which is precisely the F-1 fix.

`src/safe_storage.rs` also holds AEAD calls and is deterministic by design, but it is the
pre-existing Electron `safeStorage` compatibility path for Claude Desktop credentials, untouched by
this phase and outside the sync key hierarchy.

### Invariant: the id pins did not move

Extracted every hex literal from `tests/sync_vectors.rs` at commit `7237f7f` (the 1-07 pinning
commit, pre-remediation) and at HEAD, per pin.

| Pin | Kind | 7237f7f | HEAD | Verdict |
|-----|------|---------|------|---------|
| chunk id of `SHORT_PLAINTEXT` | id | `e18d3151…945cfe0b` | `e18d3151…945cfe0b` | **held** |
| multi-chunk manifest id + the three fixture ids | id | `f3a798c5…`, `188c0329…`, `a969848d…`, `e0eeabb6…` | identical | **held** |
| `name_key` subkey | key | `6da4903b…57c65278` | identical | **held** |
| KEK for the fixed `(password, salt, params)` | key | `23b85454…ddf6822c` | identical | **held** |
| sealed chunk ciphertext | ciphertext | 96 bytes | 120 bytes, re-pinned | moved — correct, the inline 24-byte nonce |
| pack address | ciphertext | `4c8e3c5a…` | `9b612506…` | moved — correct, every blob grew 24 bytes |

Dedup is intact: an id addresses the raw plaintext and is untouched by the nonce change, so no
future user re-uploads a chunk an existing bundle already holds. The two pins that moved are exactly
the two the format change had to move, and each carries a comment naming which kind of pin it is and
what a move means.

### Invariant: no equality version check anywhere

`check_version` (mod.rs:87-95) is `if found <= ceiling { Ok }` — the only version gate in the crate.
Every consumer routes through it: `Keyfile::unwrap_master_key` (crypto.rs:470),
`Root::open` via `probe_version` (model.rs:364), `Manifest::open_with_ceiling` /
`IndexObject::open` via `open_object` (:197, :269, :110), `pack::read_header` (:212). No `format ==`
or `format !=` comparison exists in `src/sync/`. The chunker is checked by set membership
(`check_chunker`, model.rs:81-93), not equality with the one this build writes. Version probing
reads only the `format` field before full deserialization, so a newer object with unknown required
fields refuses with the true reason rather than a missing-field complaint.

### Required Artifacts

| Artifact | Expected | Status | Details |
|---|---|---|---|
| `src/sync/crypto.rs` | key hierarchy, all six AEAD sites, KDF bounds | ✓ VERIFIED | 58 KB; ceiling in `derive_kek`, floor in `wrap`, seals `pub(crate)` where they must be |
| `src/sync/chunk.rs` | 256 KiB fixed chunker, frame, zstd, seal | ✓ VERIFIED | Bounds-checked `unframe`, power-of-two padding capped at `CHUNK_SIZE` with `.max(body)` so incompressible data is never truncated |
| `src/sync/pack.rs` | blobs, sealed header, trailer id, sharded name | ✓ VERIFIED | `PACK_TARGET = 32 MiB` (the CAL-1 fallback), header sealed via `seal_chunk` so it inherits the same nonce rule |
| `src/sync/model.rs` | root, manifest, index object | ✓ VERIFIED | `Root` carries `repo_id` + `manifest_chunks`; clock injected, never read |
| `src/sync/passphrase.rs` | generate, strength gate, no argv/env | ✓ VERIFIED | Rule coupled to `KdfParams`; module docs state the length-vs-entropy limit rather than overclaiming |
| `src/sync/anchor.rs` | monotonic rollback anchor, mode 0600, config dir | ✓ VERIFIED | Atomic write via the project's existing `cache::atomic_write`; parse failure is an error, not a reset |
| `docs/sync-format.md` | the on-disk format, both calibrations, accepted leakage | ⚠️ **INACCURATE** | 723 lines, §1-§9 all present and otherwise matched line-by-line against the code; §5 line 390 records a nonce rule the code does not implement — see Gaps |
| `tests/sync_adversarial.rs` | nine attacks, each asserting zero plaintext | ✓ VERIFIED | 13 tests, all pass with `$HOME` unset |
| `tests/sync_vectors.rs` | pinned known-answer vectors | ✓ VERIFIED | 13 tests; RFC 9106 Argon2id vector guards the crate itself |

### Key Link Verification

| From | To | Via | Status |
|---|---|---|---|
| `src/lib.rs:40` | `src/sync/` | `pub mod sync` | ✓ WIRED (declared; no production consumer by design — deferred to Phases 3/4/6) |
| `chunk::seal_chunk` | `Keys::seal` | chunk.rs:166 | ✓ WIRED — sole production caller, which is what makes the nonce↔message argument structural |
| `pack::write_header` | `chunk::seal_chunk` | pack.rs:170 | ✓ WIRED — the header inherits the chunk nonce rule rather than inventing one |
| `Root::seal` | `Keys::seal_root` | model.rs:348, passing `&self.repo_id` | ✓ WIRED |
| `Root::open` | `Keys::open_root` | model.rs:363, passing the caller's `expect_repo_id` | ✓ WIRED — the expectation is local, never the remote's claim |
| `Keyfile::create` / `rewrap` | `check_kdf_floor` | crypto.rs:437 via the single `wrap` | ✓ WIRED — both write paths bound by one guard |
| every `derive_kek` caller | `check_kdf_ceiling` | crypto.rs:176 | ✓ WIRED — inside the shared function, private, unforgettable |
| `anchor::accept` | persistence | — | ✓ CORRECTLY ABSENT — the check decides, the caller persists, and only after a snapshot verifies |

### Behavioural Spot-Checks

| Behaviour | Command | Result | Status |
|---|---|---|---|
| Full suite on the integrated tree | `cargo test` | 1104 passed, 0 failed, 11 ignored | ✓ PASS |
| Frontend contract suites | `make test` | exit 0; GNOME, KDE, Omarchy all pass | ✓ PASS |
| Lint gate | `cargo clippy --all-targets -- -D warnings` | exit 0 | ✓ PASS |
| Format gate | `cargo fmt --check` | exit 0 | ✓ PASS |
| Hermeticity | `env -u HOME -u XDG_CACHE_HOME -u XDG_CONFIG_HOME cargo test --test sync_adversarial --test sync_vectors` | 13 + 13 pass | ✓ PASS |
| Chunk-nonce invariant | `cargo test --test sync_vectors the_sealed_chunk_ciphertext_is_pinned_and_is_zstd_sensitive` | 1 passed | ✓ PASS |
| Root repo-swap invariant | `cargo test --test sync_vectors the_root_subkey_is_pinned` | 1 passed | ✓ PASS |
| Rollback refusal | `cargo test --test sync_adversarial attack_9` | 1 passed | ✓ PASS |
| Unused-dependency gate | `cargo machete` | tool not installed | ? SKIP — substituted by per-crate call-site check |

### Requirements Coverage

| Requirement | Description | Status | Evidence |
|---|---|---|---|
| CRYPTO-01 | Bundle encrypted client-side under a password-derived key; remote never sees plaintext or password | ✓ SATISFIED | Argon2id → KEK → wrapped random master key → three BLAKE3 subkeys. `no_fixture_plaintext_and_no_file_path_survives_into_the_pack_bytes` proves it at the byte level. |
| CRYPTO-02 | Memory-hard KDF with parameters stored alongside the data, raisable without breaking bundles | ✓ SATISFIED | `KdfDoc` in the keyfile, AAD-bound; readers use the stored params (`KdfDoc::params`), never the compiled default; `check_version` is at-or-below so a v2 client reads v1. |
| CRYPTO-03 | Wrong password fails cleanly, never partial or garbage output | ✓ SATISFIED | One message for wrong-password and KDF-downgrade alike; `reassemble` returns no partial buffer on any failure (chunk.rs:204-218) and `Zeroizing` wipes what it had collected. |
| CRYPTO-05 | Tampering, reordering, truncation, rollback detected and refused | ✓ SATISFIED | attacks 3-9. Order lives in the sealed manifest; rollback in the anchor; repo swap now in the root AAD rather than in local state. |
| CRYPTO-06 | Strength enforced at set time with the offline risk explained in plain language | ✓ SATISFIED | `check` + `OFFLINE_ATTACK_NOTE` + `NO_RECOVERY`, coupled to the KDF cost. Enforcement at a *surface* is Phase 3 (deferred). |
| CRYPTO-07 | Key material zeroized, never in argv, env, logs, or error messages | ✓ SATISFIED | `Zeroizing` throughout; hand-written `Debug` on `Keys`; the AEAD's returned `Vec` explicitly `.zeroize()`d with the reallocation residue documented as accepted; `no_password_input_path_reads_the_process_environment`. Refusals name the chunk *id* — an address written in the clear in every pack trailer, keyed so it cannot be inverted — not key material. |

No orphaned requirements: REQUIREMENTS.md maps exactly CRYPTO-01/02/03/05/06/07 to Phase 1, and all six are claimed and covered. CRYPTO-04's primitive (`Keyfile::rewrap`) ships here but the requirement is correctly mapped to Phase 4.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|---|---|---|---|---|
| — | — | — | — | No `TBD`, `FIXME`, `XXX`, `HACK`, `PLACEHOLDER`, `todo!` or `unimplemented!` anywhere in `src/sync/`, the two sync test files, or the format doc. The debt-marker gate is clean. |
| `.planning/phases/01-encrypted-bundle-core/1-HUMAN-UAT.md` | 3 | stale count — "1095 tests pass"; the tree is at 1104 | ℹ️ INFO | Planning artifact, not a code or format contract. Worth refreshing when the gap above is closed. |

### Gaps Summary

One gap, and it is in prose rather than in code.

`docs/sync-format.md` §5, justifying the root's random nonce, says: *"every other object's nonce is
derived from its content address because identical plaintext must seal identically or dedup dies."*
The conclusion is right and the mechanism is wrong. This document defines "content address" as the
chunk id (§3's heading, §4's "content-addressed pack names"), and §3 states in bold that the nonce
is derived **"from the bytes actually encrypted, never from the id"** — because deriving it from the
id is what let two zstd builds seal two distinct messages under one `(chunk_key, nonce, aad)`. The
sentence is byte-identical to the version at commit `ed56b39`, the audit commit that opened F-1, and
it survived both remediation rounds untouched.

Nothing is decrypted wrongly and no user's data is at risk: the shipped code is correct and every
pin agrees with it. What is at risk is the next implementation. §5 is the section a Phase-2 author
reads when they add an object to this key hierarchy, and it tells them to derive a nonce from a
content address. §3 contradicts it two pages earlier, so the likely outcome is confusion rather than
a reintroduced vulnerability — but a format spec that contradicts itself on its one nonce rule is
not a spec the milestone should inherit, and the phase has already escalated a finding (F-4b) for
this exact defect class. A secondary clause in the same paragraph — "hence the fixed AAD literal" —
also predates F-3, which made the root AAD `literal ‖ repo_id`, as the same section's own opening
paragraph correctly states.

Fix is a two-sentence edit to `docs/sync-format.md:389-394`. No code, no pin, no ciphertext, no
format version moves.

---

_Verified: 2026-08-19 (re-verification against the final tree)_
_Verifier: Claude (gsd-verifier)_
