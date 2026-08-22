---
phase: 01-encrypted-bundle-core
plan: 06
type: execute
wave: 4
depends_on: [1-01, 1-02, 1-03, 1-04, 1-05]
files_modified:
  - tests/sync_adversarial.rs
  - src/sync/crypto.rs
  - src/sync/chunk.rs
  - src/sync/pack.rs
  - src/sync/model.rs
  - src/sync/anchor.rs
autonomous: true
requirements: [CRYPTO-03, CRYPTO-05, CRYPTO-07]
must_haves:
  truths:
    - "Each of the nine attacks fails, and each failure yields zero bytes of plaintext."
    - "A failure message names the operation and never a key, password, or plaintext fragment."
    - "The nine failure messages are pairwise distinct, so an operator can tell which attack occurred."
    - "The full stack round-trips a multi-megabyte fixture byte-exactly, so the refusals are not a broken build refusing everything."
  artifacts:
    - tests/sync_adversarial.rs exercising the whole stack against an adversary who controls the remote
  key_links:
    - "The happy-path round-trip runs first in the same file — without it, nine passing refusals prove nothing"
    - "This plan holds the only edit authority over src/sync/ after wave 3, and 1-07 pins its vectors after this plan has finished"
---

<objective>
The adversarial suite. **D-04 (D4)** makes these part of done, not a nice-to-have: the research ran
them against its prototype, and the format is not accepted into the project until they exist as
project tests.

The threat model is an attacker who obtains the complete repository and may also *serve* a modified
one. A round-trip test proves none of these properties — it stays self-consistent under any
wrong-but-stable transform. **Nine** attacks do.

The count is nine, and it is stated here, in `<behavior>`, and in `<done>` as the same number
deliberately: the seven that the research prototype ran, plus the truncated-manifest and
reordered-manifest cases that CRYPTO-05 names explicitly.

Purpose: prove CRYPTO-03 and CRYPTO-05 rather than assert them.
Output: `tests/sync_adversarial.rs`.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/01-encrypted-bundle-core/1-CONTEXT.md
@.planning/research/encryption.md
@.planning/phases/01-encrypted-bundle-core/1-01-SUMMARY.md
@.planning/phases/01-encrypted-bundle-core/1-02-SUMMARY.md
@.planning/phases/01-encrypted-bundle-core/1-03-SUMMARY.md
@.planning/phases/01-encrypted-bundle-core/1-04-SUMMARY.md
@.planning/phases/01-encrypted-bundle-core/1-05-SUMMARY.md
@src/safe_storage.rs
@CLAUDE.md
</context>

<tasks>

<task type="auto" tdd="true">
  <name>Task 1: The full-stack fixture and its byte-exact round trip</name>
  <files>tests/sync_adversarial.rs</files>
  <behavior>
    - A deterministic multi-megabyte fixture — spanning several full chunks plus a partial tail, with both compressible and incompressible regions — chunks, seals, packs, manifests, roots, and comes back byte-exactly.
    - Running the whole pipeline twice over the same fixture produces identical pack bytes and identical chunk ids.
    - The fixture's plaintext appears nowhere in the pack bytes.
  </behavior>
  <action>
Create `tests/sync_adversarial.rs`.

Build a shared harness at the top of the file: a `fn fixture(len: usize) -> Vec<u8>` producing
deterministic bytes from a small counter-based generator — no randomness, so a failure is
reproducible — with an obviously compressible run and an incompressible run so both zstd paths are
exercised. A `fn bundle(keys: &Keys, data: &[u8]) -> (Vec<u8>, Manifest, Vec<u8>)` returning the pack
bytes, the manifest, and the sealed root, plus a matching `fn restore(...)` walking root to manifest
to chunk ids to chunks and reassembling. Every test in this file drives that pair.

All tests use the cheap KDF parameters. A 1.5-second key derivation inside `cargo test` would break
the AUR `check()` budget, and the AUR build runs this suite on other people's machines.

Write the happy-path assertions first. They are load-bearing: nine passing refusals mean nothing if
the pipeline refuses everything, so the round trip is what proves the refusals are selective.
  </action>
  <verify>
    <automated>cargo test --test sync_adversarial</automated>
  </verify>
  <done>The multi-megabyte fixture round-trips byte-exactly and the pipeline is deterministic across two runs.</done>
</task>

<task type="auto" tdd="true">
  <name>Task 2: Nine attacks, nine refusals, zero plaintext</name>
  <files>tests/sync_adversarial.rs, src/sync/crypto.rs, src/sync/chunk.rs, src/sync/pack.rs, src/sync/model.rs, src/sync/anchor.rs</files>
  <behavior>
    - Attack 1, wrong password: opening the keyfile under a different password errors, and no subkey is produced.
    - Attack 2, downgraded KDF parameters: rewriting `m_kib` in the serialized keyfile to a cheap value makes it fail to open even under the correct password.
    - Attack 3, chunk swap forward: serving chunk B's ciphertext under chunk A's id fails.
    - Attack 4, chunk swap reverse: serving chunk A's ciphertext under chunk B's id fails — both directions, because a one-directional test can pass by accident of length.
    - Attack 5, flipped bit: inverting one bit anywhere in a chunk's ciphertext makes it fail to open.
    - Attack 6, truncated pack: removing the final byte, and separately removing the last kilobyte, both fail at header read and return zero entries.
    - Attack 7, truncated manifest: cutting bytes from a sealed manifest makes it fail to open and return zero entries — asserted on the entry count, not merely on `is_err`, so a manifest that opened with a shorter file list could not pass.
    - Attack 8, reordered manifest: transposing two `ChunkId`s inside a file entry of a sealed manifest makes it fail to open and return zero entries. CRYPTO-05 names reordering explicitly and nothing else in the suite covers it.
    - Attack 9, rolled-back snapshot: a root whose counter is below the anchor's is refused, and passing the allow-rollback flag accepts it.
    - Every one of the nine returns zero bytes of plaintext — asserted on the returned value, not merely on `is_err`.
    - Every failure message contains none of: the password, any subkey byte, any fixture plaintext fragment.
    - The nine failure messages are pairwise distinct.
  </behavior>
  <action>
Add the nine attacks to `tests/sync_adversarial.rs`, one `#[test]` per attack, each named for what it
does rather than for what it asserts. Nine tests, matching the nine `<behavior>` attack lines exactly
— do not stop at seven.

Two assertions matter more than the refusal itself and must appear in every case. First, **zero
plaintext**: check the returned value, not just that the call errored — a function that errors *and*
writes into an out-parameter has still leaked. For the two manifest attacks that means asserting the
returned entry count is zero rather than that an error occurred, because "opened with fewer entries"
is exactly the failure mode being hunted. Second, **no secret in the message**: format the error to a
string and assert it contains no fragment of the password, no byte of any subkey rendered in hex, and
no run of fixture plaintext. That second assertion is CRYPTO-07 made testable, following the existing
precedent in `src/error.rs` where `user_message_does_not_expose_authentication_response_bodies`
asserts on absence rather than on shape.

For attack 2, mutate the *serialized* keyfile JSON, then deserialize and open — that is the actual
attack, and mutating an in-memory struct would not exercise the associated-data binding.

For attack 8, transpose the ids inside the manifest's serialized form before sealing is *not* the
attack; the attack is transposing them inside an already-sealed manifest's ciphertext region, or
re-sealing a reordered manifest under the original manifest's id. Do whichever expresses "the
attacker serves a manifest whose chunk order differs from the one the root names", and say in a
comment which you chose.

For attack 9, drive `anchor::accept` with a local anchor and a lower remote counter, then repeat with
the allow-rollback flag set. Assert the refusal message names the escape.

Add a final test collecting all nine error messages and asserting they are pairwise distinct. A
single opaque "decryption failed" for every case is technically safe and operationally useless, and
CRYPTO-03 asks for unambiguous.

If an attack does **not** fail, that is a real defect in the module under attack. Fix it in the
smallest possible edit to the owning `src/sync/` file, and record both the defect and the fix in the
summary — waves 2 and 3 have already merged, so this plan holds the only edit authority over those
files, and 1-07 runs after this plan for exactly that reason.
  </action>
  <verify>
    <automated>cargo test --test sync_adversarial</automated>
  </verify>
  <done>All nine attacks fail, each with zero plaintext returned, a secret-free message, and a message distinct from the other eight.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|---|---|
| a fully attacker-controlled remote → the whole stack | This suite *is* the boundary test |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|---|---|---|---|---|---|
| T-06-01 | Elevation of privilege | offline password guessing | critical | mitigate | The wrong-password case asserts a clean failure with no partial output, so an attacker gains no oracle beyond pass or fail |
| T-06-02 | Tampering | swap, bit flip, truncation | critical | mitigate | Six explicit attack tests, each asserting zero plaintext returned |
| T-06-03 | Tampering | reordering, which CRYPTO-05 names and no round-trip test detects | high | mitigate | Attack 8 asserts a transposed manifest fails to open and yields zero entries |
| T-06-04 | Tampering | rollback replay | high | mitigate | Attack 9, both refused and explicitly allowed |
| T-06-05 | Information disclosure | secrets in error messages | high | mitigate | Every failure message asserted free of the password, subkey bytes, and plaintext fragments |
| T-06-06 | Denial of service | a 1 GiB KDF inside the AUR `check()` | medium | mitigate | Every test uses the cheap parameter seam |
</threat_model>

<verification>
- `cargo test --test sync_adversarial` passes with `$HOME` unset and no network.
- The suite completes well inside the AUR `check()` budget — no test derives at production parameters.
- Nine `#[test]` functions cover the nine named attacks.
</verification>

<success_criteria>
1. A multi-megabyte fixture round-trips byte-exactly, and sealing it twice is byte-identical.
2. Wrong password, downgraded KDF parameters, chunk swap in both directions, a flipped bit, a truncated pack, a truncated manifest, a reordered manifest, and a rolled-back snapshot each fail — nine cases.
3. Every failure yields zero bytes of plaintext, asserted on the value, and zero entries for the two manifest cases.
4. Every failure message is free of secrets and distinct from the other eight.
</success_criteria>

<output>
Create `.planning/phases/01-encrypted-bundle-core/1-06-SUMMARY.md` when done. If any attack initially
succeeded, record the defect, the module, and the fix — that is the most important thing this phase
can learn, and 1-07 pins its vectors against the post-fix behaviour.
</output>
