---
phase: 04-push-packs-atomic-flip-gc-rekey
plan: 06
subsystem: crypto
tags: [rekey, crypto-04, d5, rewrap, ordered-destroy, verifiable-delete, hermetic-tests]

requires:
  - phase: 01-encrypted-bundle-core
    provides: "`Keyfile::{open, rewrap}`, `KdfParams`, `content_address`, `passphrase::check`"
  - phase: 03-github-auth-and-the-private-repo-gate
    plan: 08
    provides: "`gate::Pushing`, `PushClearance::spend` — the consuming capability"
  - phase: 03-github-auth-and-the-private-repo-gate
    plan: 07
    provides: "`cli::keyfile_path` — the one local keyfile location, and the rekey arm's prompts"
  - phase: 04-push-packs-atomic-flip-gc-rekey
    plan: 01
    provides: "`PushCtx`, `Pointer`, `push::gate_now`, `keyfile_asset_name`, `pointer::{load, commit}`, `write::{ensure_release, list_assets, upload_asset, delete_asset}`"
provides:
  - "`rekey::run` — the whole of `ai-usagebar sync rekey`, filled"
affects: [4-03, 4-04, phase-5-restore, phase-6-readme]

tech-stack:
  added: []
  patterns:
    - "A rebuild closure written as `Pointer { keyfile, ..arriving.clone() }` — struct update syntax is what makes 'only this field moves' a compile-time fact rather than a promise."
    - "A mockito listing whose body comes from a call counter (`with_body_from_request`), so 'before the delete' and 'after the delete' are one mock rather than two whose match order belongs to the library."
    - "An `include_str!` guard over a *sibling* file the plan does not own, to pin a user-facing sentence (the not-revocation note in `cli.rs`) that no type can hold."

key-files:
  created: []
  modified:
    - src/sync/push/rekey.rs

requirements-completed: [CRYPTO-04]

duration: 1h
completed: 2026-08-19
status: complete
---

# Phase 4 / Plan 06: The Rekey

**`ai-usagebar sync rekey` rewraps the same master key under a new password,
publishes the new keyfile, flips the pointer to it, deletes the old asset, and
then re-lists to prove the deletion happened — with not one pack byte moved.**
An old wrapper that survives its own delete is an error naming it, never a
warning and never partial success.

## Task commits

1. `43f79dd` — the rewrap, the ordered upload/flip/delete/confirm, and the honest message

## The sequence, exactly

Offline first, then the gate, then the remote:

| # | Step | Why it is where it is |
|---|---|---|
| 0 | read the local keyfile, `passphrase::check` the new password, `Keyfile::rewrap` | The old password is verified **by unwrapping**, never by comparing. A wrong one fails here, before a single request, with Phase 1's one indistinguishable message (`wrong password or corrupted keyfile`). Phase 1's strength floor is applied through Phase 1's own function at the parameters this bundle lives at — not restated, not lowered. |
| 1 | `push::gate_now` → one `Pushing` | This uploads the wrapped master key. A repository can be flipped public from the web UI between `sync setup` and now, so the clearance is re-earned here exactly as a push re-earns it. The permit is minted once and passed **by reference** to every write below. |
| 2 | `pointer::load` | What is published now — and whose `keyfile` field names the asset to destroy. Never `ctx.keyfile_asset`: the remote's own claim is what readers resolve. |
| 3 | `ensure_release` | The release the assets hang off. |
| 4 | `upload_asset` the new keyfile | Content-addressed over exactly the bytes uploaded, so the new and old assets **coexist** for the instant between here and step 6. |
| 5 | `pointer::commit`, rebuild = `Pointer { keyfile, ..arriving.clone() }` | Only the keyfile field moves. Every snapshot record — including a competitor's that landed while this ran — is carried forward untouched. |
| 5a | replace the local keyfile: `cache::atomic_write` + explicit 0600 | **After** the flip, and **before** the delete. After, because a local keyfile replaced ahead of a failed flip leaves the machine unable to open its own remote bundle. Before the delete, because a delete that cannot be confirmed still has to leave this machine holding the keyfile the pointer now names. |
| 6 | `delete_asset` the old keyfile | Last, and only now. If it went first, an interruption between the delete and the flip would leave a pointer naming an asset that no longer exists — the bundle unreadable, with no recovery, from a routine-maintenance command. |
| 7 | `list_assets` again, and assert the old name is **absent** | D5. The delete is confirmed, not assumed. |

The order is asserted as an *order* — request-by-request against a mock, with the
gate's visibility read at index 0 — not as a set.

## The residual, stated because Phase 6 has to repeat it

**T-4-46 is accepted and disclosed, not mitigated.** Password change is **not
revocation**. The rewrap moves the wrapper; the three data subkeys are unchanged,
so anyone holding a copy of the old keyfile can still unwrap it with the old
password — forever, and including data written *after* the change. Deleting the
remote asset removes the copy this project published; it cannot reach one already
taken. Real revocation means a new master key and a re-encrypted bundle, which is
precisely the whole-bundle re-upload CRYPTO-04 exists to avoid.

`docs/sync-format.md` §9 already says this and the `sync rekey` arm in `cli.rs`
says it twice — before the prompts and after success. Because that sentence lives
in a file this plan does not own, a test here (`include_str!("../cli.rs")`, scoped
to the `fn rekey` arm) fails if a later edit drops it. Nothing in the module's own
output implies otherwise: the success value is the new asset name and nothing else.

## Deviations from the plan

**1. [Contract, forced] `rekey::run` takes no `release_id`.** As 4-01's summary
recorded (its deviation 3), a gate-first entry point cannot be handed a release id
fetched before its own gate. `run` mints the permit and calls `ensure_release`
itself. Built against the summary, not the plan text.

**2. [Coordination — needs a decision] `upload::ensure_keyfile` is *not* called,
and cannot be.** The coordinator's brief says 4-03 owns
`ensure_keyfile(ctx, release_id, permit) -> Result<()>`, idempotent by content
address, and that this plan should call it rather than write a second keyfile
upload. **That signature cannot express what a rekey uploads.** It derives both
the name and the bytes from `ctx` — i.e. from the *local* keyfile — and during a
rekey the local keyfile is deliberately still the **old** one until after the
flip (T-4-49). Calling it would either publish the old wrapper under a new name or
require writing the local keyfile before the flip, which is the exact failure
T-4-49 forbids.

So step 4 is a single `client.upload_asset` of the freshly-rewrapped bytes. This
is not a duplicate of 4-03's function: there is nothing to make idempotent, since
a rewrap draws a fresh salt and therefore always has a new content address, so the
asset provably does not exist yet. `ensure_keyfile` remains 4-03's to own for the
**first-push** path, which is the gap 4-01's deviation 9 identified and which this
plan does not close. **No conflict in `upload.rs`: this plan does not touch it.**

**3. [Additive arm] A bundle with no published pointer changes only the local
keyfile.** `pointer::load` returning `None` means nothing is published: there is
no remote wrapper to replace and nothing to flip, and uploading a keyfile no
pointer names would just be litter. The password still changes locally, and the
next push publishes the new keyfile through the ordinary path. No remote write is
issued on this arm at all — asserted.

**4. [Rebuild closure has a `None` arm that errors]** `pointer::commit` calls
`rebuild(current)` and 4-04's conflict retry will call it again with whatever is
now on the remote. If that is `None` — the pointer was deleted mid-rekey — this
refuses rather than writing a pointer with an empty snapshot list. Nothing is
flipped, the old keyfile is untouched, and the message says to re-run.

## Known gap, deliberately not closed here

**An interrupted *first* push can leave an orphan keyfile asset that a later rekey
does not destroy.** If a first push uploads the keyfile (4-03's `ensure_keyfile`)
and then fails before the flip, the remote holds a keyfile asset with no pointer
naming it. A rekey run in that state takes arm 3 above — local-only — and leaves
that orphan wrapper in place, so an old password still opens it. It is not
reachable through any published pointer, and the packs it would open are likewise
unreferenced, but the bytes are there. Closing it inside this file would mean
calling `ensure_release` (which *creates* a release) on a bundle that has none,
purely to run a delete. It belongs with whatever eventually sweeps unreferenced
assets — 4-05's prune covers packs only today.

## Security properties, and how each is enforced

| Property | Enforcement |
|---|---|
| T-4-43b — the wrapped master key uploaded to a repository that turned public | `gate_now` runs inside the command, before any request carrying a body. Test: a public repository produces exactly one request in the trace — `GET /repos/o/n` — and the error is 3-04's `REFUSING TO PUSH`. |
| T-4-43 — a new master key generated instead of a rewrap | `Keyfile::rewrap` is called, never approximated. Test: a chunk sealed before the rekey and one sealed after, under the subkeys the **new** password opens, are compared byte for byte — `Keys::seal` derives its nonce from the message, so a changed master key changes the ciphertext. The `chunk_id` is asserted equal too. |
| T-4-44 — the old keyfile deleted before the flip | Upload → flip → delete → confirm, asserted as an order. Test: a flip that 409s leaves `DELETE` at `expect(0)` and the **local** keyfile still opening under the old password. |
| T-4-45 — reporting success while the old wrapper survives | The confirming re-list is mandatory. Two tests: a delete GitHub *accepts* while still listing the asset, and a delete the remote refuses. Both are `Err`, both name the asset, both say the old password still opens it. |
| T-4-46 — the old password still opening a leaked copy | **Accepted, disclosed.** See above; guarded by the `cli.rs` sentence test. |
| T-4-47 — a password in argv or the environment | Nothing in this file reads either. Both passwords arrive as `&Zeroizing<String>` arguments from the CLI's prompt seam. |
| T-4-48 — a password, key, or keyfile byte rendered | Test asserts the returned name and every error string carry neither password, neither wrapped-master-key base64, nor an eight-character prefix of any of the four. |
| T-4-49 — the local keyfile replaced before a failed flip | `write_local` is called only after `pointer::commit` returns, through `cache::atomic_write` (tempfile in the destination's own directory + `persist`, never `/tmp`) plus an explicit 0600. Tests assert the mode, that no `.tmp.` file survives, and that a failed flip leaves the old keyfile in place. |
| T-4-50 — a concurrent push republishing the old keyfile name | 4-04's merge rule, unchanged: `push::run`'s closure takes `keyfile` from the pointer that arrived. This is the one command that overrides it. |
| CRYPTO-04 — no pack re-upload | Test asserts no request in the whole trace contains `pack-`. |

## Verification

```
cargo test --lib sync::push::rekey     12 passed, 0 failed          (0.30s)
cargo test --lib                       1312 passed, 0 failed        (baseline 1300)
cargo clippy --all-targets -- -D warnings   clean
cargo fmt --check                           clean
Cargo.toml / Cargo.lock                     unchanged
git diff --stat                             src/sync/push/rekey.rs only
```

- `grep -c 'Utc::now' src/sync/push/rekey.rs` → **0.** Every timestamp is `ctx.now`.
- No test reads a real `$HOME`/`$XDG` path, an environment variable, the network,
  or the wall clock: a `TempDir` holds the keyfile and the index, `mockito` holds
  the remote, `NOW` is fixed, and nothing sleeps.
- **Every keyfile in a test is written at `MIN_KDF_MEMORY_KIB` (8 MiB), `t = 1`,
  `p = 1`** — the format's own floor, never the shipped 1 GiB. Twelve tests
  totalling ~30 Argon2id derivations run in 0.30 s, which is what keeps the AUR
  `check()` inside its budget on an installer's machine. A consequence worth
  knowing: below the shipped `m_kib`, `passphrase::check` demands 20 characters,
  so the fixtures' passwords are 20 characters long and a short one is the
  floor-refusal test.

## Not done here

- `STATE.md`, `ROADMAP.md` and `REQUIREMENTS.md` were **not** touched: five
  sibling plans are executing in parallel worktrees against the same lines, and
  six edits to one progress table is six merge conflicts. The coordinator owns
  them after the merge. CRYPTO-04 is complete and ready to be checked off.
- `upload::ensure_keyfile` (deviation 2) — 4-03's, and still the thing that makes
  a *first* push produce a restorable bundle.
