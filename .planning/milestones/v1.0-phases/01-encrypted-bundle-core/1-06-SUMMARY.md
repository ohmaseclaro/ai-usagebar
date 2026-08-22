---
phase: 01-encrypted-bundle-core
plan: 06
subsystem: sync
tags: [adversarial, crypto, threat-model, error-messages, rollback, ordering-integrity, secret-hygiene]

# Dependency graph
requires: [1-01, 1-02, 1-03, 1-04, 1-05, 1-09]
provides:
  - "tests/sync_adversarial.rs — the whole stack driven against an adversary who controls the remote"
  - "A `refused` funnel every attack passes through: zero recovered on the *value*, a secret-free message, and the message returned for pairwise comparison"
  - "Chunk-layer authentication refusals now name the chunk, so five formerly identical failures are five distinguishable ones"
affects: [1-07, 1-08, phase-2, phase-3, phase-5]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "One shared `OnceLock` bundle per test binary: deterministic sealing means sharing changes no outcome, and rebuilding a multi-megabyte fixture per test would put the file an order of magnitude over the AUR check() budget"
    - "Attack outcomes are mapped to a *count* before assertion, so a failing security assertion cannot print a user's plaintext into a CI log"
    - "Eight of nine refusals are pinned by whole-message equality — strictly stronger than an absence check, because an equal string cannot be hiding a key"

key-files:
  created:
    - tests/sync_adversarial.rs
  modified:
    - src/sync/crypto.rs

key-decisions:
  - "`Keys::open`'s refusal now names the chunk id. Five of the nine attacks produced the byte-identical string 'chunk failed authentication' — the exact message collapse CRYPTO-03's 'unambiguous' rules out. The id is safe to say: it is already in the clear in every pack trailer and index object, and it is a keyed hash, so it cannot be inverted or confirmed without name_key"
  - "Attacks 1 and 2 share a message *by cryptographic necessity*, and the suite pins that rather than papering over it: a wrong password and an in-range m_kib downgrade both produce a wrong KEK, and a message that distinguished them would be an oracle. Attack 2 therefore contributes its below-Argon2-floor leg to the distinctness set"
  - "Attack 6's representative message is the deterministic below-trailer cut. The plan's final-byte and final-kilobyte cuts are asserted too, but *which* pack-reader refusal they hit depends on keyed-hash bytes that differ every run, and a suite that is the phase's proof must not flake"
  - "Attack 8 asserts the guarantee (the ordered list sits inside the root's sealed plaintext, so transposing it needs the key) before the consequence (a followed reordering fails to parse), never the consequence alone"
  - "Attack 2 seals one keyfile at m_kib = 64 rather than the cheap seam's 8, because Argon2's own floor is m = 8·p and a keyfile written at the seam has nothing left to downgrade to. 64 KiB at t = 1 is still microseconds"

patterns-established:
  - "An adversarial test asserts on the returned value, never on is_err(): a call that errors while handing back a partial buffer has still leaked, and is_err() would call that a pass"
  - "Failure messages are part of the security contract — distinctness is testable, and a refactor that collapses two failure modes is caught by a pairwise assertion rather than found in production"

requirements-completed: [CRYPTO-03, CRYPTO-05, CRYPTO-07]

coverage:
  - id: D1
    description: "A 2.1 MiB deterministic fixture — eight full chunks plus a partial tail, one compressible half and one incompressible — chunks, seals, packs, manifests, roots and comes back byte-exactly through the whole stack"
    requirement: CRYPTO-03
    verification:
      - kind: integration
        ref: "cargo test --test sync_adversarial — the_full_stack_round_trips_a_multi_megabyte_fixture_byte_exactly"
        status: pass
    human_judgment: false
  - id: D2
    description: "Running the whole pipeline twice over one fixture produces identical pack bytes and identical chunk ids, while the root differs by design"
    requirement: CRYPTO-03
    verification:
      - kind: integration
        ref: "cargo test --test sync_adversarial — sealing_one_fixture_twice_produces_identical_packs_and_identical_chunk_ids"
        status: pass
    human_judgment: false
  - id: D3
    description: "No run of fixture plaintext, and no file path from the manifest travelling in the same pack, survives into the pack bytes"
    requirement: CRYPTO-03
    verification:
      - kind: integration
        ref: "cargo test --test sync_adversarial — no_fixture_plaintext_and_no_file_path_survives_into_the_pack_bytes"
        status: pass
    human_judgment: false
  - id: D4
    description: "Attack 1 — a wrong password produces no Keys value at all and no plaintext out of the whole stack, while the right password still restores byte-exactly"
    requirement: CRYPTO-03
    verification:
      - kind: integration
        ref: "cargo test --test sync_adversarial — attack_1_a_wrong_password_yields_no_subkey_and_no_plaintext"
        status: pass
    human_judgment: false
  - id: D5
    description: "Attack 2 — m_kib rewritten in the *serialized* keyfile fails to open even under the correct password (the AAD binding), and a rewrite below Argon2's floor is refused by name before any derivation"
    requirement: CRYPTO-03
    verification:
      - kind: integration
        ref: "cargo test --test sync_adversarial — attack_2_downgrading_the_kdf_parameters_in_transit_opens_nothing"
        status: pass
    human_judgment: false
  - id: D6
    description: "Attacks 3 and 4 — one chunk's ciphertext served under another chunk's id fails in both directions, with the pair chosen so their ciphertext lengths differ"
    requirement: CRYPTO-05
    verification:
      - kind: integration
        ref: "cargo test --test sync_adversarial — attack_3_serving_one_chunks_ciphertext_under_an_earlier_chunks_id_fails, attack_4_serving_one_chunks_ciphertext_under_a_later_chunks_id_fails"
        status: pass
    human_judgment: false
  - id: D7
    description: "Attack 5 — one inverted bit at the first, middle and last byte of a chunk's ciphertext each abort the restore with zero bytes recovered"
    requirement: CRYPTO-05
    verification:
      - kind: integration
        ref: "cargo test --test sync_adversarial — attack_5_flipping_one_bit_of_a_chunks_ciphertext_fails"
        status: pass
    human_judgment: false
  - id: D8
    description: "Attack 6 — removing the final byte, the final kilobyte, and everything below the trailer all fail inside read_header and return zero entries"
    requirement: CRYPTO-05
    verification:
      - kind: integration
        ref: "cargo test --test sync_adversarial — attack_6_a_truncated_pack_fails_at_header_read_with_no_entry_returned"
        status: pass
    human_judgment: false
  - id: D9
    description: "Attack 7 — a sealed manifest cut short returns zero file entries, asserted on the entry count rather than on is_err(), so 'opened with a shorter file list' could not pass"
    requirement: CRYPTO-05
    verification:
      - kind: integration
        ref: "cargo test --test sync_adversarial — attack_7_a_truncated_manifest_returns_no_file_entry"
        status: pass
    human_judgment: false
  - id: D10
    description: "Attack 8 — manifest_chunks comes back exactly as written from the root's sealed plaintext, a transposed list re-sealed by a stranger is refused outright, transposed ids inside a file entry re-seal to a different ChunkId and fail when served under the honest one, and a followed reordering yields zero entries"
    requirement: CRYPTO-05
    verification:
      - kind: integration
        ref: "cargo test --test sync_adversarial — attack_8_transposed_chunk_ids_are_protected_by_the_sealed_order"
        status: pass
    human_judgment: false
  - id: D11
    description: "Attack 9 — an authentic root at a counter below the local anchor is refused with a message naming --allow-rollback, and the same restore with the flag returns the fixture byte-exactly"
    requirement: CRYPTO-05
    verification:
      - kind: integration
        ref: "cargo test --test sync_adversarial — attack_9_a_rolled_back_snapshot_is_refused_unless_explicitly_allowed"
        status: pass
    human_judgment: false
  - id: D12
    description: "Every one of the nine returns zero bytes / zero entries, asserted on the value; every message is free of the password, the keyfile's wrapped key and salt, and any run of fixture plaintext; and the nine messages are pairwise distinct"
    requirement: CRYPTO-07
    verification:
      - kind: integration
        ref: "cargo test --test sync_adversarial — the_nine_refusals_carry_nine_distinct_messages, plus the `refused` funnel inside all nine"
        status: pass
    human_judgment: false

# Metrics
duration: 55min
completed: 2026-08-19
status: complete
---

# Phase 1 Plan 06: The Adversarial Suite Summary

**Nine attacks, nine refusals, zero plaintext — and one real defect found and fixed.** Five of the
nine attacks produced the byte-identical message `chunk failed authentication`, so an operator could
not tell a swapped chunk from a flipped bit from a truncated manifest. `src/sync/crypto.rs` now names
the chunk in that refusal. **1-07 should pin the post-fix string.**

## ⚠ The `src/` change 1-07 must know about

`Keys::open` in `src/sync/crypto.rs`:

```rust
-  .map_err(|_| AppError::Other("chunk failed authentication".into()))
+  .map_err(|_| AppError::Other(format!("chunk {id} failed authentication")))
```

**The defect.** Every tampering attack that reaches that line is the same event to Poly1305: the tag
did not verify. Attacks 3, 4, 5, 7 and one leg of 8 therefore all produced one string. The behaviour
was correct — every attack was refused, and no attack ever leaked a byte — but CRYPTO-03 asks for
*unambiguous*, and five collapsed failure modes are the exact thing the distinctness requirement
exists to catch. It was caught by writing the tests, not by reading the code.

**Why naming the id is safe.** The id is already written in the clear in every pack trailer
(`pack.rs` puts it there deliberately, because `read_header` needs it before it can decrypt) and
listed in every index object and manifest. It is a *keyed* hash — `blake3::keyed_hash(name_key, …)` —
so nobody without `name_key` can invert it or confirm a guess against it. `crypto.rs` already
documents it as "an address, not a secret", which is why `ChunkId` derives `Debug`. CRYPTO-07 is
about keys, passwords and plaintext, none of which appears here.

**Blast radius.** One `map_err`. All 93 existing `sync::` unit tests pass unchanged; the only one
that asserted on this string (`chunk::tests::transposed_ciphertexts_abort_reassembly_rather_than_\
reordering_it`) matches the `failed authentication` substring.

## What the suite asserts, and why it is shaped that way

### Zero plaintext is asserted on the value, never on `is_err()`

Every attack funnels through one helper:

```rust
fn refused(outcome: Result<usize>, keyfile: &Keyfile, plaintext: &[u8]) -> String {
    assert_eq!(*outcome.as_ref().unwrap_or(&0), 0, "an attack recovered data instead of nothing");
    let message = outcome.expect_err("this attack must be refused").to_string();
    assert_secret_free(&message, keyfile, plaintext);
    message
}
```

`outcome` is a *count* — bytes recovered, files recovered, pack entries recovered. Two reasons.
Asserting the count is zero asserts on the returned **value**, which `is_err()` does not: a call that
errors while filling an out-parameter has still leaked, and `is_err()` would call that a pass. And a
count cannot print a user's plaintext into a CI log when the assertion fails.

The funnel is the point of the design: no case can quietly assert less than the others, because every
case goes through the same three checks.

### Secret-freedom is absence *plus* equality

`assert_secret_free` checks the message against the password, the wrong-password guess, the keyfile's
base64 wrapped master key, its salt, the fixture's planted `NEEDLE`, and four 32-byte windows spread
across both halves of the fixture — following `src/error.rs`'s
`user_message_does_not_expose_authentication_response_bodies`, which asserts on absence rather than
on shape.

On top of that, **eight of the nine pin their whole message by equality**. That is strictly stronger
than any absence check: a string equal to a known literal cannot be hiding a subkey, a nonce or a
plaintext fragment anywhere inside it. The ninth (rollback) is prose with two injected counters, so
it asserts its substrings plus a direct check that no 32-character run of hex digits appears — which
is what a rendered 32-byte key would look like.

### Attack 8 asserts the guarantee, then the consequence

Three legs, in this order:

1. **The guarantee.** `manifest_chunks` sits inside the root's *sealed plaintext*. Served under the
   original root ciphertext, the list comes back exactly as written; and a transposed list re-sealed
   by a stranger — an attacker holding a key he actually has — is refused outright
   (`snapshot root failed authentication`). That is "transposing the ids requires the key", stated as
   an assertion rather than as prose.
2. **Transposing ids inside a file entry.** Re-sealing a reordered manifest yields a *different*
   `ChunkId` (1-04's finding), so the root never points at it; the genuine attack is to serve those
   bytes under the id the root *does* name, and that leg errors with zero files returned.
3. **The consequence.** A reader that did follow a reordered list reassembles a buffer that will not
   parse (`manifest is malformed`). Asserted last and never alone — 1-09 was explicit that the parse
   failure is downstream of the guarantee, not the guarantee.

### The happy path is load-bearing

Three tests run before the nine. A 2.1 MiB deterministic fixture (eight full 256 KiB chunks plus a
4,096-byte tail, a compressible first half and an xorshift second half so both zstd paths are
exercised, with `NEEDLE` between them) round-trips byte-exactly; two runs produce identical pack
bytes and identical chunk ids while the root differs by design; and neither fixture plaintext nor a
manifest file path survives into the pack. Without these, nine passing refusals would be consistent
with a pipeline that refuses everything.

## Deviations from the plan

1. **The nine messages are pairwise distinct, but two of the nine attacks share a failure by
   cryptographic necessity — and the suite pins that instead of hiding it.** A wrong password and an
   in-range `m_kib` downgrade both yield a wrong KEK, and *nothing can tell them apart*: any message
   that distinguished them would be an oracle, which is why `crypto.rs` gives both the same string on
   purpose. Attack 2 asserts that collision explicitly (`assert_eq!(shared, "wrong password or
   corrupted keyfile")`) so a future "helpful" split is caught rather than shipped, and then
   contributes its *other* leg — a downgrade below Argon2's own `m = 8·p` floor, refused by name
   before any derivation — to the distinctness set. Nine distinct messages, and the one forced
   collision is documented in a test rather than in a comment.

   Attacks 3 vs 4 vs 5 vs 7 were the same problem and *were* fixable, which is what motivated the
   `crypto.rs` change above.

2. **Attack 6's representative message is the below-trailer cut, not the final-byte cut.** The plan
   names the final byte and the final kilobyte, and both are asserted (refused, zero entries,
   secret-free message). But *which* of the pack reader's refusals they hit depends on the bytes the
   truncation exposes as the trailing `u32` length, and those bytes are a keyed hash — different on
   every run. Roughly one run in 1,800 would take the other branch. Only the refusal itself is
   deterministic, so only that is asserted for those two, and the distinctness test collects the
   deterministic third cut. A suite that is the phase's proof must not flake.

3. **Attack 2 seals one keyfile at `m_kib = 64` rather than at the cheap seam's 8.** Argon2's floor
   is `m = 8·p`, so a keyfile written at the seam has nothing valid left to downgrade to and the
   AAD-binding leg could not exist. 64 KiB at `t = 1` is still microseconds. Every other derivation
   in the file uses `{ m_kib: 8, t: 1, p: 1 }`.

4. **One `OnceLock` bundle shared across the binary.** The first draft built a fresh 2.1 MiB bundle
   per test and ran 19.6 s single-threaded: `zstd` is a C dependency and the `dev` profile compiles
   it unoptimised, so ~40 multi-megabyte zstd passes dominated. Sharing changes no outcome — the
   pipeline is deterministic and no test mutates the shared state, each cloning what it tampers with
   — and brings it to **3.2 s single-threaded / 1.3 s parallel**. Attacks needing a different key
   hierarchy (attack 2) still build their own.

5. **A tenth `#[test]`, and nine plain functions behind them.** Each attack is a plain
   `fn attack_N_…() -> String` that performs its own assertions and returns its message; a thin
   `#[test]` wrapper gives each one its own name and its own failure attribution, and
   `the_nine_refusals_carry_nine_distinct_messages` calls all nine to compare them. Thirteen tests
   total: three round-trip, nine attacks, one distinctness.

## Test Approach

`cargo test --test sync_adversarial` — 13 tests, 1.34 s parallel, 3.22 s with `--test-threads=1`.

Hermetic, verified rather than asserted: the binary passes with `HOME`, `XDG_CONFIG_HOME` and
`XDG_CACHE_HOME` unset. No network, no Keychain, no clock (`created_at` is the fixed literal
`2026-08-19T12:00:00Z`), no randomness in the fixture (a seeded xorshift64, so a failure is
reproducible from this file alone). The only randomness is the OS CSPRNG inside `Keyfile::create` and
`seal_root`, which is the production behaviour under test.

`cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` are clean. `cargo test --lib
sync::` — 93 tests, unchanged and passing after the `crypto.rs` edit. Per the plan's scope: no full
`cargo test`, no `make test`.

**Evidence the suite has teeth:** before the `crypto.rs` fix, exactly six of the thirteen failed
(attacks 3, 4, 5, 7, 8 and the distinctness test) while the other seven passed — granular detection,
not an all-or-nothing gate.

## Threat Flags

None new. Two residual gaps, both inherent and both already documented upstream, restated because
this is the file that would have found them if they were fixable here:

- **A wrong password and an in-range KDF downgrade are indistinguishable** (attack 2, leg 1). Closing
  it would need local state recording the parameters last seen for a bundle — the same shape as the
  rollback anchor. That is a format/API change with Phase 2 and Phase 4 ripples and no behavioural
  gain (both cases already refuse); it is recorded here as an option, not taken.
- **First contact is trust-on-first-use** (`anchor::accept` with `local == None`). Attack 9 exercises
  the anchored path only, because there is nothing to compare against on a brand-new machine.
  `anchor.rs` documents this as an accepted residual gap.

## User Setup Required

None — pure and offline.

## Next Phase Readiness

**Ready.**

- **1-07** must pin `chunk {id} failed authentication`, not `chunk failed authentication`. That is
  the only observable change this plan made to `src/`. Everything else it pins is untouched:
  deterministic chunk sealing, the keyed id, the AAD binding, the version ceilings.
- **1-08** can document the read path exactly as `restore_gated` walks it: root → anchor gate →
  `manifest_chunks` → manifest → file chunk list → bytes.
- **Phase 2** inherits a `refused`-style funnel worth reusing: any new failure path it adds should
  assert on the returned value and add its message to a distinctness check, or the guarantee this
  plan established decays the first time a new error string is introduced.
- **Phase 3** (network) is where the "attacker serves a modified repository" premise stops being
  hypothetical. Every refusal here is offline; the transport must not add a path that reads an id
  list or a length before its container has authenticated.

## Self-Check: PASSED

- Nine `#[test]` functions cover the nine named attacks, plus three round-trip tests and one
  distinctness test.
- Every attack asserts zero recovered on the returned value, not on `is_err()`.
- Every failure message is asserted free of the password, the keyfile's secrets and fixture
  plaintext; eight are pinned by whole-message equality.
- The nine messages are pairwise distinct.
- The suite runs in 1.3 s parallel with `$HOME` unset, entirely on the cheap KDF seam.
