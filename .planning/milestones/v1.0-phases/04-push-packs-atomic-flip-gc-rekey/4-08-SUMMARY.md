---
phase: 04-push-packs-atomic-flip-gc-rekey
plan: 4-08
subsystem: sync/push
status: complete
tags: [security-remediation, two-machine, rollback-anchor, rekey, counter]
requires: [4-01, 4-02, 4-03, 4-04, 4-05, 4-06, 4-07]
provides: [anchor-on-the-push-path, monotonic-snapshot-counter, rekey-that-sticks]
affects: [5-01]
key-files:
  modified:
    - src/sync/push/mod.rs
    - src/sync/push/packer.rs
    - src/sync/push/upload.rs
    - src/sync/push/prune.rs
    - src/sync/push/rekey.rs
    - src/sync/github/gate.rs
    - src/sync/github/write.rs
    - src/sync/crypto.rs
    - src/sync/passphrase.rs
    - src/sync/mod.rs
    - src/sync/index.rs
    - src/sync/cli.rs
    - src/widget/cli.rs
    - docs/sync-github.md
    - tests/sync_push_e2e.rs
metrics:
  lib_tests: 1368
  total_tests: 1412
  failing: 0
  new_crates: 0
---

# Phase 4 Plan 08: Security Remediation — the Three Two-Machine Blockers Summary

The Phase 4 audit's three blocking findings are closed, plus F-4 through F-7, D1, and
verification instance 8. Every fix was negative-controlled — the fix reverted, the new test
watched to fail on the exact defect the audit described, the revert undone.

---

## ⚠️ SIGNATURE CHANGES — 5-01 IS BUILDING AGAINST THESE RIGHT NOW

Five contracts moved. Four are on the push side and are informational for restore; the first
is the one that changes what restore *reads*.

| Contract | Before | After |
|---|---|---|
| **`PushBundle`** | `{ packs, root: Vec<u8>, index_chunks, referenced_packs, counter: u64 }` | `{ packs, manifest_chunks: Vec<ChunkId>, index_chunks, referenced_packs }` |
| **`PushCtx`** | — | gains `pub allow_rollback: bool` |
| **`upload::run`** | `-> Result<(usize, usize, u64)>` | `-> Result<Uploaded { names: Vec<String>, skipped: usize, bytes: u64 }>` |
| **`prune::plan_deletions`** | `(pointer, assets, keep, now, grace)` | `(pointer, local_keyfile: &str, assets, keep, now, grace)` |
| **`gate::Pushing`** | `Pushing(())` | `Pushing(RepoRef)`, with `pub(crate) fn covers(&self, &RepoRef) -> Result<()>` |

**What 5-01 actually has to know**, beyond the table:

1. **The snapshot counter is now strictly monotonic across machines, and unique per snapshot.**
   The audit's carry-forward #3 said "Phase 5's selection must break ties deterministically and
   loudly rather than picking the first match". That tie can no longer be produced by the push
   path. Selection by highest counter is now well-defined. Restore should still refuse a
   duplicate loudly rather than assume — a hostile remote can still hand-write one — but it is
   no longer the ordinary two-machine outcome.

2. **The anchor is wired, and 5-01 must not implement it a second time.** Carry-forward #2 said
   so explicitly. The comparison lives in `push::assert_no_rollback`, the counter it compares
   comes from `packer::highest_counter`, and the file lives at
   `push::anchor_path(roots, repo)` → `<config_dir>/sync-anchor-<owner>-<name>.json`. **Restore
   must call these, not reimplement them**, and must write the anchor through
   `anchor::write_to` after a snapshot verifies. The path is keyed on the locally-configured
   `RepoRef`, never on the pointer's `repo_id` — `anchor.rs`'s module doc explains why that is
   load-bearing, and `the_anchor_is_named_after_the_remote_and_never_after_what_the_remote_claims`
   pins it.

3. **`--allow-rollback` exists on `sync push` only.** `sync prune` and `sync rekey` refuse a
   rolled-back pointer with no override. If restore wants an "open the older snapshot on
   purpose" path, the flag name is already taken and already documented; reuse it.

4. **A push refuses outright when this machine's keyfile is not the one the pointer names.**
   Restore will meet the same condition from the other side — a machine holding a superseded
   wrapper cannot open a bundle rekeyed elsewhere. The message text and the catch-up
   instruction are in `upload::assert_keyfile_is_current`; say the same thing.

5. **Carry-forward #1 is untouched and still open.** `RemoteIndexEntry.offset`, `clen` and
   `true_len` remain unauthenticated plaintext. Phase 4 still only echoes them. Phase 5 is the
   phase that dereferences them and must bound them against the pack it actually fetched.

---

## NEW-1 — the counter is derived where the race is resolved

**The defect.** `packer::build` derived the counter from the pointer read at step 2, sealed it
into a root, and `push::run` built one `SnapshotRecord` and `clone()`d it. `pointer::commit`
re-invoked `rebuild` on a 409 — but `rebuild` rebuilt the *list*, not the *root*. Two machines
reading a pointer at counter 6 both computed 7 and both published 7. Rule 1's dedup compared
root *bytes*, which differ, so it could not see the collision; `anchor::accept` reads an equal
counter as "already seen", so restoring A's snapshot made B's distinct snapshot read as a
re-read.

**The fix.** `PushBundle` no longer carries a root or a counter. `packer::root_for(ctx,
arriving, manifest_chunks)` derives the counter from the pointer **passed in** and seals the
root with it, and `push::run`'s rebuild closure calls it — the closure being the only code that
runs again after the race. On the retry `arriving` is the winner's pointer, so the loser
re-seals one above whatever the winner published.

This deliberately does *not* paper over it: the counter is still `highest + 1`, still read out
of sealed roots rather than off the pointer's shape, still monotonic, and `anchor::accept` is
unchanged.

**The one thing it cost.** Rule 1b's replay-idempotency dedup used to compare against one frozen
root. Re-sealing makes that root different on every attempt, so the closure now keeps a
`RefCell<Vec<String>>` of every root *this run* has produced and drops any arriving record
matching one. Same property, and it survives the re-seal. A `RefCell` rather than a `FnMut`
because `pointer::commit` takes `Fn`; changing that signature would have rippled into three
callers for no gain.

**Negative control.** Closure reverted to `root_for(&ctx, ctx.previous.as_ref(), …)`:

```
assertion failed: no two snapshots may claim one counter: [1, 2, 2]
```

**Test.** `a_flip_lost_to_another_machine_republishes_at_a_higher_counter_never_the_same_one` —
two genuine `Local` machines (`wrap_by_hand` is deterministic, so they share a master key and a
keyfile address while holding separate temp dirs, indexes, pairings and anchors). A wins the
flip, B loses it, and the landed pointer carries counters `[1, 2, 3]` with all three snapshots
intact. Each is then walked through `anchor::accept` with an advancing anchor: none reads as
already-seen.

---

## NEW-2 (T-4-04) — the anchor is on the path

**The defect.** The accept's written justification named "the local anchor's counter" as its
backstop. `grep -rn anchor src/sync/push/` returned three doc comments and zero reads. An
attacker with repo write replaced the pointer with an authentic older copy; every root opened,
`repo_id` matched, nothing errored. The next honest push carried those records forward, appended
its own and flipped — laundering the rollback into a legitimately-written pointer with a fresh
valid `sha` — and then `prune::run` computed liveness over the laundered pointer and deleted
every pack the rollback orphaned. Those packs are older than 24 h, so `PRUNE_GRACE` did not
cover them.

**The fix — wired, not reworded.** `push::assert_no_rollback(ctx, arriving)` reads the anchor and
refuses a pointer whose highest openable counter is below the local high-water mark, with
`anchor::accept`'s own `allow_rollback` escape. It is called from **three** places, not one:

- `push::run`, at step 2a — before the packer, so a refusal costs nothing;
- `prune::run_on_demand`, before liveness is computed — this is the path that actually performs
  the deletion, and guarding only `push` would have left `sync prune` as the executioner;
- `rekey::run`, after its pointer load — a rekey carries every arriving record forward untouched,
  so it launders a rollback exactly as a push does.

`push::run` advances the anchor at step 7b — after the flip lands, never before. Phase 1's rule
holds: the counter this machine refuses to go below is one it has *seen published*.

**The escape is real.** `anchor::accept`'s message names `--allow-rollback`, and until now no such
flag existed anywhere — a message outrunning its implementation, which is the exact class of
defect this phase kept producing. `sync push --allow-rollback` now exists, is documented, and is
exercised by the test. `sync prune` and `sync rekey` deliberately have no override: neither is a
command anyone reaches for when they mean to move the bundle backwards.

**The anchor path.** `<config_dir>/sync-anchor-<owner>-<name>.json`, keyed on the locally-configured
`RepoRef`. `anchor.rs`'s module doc states the constraint the audit told me to honour: a path
derived from the remote's claimed `repo_id` resolves to an absent file for any id this machine
has not seen, `read_from` returns `Ok(None)`, and `accept` returns `Ok(())` for `None` *before*
it compares anything — so the remote would manufacture its own amnesty. `owner/name` comes from
`[sync] repo` in `config.toml`, and `RepoRef::parse` has already restricted both halves to
`[A-Za-z0-9._-]` with `.` and `..` refused, so it is directly usable as a filename. Config
directory, never the cache: a wiped anchor is a free rollback.

**Negative control.** Both call sites removed:

```
a rolled-back pointer must not be pushed onto:
PushOutcome { packs_uploaded: 2, …, packs_deleted: 2, prune_warning: None }
```

`packs_deleted: 2` — the victim's own push destroying the packs the rollback orphaned, exactly
as described, exit 0.

**Test.** `a_pointer_rolled_back_to_an_authentic_older_copy_is_refused_rather_than_laundered` —
two pushes, every asset aged past `PRUNE_GRACE`, the pointer replaced with its own earlier bytes.
The push refuses, the tampered pointer is *not* republished, nothing is deleted, every orphaned
pack survives, `prune::run_on_demand` refuses on the same evidence, and `--allow-rollback` gets
through.

---

## NEW-3 (T-4-45) — a rekey that sticks, and what I decided

**The defect.** `upload::ensure_keyfile` published whatever keyfile is on *this* machine's disk.
Machine A rekeys and verifiably deletes keyfile-OLD; machine B, which has not rekeyed, pushes
and re-uploads keyfile-OLD. Prune calls it an orphan, but `PRUNE_GRACE` holds it 24 h and B's
next push resets `created_at`, so it is never collected. The old wrapper lives forever and the
password change was cosmetic. `upload.rs:146-152` named this as a "known sharp edge" for
"whoever wires the rekey path"; nobody wired it.

**The decision: refuse the push, before a byte is packed.**

The alternative was to skip the keyfile upload silently and let the push succeed. I rejected it,
and the reasoning is worth writing down because the silent-skip is the more *convenient* answer:

- **A silent skip leaves the bundle right and the user wrong.** The remote would be correct — the
  pointer names the new wrapper, the old one stays deleted — but the old password would keep
  opening this machine's local keyfile indefinitely, while the user believes they changed the
  password everywhere. The whole value of `sync rekey` is that the old wrapper is destroyed;
  a machine quietly holding one is the same failure one file further out.
- **The refusal is cheap and reversible.** A rekey rewraps the master key and re-encrypts no
  chunk, so B's data is entirely valid and everything it has already pushed stays readable. The
  refusal costs one file copy, not a backup. It is not the "backups stop working" trade it
  looks like at first glance.
- **It has to be loud because nothing else will tell them.** There is no restore command yet and
  no sync-status field that would surface this. The push is the only place the user meets this
  machine.

The check is `upload::assert_keyfile_is_current`, called from **two** places: at the top of
`push::run` (step 2b) where the refusal costs nothing, and at the head of `ensure_keyfile`
itself, which is the function that would do the damage — so a future caller cannot route around
the orchestrator's check. `previous == None` (a first push) is the only case that legitimately
publishes from local state.

The message states, in order: what happened, which wrapper is which, that nothing was uploaded
and the pointer is untouched, that the data is unaffected and why, and the exact catch-up —
copy `sync/keyfile.json` from the machine where the password was changed, then re-run.

**What I did not change.** `rekey::destroy` still deletes exactly one name, `previous.keyfile`.
The audit noted that any *other* keyfile asset survives a rekey untouched. That is prune's job
and prune already does it (`an_orphan_keyfile_no_pointer_names_is_swept_like_a_pack`, added by
4-05) — and it now actually works, because no machine re-uploads the orphan to reset its
`created_at`. Adding a second sweep to `rekey` would mean calling `ensure_release` from a
command that only wants to delete, which is precisely what 4-06 declined to do.

**Negative control.** Both checks removed: the stale machine's push succeeds and republishes.

**Test.** `a_machine_that_missed_a_rekey_refuses_rather_than_republishing_the_old_wrapper` — two
machines sharing a bundle, A runs the *real* `rekey::run`, B pushes. B refuses, `uploads() == 0`,
the destroyed wrapper is not back on the release, and the pointer still names the new one. Then
A's keyfile is copied onto B and B's push succeeds — the catch-up the message names is asserted
to work rather than merely described.

---

## The non-blocking findings

### F-4 — the incident cleanup no longer compares two clocks

`went_public_mid_push` selected assets by `a.created_at >= ctx.now` — GitHub's clock against this
machine's, captured at process start. A local clock seconds fast made it delete nothing and print
*"All 0 asset(s) this run uploaded were deleted."*, and `created_at` being host-supplied made it
a guaranteed no-op against a hostile remote.

`upload::run` now returns `Uploaded`, carrying the names it observed itself sending, and the
cleanup filters on membership alone. The `created_at` leg was standing in for "a pack this run
*skipped* must not be deleted" — a skipped pack is simply not in the set, so the leg is gone
rather than weakened.

**The fixture was the reason no test caught it**: the fake remote planted every upload at exactly
`NOW`, the single value that cannot expose a `>= now` comparison. It now stamps uploads at
`REMOTE_CLOCK`, 90 seconds behind `NOW`, with the reasoning written next to the constant.
Restoring the old predicate against the new fixture fails
`a_long_push_reports_advancing_counts_and_every_failure_names_an_action` on
`"the incident path deletes what this run uploaded"`.

### F-5 — both guards walk, and the shape is fixed rather than the instance

Two inherited structural guards could not see the directories Phase 4 created.

- `only_the_crypto_module_imports_the_cryptographic_crates` used a non-recursive `read_dir`,
  leaving `push/` and `github/` — 11,500 lines — outside the invariant whose stated value is
  "what lets a security auditor read one file instead of six".
- `no_password_input_path_reads_the_process_environment` iterated a **hand-maintained list of
  three files** that had never been extended to `push/rekey.rs`, the file T-4-47 is about.
  That is Phase 3's F-8 recurring verbatim: Phase 3 found the list stale and fixed it *by adding
  a file to the list*.

The instruction was to fix the shape. Both now walk recursively through one shared helper,
`sync::guard` (`rs_files`, `rs_files_in`, `production_code`), which also replaces the two
duplicate `collect_rs` copies in `github/mod.rs` and `github/gate.rs` — four copies down to one.

The passphrase guard enumerates **exemptions** instead of inclusions, and asserts `skipped == 1`:
the single exemption is `github/token.rs`, which reads the GitHub *token* from the environment
deliberately (a token is revocable; a sync password is not), with that justification written next
to it. A file added tomorrow is scanned by default. Both guards carry non-vacuity floors
(`checked >= 20`, `scanned >= 20`) so a renamed directory cannot report green forever.

**Negative control** — the audit's own two injections, `use chacha20poly1305::XChaCha20Poly1305;`
in `push/packer.rs` and `std::env::var("SYNC_PW")` in `push/rekey.rs`. Both guards, which the
audit watched pass, now fail. Both injections reverted.

### F-6 — `Pushing` names its subject

`Pushing(())` proved *when* a check happened and nothing about *what*. A permit minted against
private repo A type-checked against a `put_contents` to repo B; it held only because every caller
threads one `ctx.repo` through both. `PushClearance` now carries the `RepoRef` it was minted
against, `spend` moves it into `Pushing`, and all four write verbs open with
`permit.covers(repo)?`. The guard asserts the count of `permit.covers(repo)?;` equals the count of
`permit: &Pushing,` — so a fifth write verb that forgets the check fails the test rather than
compiling.

### F-7 — the keyfile exclusion has a trustworthy half

`plan_deletions` excluded `a.name != kept.keyfile`, sourced entirely from the untrusted pointer,
for the operation T-4-36 calls "the single worst thing this function could do". It now also
excludes `ctx.keyfile_asset`, this machine's own content address.
`a_lying_pointer_cannot_make_this_machine_sweep_its_own_keyfile` fails without the clause.

### Instance 8 — `Index::known_chunks` deleted

Zero production call sites; every reference a test. It was kept "to preserve a frozen surface".
A public function nothing reaches is instance 8 of this milestone's most repeated defect, and a
frozen surface with no caller is not a surface. Deleted; its seven tests now assert through
`chunk_locations`, which *is* called.

### D1 — the comment that described a hole 4-05 had closed

`upload.rs`'s "the old wrapper comes back as an orphan asset prune never collects" was false as
of 4-05's keyfile sweep, and is doubly moot now that nothing republishes the wrapper. The whole
"known sharp edge, for whoever wires the rekey path" paragraph is replaced by a description of
the refusal that actually exists.

---

## Documentation

`docs/sync-github.md` gains the two new user-visible refusals: the rollback refusal with
`--allow-rollback` and the fact that `prune`/`rekey` offer no override, and a new *"After a rekey,
catch up your other machines"* section stating plainly that the stale machine's push refuses, that
its data is fine, and how to copy the keyfile across. A CLI flag nobody documents is the same
defect class as a control nobody calls.

---

## Verification

| Gate | Result |
|---|---|
| `cargo test` | **1368 lib** (baseline 1365, +3), 3 `anthropic_e2e`, 13 `sync_adversarial`, **15 `sync_push_e2e`** (baseline 12, +3), 13 `sync_vectors` — **1412 total, 0 failing** |
| `cargo clippy --all-targets -- -D warnings` | exit 0, no warnings |
| `cargo fmt --check` | exit 0 |
| `make test` | exit 0, plus the GNOME, KDE and Omarchy JS contract suites |
| `env -u HOME -u XDG_CONFIG_HOME -u XDG_CACHE_HOME cargo test` | exit 0 — hermeticity holds |
| `Cargo.toml` / `Cargo.lock` | unchanged; no new crates |

**Negative controls run, and every one reverted:** NEW-1 (counter from `ctx.previous` → `[1, 2, 2]`),
NEW-2 (both anchor call sites removed → `packs_deleted: 2`), NEW-3 (both keyfile checks removed →
the stale push succeeds), F-4 (`created_at >= ctx.now` restored → cleanup deletes nothing), F-5a
(`chacha20poly1305` import in `packer.rs` → RED), F-5b (`std::env::var` in `rekey.rs` → RED), F-6
(one `covers` call removed → RED), F-7 (local-keyfile clause removed → RED). `git status` is clean
at the commit; nothing injected survives.

New tests are hermetic: every root is a `TempDir`, both `Endpoints` point at one mockito server,
`now` and the remote's clock are constants, and no test reads a real `$HOME`, token or Keychain.
`nothing_in_this_suite_resolves_a_real_home_or_a_real_token` still passes.

## Self-Check: PASSED

- `.planning/phases/04-push-packs-atomic-flip-gc-rekey/4-08-SUMMARY.md` — written
- commit `4bdc3bf` — present on `gsd/4-08`
