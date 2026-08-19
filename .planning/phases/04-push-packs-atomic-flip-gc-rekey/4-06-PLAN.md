---
phase: 04-push-packs-atomic-flip-gc-rekey
plan: 06
type: execute
wave: 2
depends_on: ["4-01"]
files_modified:
  - src/sync/push/rekey.rs
autonomous: true
requirements: [CRYPTO-04]
must_haves:
  truths:
    - "`sync rekey` under a new password unwraps the **same** master key and rewrites only the keyfile — not one pack byte moves (CRYPTO-04)."
    - "The new keyfile is uploaded and the pointer flipped to it **before** the old asset is deleted, so an interruption never leaves a bundle with no reachable keyfile."
    - "The old keyfile asset is **verifiably** gone: the delete is followed by a re-list that confirms its absence, and a failure to confirm is reported as a failure, not as success (D5)."
    - "A wrong old password fails before anything is uploaded, with Phase 1's single indistinguishable message."
    - "The local keyfile is replaced atomically at mode 0600, and only after the remote flip succeeded."
    - "The command prints plainly that this is not revocation: anyone holding an old keyfile can still unwrap it with the old password forever."
    - "Neither password, nor the master key, nor any keyfile byte appears in any rendered line."
  artifacts:
    - src/sync/push/rekey.rs — the rewrap, the ordered upload/flip/delete/confirm, and the honest message
  key_links:
    - "`Keyfile::rewrap` is Phase 1's and is called, never re-implemented — the data subkeys must not change or every pack becomes unreadable"
    - "The keyfile asset name is a content address, so the new and old keyfiles coexist for the instant between flip and delete"
    - "`pointer::commit` carries the new `keyfile` name; 4-04's merge rule is what stops a concurrent ordinary push republishing the old one"
---

<objective>
Let the user change the sync password without re-uploading their bundle — and make the old wrapper
actually go away rather than comfortingly appear to.

Implements **CRYPTO-04** and **D5**: rewrap the same master key under a new KEK, upload the new
keyfile, flip the pointer to it, then delete the old asset and confirm the deletion. Data packs
are untouched, which is what "without re-uploading the entire bundle" means.

Purpose: choosing Release assets over git objects was justified partly by this — an asset delete
removes the bytes, whereas a git object survives in history and makes "password change" a lie. The
payoff only exists if the deletion actually happens and is checked.
Output: `rekey::run` and the `sync rekey` body.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/04-push-packs-atomic-flip-gc-rekey/4-CONTEXT.md
@.planning/phases/04-push-packs-atomic-flip-gc-rekey/4-01-SUMMARY.md
@docs/sync-format.md
@CLAUDE.md
@src/sync/crypto.rs
@src/sync/passphrase.rs
@src/sync/push/mod.rs
@src/sync/github/write.rs
</context>

<tasks>

<task type="auto" tdd="true">
  <name>Task 1: Rewrap, publish, then verifiably destroy the old wrapper</name>
  <files>src/sync/push/rekey.rs</files>
  <behavior>
    - A keyfile rewrapped under a new password opens to the same three subkeys as the original opened to under the old one — asserted by sealing a chunk before and after and comparing the ciphertext byte for byte.
    - A wrong old password fails before any request is issued, with the same message a corrupted keyfile gets.
    - A new password under Phase 1's floor is refused by Phase 1's own gate; the floor is not re-implemented and not lowered.
    - The request order against a mock is: upload the new keyfile, flip the pointer, delete the old asset, re-list to confirm — and a test asserts that order, not just the set.
    - A run interrupted after the upload but before the flip leaves the pointer naming the **old** keyfile, which still opens under the old password.
    - A delete that succeeds but whose confirming re-list still shows the old asset returns an error naming the asset and telling the user it survived.
    - No pack asset is uploaded, deleted, or downloaded at any point.
    - The local keyfile file is replaced only after the flip returns, atomically, at mode 0600.
    - The rendered output contains neither password, nor any keyfile byte, nor an eight-character prefix of either.
  </behavior>
  <action>
Fill `rekey::run`, whose signature plan 4-01 froze, taking the context, the old password, the new
password, and the release id.

**Rewrap first, offline.** Read the local keyfile, and call Phase 1's
`Keyfile::rewrap(old_pw, new_pw, params)`. Do not re-derive, re-generate, or re-implement any part
of it: `rewrap` unwraps the existing master key and rewraps *the same* 32 bytes under a KEK from a
fresh salt. Generating a new master key here would silently orphan every pack on the remote, and
it would look like success. A wrong old password fails here, before a single request, with Phase 1's
single indistinguishable message — there is nothing useful to tell apart and nothing an attacker
should learn from the difference. Apply Phase 1's strength gate to the new password through
`sync::passphrase`; do not restate the floor and do not lower it.

**Then the ordered remote sequence, and the order is the whole safety property.**

1. Serialize the new keyfile, content-address it, and `upload_asset` it under
   `push::keyfile_asset_name`. The name is a content address, so old and new coexist — which is
   exactly what makes step 3 safe to do last.
2. `pointer::commit` with a rebuild closure that changes only the `keyfile` field, leaving every
   snapshot record untouched. Nothing about the bundle's contents changes in a rekey.
3. Only now `delete_asset` the old keyfile.
4. Re-list the assets and assert the old name is absent.

Steps 1 and 2 before step 3 is not an ordering preference. If the old asset went first, an
interruption between the delete and the flip would leave a pointer naming an asset that no longer
exists — the bundle unreadable, with no recovery, from a command whose whole purpose is routine
maintenance. Write that sentence into the doc comment.

**Step 4 is D5 and it is not optional.** The research flagged that a git object survives deletion
in history and makes a password change a comforting lie; Release assets were chosen partly because
their deletion is real. That payoff exists only if the deletion is confirmed. A delete that returns
success but whose re-list still shows the asset is a **failure**, reported as one, naming the asset
and saying the old wrapper is still reachable to anyone with the old password. Do not report
partial success. Do not swallow it as a warning — this is not prune, where leftover bytes cost
storage; here leftover bytes cost the entire point of the command.

**Locally**, replace the keyfile only after step 2 returned, through
`NamedTempFile::new_in` the destination directory, `persist()`, then an explicit mode 0600 — the
convention `src/tui/settings.rs` and Phase 1 already follow. Never `/tmp`: it is world-readable,
often a different filesystem so `persist` degrades to a copy leaving the original behind, and may
be tmpfs that survives in swap. Order matters here too: a local keyfile replaced before a failed
flip would leave the machine unable to open its own remote bundle.

**The honest message.** On success, print — not in a footnote — that changing the password is
**not revocation**: the data subkeys are unchanged, and anyone holding a copy of the old keyfile
can still unwrap it with the old password forever. Real revocation means a new master key and a
re-encrypted bundle, which this command deliberately does not do because that is precisely the
whole-bundle re-upload CRYPTO-04 exists to avoid. Say what was done and what it is worth; a user
who believes a rekey locked out a leaked copy has been misled by us.

Both passwords are `Zeroizing` throughout and arrive only from a TTY prompt, stdin, or a
mode-0600 file — never a command-line argument, never an environment variable. That is Phase 1's
rule and it is not relaxed here. Nothing in this file derives `Debug` on a type holding key
material, and no rendered line carries a password, a key, or a keyfile byte.

Tests drive `mockito::Server::new_async()` with both `Endpoints` fields at `server.url()`, cheap
KDF parameters, a `TempDir` for the keyfile, and a fixed `now`. Never production KDF parameters:
the AUR `check()` runs these on an installer's machine.
  </action>
  <verify>
    <automated>cargo test --lib sync::push::rekey</automated>
  </verify>
  <done>`cargo test --lib sync::push::rekey` is green. A chunk sealed before the rekey and one sealed after are byte-identical, proving the data subkeys did not move. The four remote steps happen in order, asserted as an order. A surviving old asset at the confirming re-list is a failure. No pack asset is touched on any path. The local keyfile is replaced atomically at mode 0600 only after the flip. The rendered output states that this is not revocation, and contains no password, key, or keyfile byte.</done>
  <reversibility rating="one-way">A rekey that generated a new master key instead of rewrapping the old one orphans every pack on the remote and reports success. There is no password recovery and no escrow by design, so the failure is unrecoverable and silent. `Keyfile::rewrap` is Phase 1's and is called, never approximated.</reversibility>
  <precondition>Plan 4-01 is merged: `PushCtx`, `Pointer`, `rekey::run`'s signature, `push::keyfile_asset_name`, `pointer::commit`, and `write::{upload_asset, delete_asset, list_assets}` exist as `4-01-SUMMARY.md` records them, and `SyncAction`'s rekey variant is already dispatched.</precondition>
  <precondition>Phase 1 and Phase 3 are merged: `crypto::Keyfile::{open, rewrap}`, `sync::passphrase`'s generation and strength floor, and 3-07's local keyfile path all exist. Read `3-07-SUMMARY.md` for where the keyfile is written; this plan replaces that file and must not invent a second location.</precondition>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|---|---|
| password entry → KEK derivation | Two passwords are live in memory simultaneously, and the old one still opens every copy of the old keyfile that exists anywhere |
| local keyfile → remote asset | The wrapped master key crosses to a host the project does not control |
| remote delete → the user's belief about revocation | The command's entire value rests on a deletion the user cannot see |
| rendered output → terminal and scrollback | A password and a wrapped key are both in scope during the flow |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|---|---|---|---|---|---|
| T-4-43 | Denial of service | a new master key generated instead of a rewrap | critical | mitigate | `Keyfile::rewrap` is called, never approximated, and a test seals a chunk before and after and compares the ciphertext byte for byte — a changed master key changes it |
| T-4-44 | Denial of service | the old keyfile deleted before the flip | critical | mitigate | Upload, then flip, then delete, then confirm; a test asserts the order rather than the set, and an interruption before the flip leaves the pointer naming the old keyfile, which still opens |
| T-4-45 | Repudiation | reporting success while the old wrapper survives | critical | mitigate | The confirming re-list is mandatory and its failure is an error naming the asset — D5's "verifiably delete", not "attempt to delete" |
| T-4-46 | Information disclosure | the old password still opening a leaked keyfile copy | high | accept | Inherent to a rewrap; mitigated by disclosure, not by code — the command states plainly that this is not revocation and what real revocation would cost |
| T-4-47 | Information disclosure | a password in argv or the environment | critical | mitigate | Accepted only from a TTY prompt, stdin, or a mode-0600 file, per Phase 1's rule; nothing here reads an environment variable or a command-line value |
| T-4-48 | Information disclosure | a password, key, or keyfile byte rendered | critical | mitigate | Both passwords are `Zeroizing`; no type here derives `Debug` over key material; a test checks the rendered output for both values and their eight-character prefixes |
| T-4-49 | Denial of service | the local keyfile replaced before a failed flip | high | mitigate | The local write happens only after `commit` returns, atomically through `NamedTempFile::new_in` the destination directory plus an explicit mode 0600 — never `/tmp` |
| T-4-50 | Tampering | a concurrent ordinary push republishing the old keyfile name | high | mitigate | 4-04's merge rule carries the `keyfile` field from the remote's current value unless the run is the one changing it |
| T-4-SC | Tampering | dependency surface | low | accept | Zero new crates. `Cargo.toml` is not in `files_modified` |
</threat_model>

<verification>
- `cargo test --lib sync::push::rekey` is green.
- `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` are clean.
- No test uses production KDF parameters, reads a real `$HOME`, or touches a real keyfile.
- No pack asset is uploaded, downloaded, or deleted on any path in this file.
- `src/sync/crypto.rs`, `src/sync/passphrase.rs`, and `src/sync/push/mod.rs` are unchanged by this
  plan.
</verification>

<success_criteria>
`sync rekey` under a new password opens the same master key, rewrites only the keyfile, flips the
pointer to it, deletes the old asset, and confirms it is gone — with not one pack byte moved. It
says plainly that this is not revocation. Every failure direction leaves a bundle that still
opens.
</success_criteria>

<output>
Create `.planning/phases/04-push-packs-atomic-flip-gc-rekey/4-06-SUMMARY.md` when done.

Record the exact four-step remote order and the confirming re-list, and state that the old-password
residual (T-4-46) is accepted-and-disclosed rather than mitigated — Phase 6's README work needs to
repeat it, and `docs/sync-format.md` §9 already says it.
</output>
</content>
