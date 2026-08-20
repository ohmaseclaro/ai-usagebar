---
phase: 05-pull-and-restore
plan: 04
subsystem: sync/restore
tags: [restore, atomic-write, file-modes, SAFE-05, partial-restore]
status: complete
requires:
  - "sync::restore::{RestoreCtx, RestorePlan, ItemPlan, Disposition, Applied, PackSource} (5-01, frozen)"
  - "sync::restore::layout::from_manifest_path (5-01) — re-run at the write boundary"
  - "sync::restore::backup::take (5-05) — must have run first; run() owns the ordering"
provides:
  - "sync::restore::write::apply — the only code in the crate that puts a synced byte on disk"
affects:
  - "5-06 (report renders Applied::failed_at + overwritten), 5-07 (CLI resolves NeedsCredentialConfirm before apply)"
tech-stack:
  added: []
  patterns:
    - "tempfile in the destination's own directory, chmod 0600 before content, persist — the cache.rs pattern, reused not re-derived"
    - "DirBuilder::mode(0o700) + recursive: closes what the restore creates, leaves what the user made"
    - "whole-plan preflight before the first byte, so Err means nothing was written"
    - "std::fs::FileTimes on the tempfile before persist — no crate, no second open of the destination"
key-files:
  created: []
  modified:
    - src/sync/restore/write.rs
decisions:
  - "a per-item write failure is Ok(Applied::failed_at), not Err — run() already gates the anchor on it and report.rs already renders it; the cause goes to stderr"
  - "the whole plan's paths are re-resolved before the first write, so an Err from apply carries the guarantee that nothing landed"
  - "a symlinked directory above the destination is followed deliberately; the bundle cannot create one and refusing breaks the legitimate ~/.claude-on-another-disk setup"
  - "the manifest's mode is structurally unreachable — ItemPlan does not carry it — rather than merely unread"
metrics:
  duration: ~35 min
  completed: 2026-08-19
---

# Phase 5 Plan 04: The write path — Summary

`write::apply` fills in: every decrypted byte reaches disk through a `.tmp.` file
created in the **destination's own directory**, chmod 0600 before its first byte,
stamped with the snapshot's `created_at`, and renamed into place — with the whole
plan's paths re-checked before the first tempfile exists, so an `Err` from `apply`
means nothing was written and an `Ok` carrying `failed_at` means a partial restore
that a re-run can finish.

---

## Signature changes

**None.** `pub fn apply(ctx: &RestoreCtx<'_>, plan: &RestorePlan, packs: &PackSource) -> Result<Applied>`
is exactly the frozen 5-01 signature. No type in `restore/mod.rs` was touched.

**One behaviour change siblings must know about**, because 5-01's tracer did the
opposite: a failure *while writing an item* is no longer propagated as `Err`. It
is now `Ok(Applied { failed_at: Some(manifest_path), .. })`.

- This is what `run`'s step 7 (`if applied.failed_at.is_none()`) and `report.rs`'s
  `stopped at {failed}` line were already written against — under the old
  behaviour both were unreachable.
- `Err` from `apply` now means, and only means, **the plan was refused before any
  byte was written**: an unresolved `NeedsCredentialConfirm`, a writable item with
  no destination, or a destination that disagrees with its manifest path.
- `failed_at` is the manifest path verbatim, nothing appended — 5-06 can render it
  as a path. The *cause* (the io error) is printed once to stderr as
  `sync: restore stopped at <path>: <error>`, since `Applied` has nowhere to carry
  it and swallowing it would make a partial restore undiagnosable.

---

## The write ordering, exactly

Each step's position is the mitigation; this is the order to preserve if the file
is ever refactored.

**Preflight, over every item in the plan, before the first tempfile:**

1. `NeedsCredentialConfirm` → hard error. The CLI (5-07) resolves it to
   `Overwrite` or `SkipLocalNewer` before `apply` runs; reaching here means the
   gate was skipped, which is not a thing to guess about.
2. Non-writable dispositions → `skipped += 1`, no destination touched.
3. A writable item with `dest: None` → hard error. `RejectedPath` and
   `ExcludedByPolicy` are the only dispositions that carry no destination and
   neither writes, so this is a bug in the plan, not a case to skip quietly.
4. `layout::from_manifest_path` re-run against `ctx.roots`; a result that differs
   from the plan's `dest` → hard error. Defence in depth against a plan mutated
   between planning and applying (T-5-36).

**Per item, in manifest order** (deterministic, so a partial restore stops in the
same place twice):

5. `DirBuilder::new().recursive(true).mode(0o700).create(parent)` — **before** the
   tempfile, so it is never briefly parented by a world-listable directory
   (T-5-33). `DirBuilder`'s mode applies only to directories it creates, so a
   pre-existing `~/.claude` keeps whatever its owner gave it.
6. `tempfile::Builder::new().prefix(".tmp.").tempfile_in(parent)` — the same
   prefix `cache::atomic_write` uses, which `scope`'s exclusion rules already
   ignore if a collection scan runs concurrently.
7. `set_permissions(0o600)` on the open handle, **before the first byte**.
   `persist` keeps the tempfile's mode, so there is no readable window at either
   name (T-5-31).
8. Chunks written as `PackSource::chunk` hands them over, one `Zeroizing` buffer
   at a time — no whole-file `Vec` (T-5-37).
9. Length check: reassembled bytes must equal `item.true_len`, else refuse.
10. `sync_all`.
11. `File::set_times(FileTimes::new().set_modified(created_at))` — stdlib since
    1.75, no crate. On the **tempfile**, before the rename, which preserves it:
    no second open of the destination to fail. Best effort; a filesystem that will
    not take a timestamp does not fail a restore that already succeeded.
12. `persist(dest)`. The real name is only ever reached by this rename; nothing
    here opens the destination for writing, so a half-written credential cannot
    exist at its real name (T-5-34).

Every error path between 6 and 12 drops the `NamedTempFile`, whose `Drop` removes
it. Nothing calls `into_temp_path().keep()`; there is no staging name outside the
destination directory to rename later (T-5-35).

## What survives a kill mid-restore

Stated plainly, because it is the question this plan exists to answer:

- Items already persisted stay persisted, byte-complete, at 0600.
- At most **one** unpersisted `.tmp.` file sits inside a destination directory.
  Nothing in a shared temp directory, nothing half-written at a real name.
- Items after the interruption are untouched.
- **A re-run finishes it.** Every write is idempotent, and 5-03's `SkipIdentical`
  means the second run does not reopen what the first completed. The stray
  `.tmp.` file is `scope`-excluded, so it is never collected into a later push.
- The rollback anchor was not advanced (`run` gates it on `failed_at.is_none()`),
  so the machine has not claimed to have seen this snapshot whole.

A partial restore is **reported, not rolled back**: undoing the successful writes
would mean writing again, from an archive, on a machine that just demonstrated it
cannot complete a write. The user gets 5-05's rollback command and decides.

## The `Applied` shape 5-06 renders

```rust
Applied {
    written: usize,           // files that reached their real name
    overwritten: Vec<String>, // manifest paths of the Overwrite items, in manifest order
    skipped: usize,           // the non-writable dispositions
    failed_at: Option<String>,// the manifest path the run stopped at, verbatim
}
```

Items *after* `failed_at` are in neither `written` nor `skipped` — they were never
attempted, and `failed_at` is what tells the reader the run stopped. `overwritten`
lists `Disposition::Overwrite` only (never `Update`, which replaces a file the
plan established is older), in manifest order, so the summary can print it without
re-deriving anything.

## Modes, and what is never consulted

- Files: **0600**, unconditionally.
- Directories the restore creates: **0700**, unconditionally.
- Directories that already existed: **untouched**.
- The manifest's recorded `mode` is not merely ignored — `ItemPlan` does not carry
  it, so there is no value in this module to apply by accident (T-5-32). The one
  user-visible consequence is that a restored executable does not come back
  executable; no category in the bundle contains one.

## The backup precondition, unrelaxed

`run` calls `backup::take` over exactly the destinations whose
`Disposition::writes()` (step 5) before `write::apply` (step 6), so *nothing
archived implies nothing overwritten*. `apply` offers no route around that: it
takes no "skip the backup" option, reads no flag that could grant one, and was
built against 5-05's contract rather than against today's refuse-if-anything-exists
stub. Nothing in this plan touched `backup.rs`, `mod.rs`, or the ordering.

## Tests — 14, all hermetic

Every one runs inside a `TempDir`; none resolves a real `$HOME`/`$XDG`, none
dials (the fixture `Client` is parked at `127.0.0.1:1`), none reads the wall
clock — `SNAPSHOT` and `NOW` are fixed and deliberately far apart, so a file
stamped with the wrong one of the two is visible. `HOME= cargo test --lib
sync::restore::write` passes, which is what the AUR `check()` effectively does.

Packs are built through the real `PackWriter` + `chunk::seal_chunk` with
`KdfParams { m_kib: 8, t: 1, p: 1 }` — microseconds, never production parameters.

| Test | What breaks it |
|---|---|
| `a_multi_chunk_file_lands_byte_for_byte_at_mode_0600_stamped_with_the_snapshot` | wrong bytes across a 3-chunk file, a mode other than 0600, or `now` instead of `created_at` |
| `every_directory_the_restore_creates_is_closed_to_other_users` | any group/world bit on a directory the restore made |
| `a_directory_that_already_existed_keeps_the_mode_its_owner_gave_it` | re-chmodding the user's own 0755 directory |
| `a_chunk_the_packs_do_not_carry_leaves_nothing_in_the_destination_directory` | a surviving `.tmp.` file after a failure between tempfile and persist |
| `a_reassembly_shorter_than_the_snapshot_records_is_refused_and_leaves_nothing` | writing a truncated file |
| `a_destination_whose_parent_is_a_file_stops_the_restore_rather_than_panicking` | a panic instead of a report |
| `only_the_three_writable_dispositions_put_a_file_on_disk` | any of the five non-writing dispositions producing a file |
| `overwritten_names_every_item_it_replaced_in_manifest_order` | reordering what 5-06 prints |
| `a_failure_part_way_keeps_what_it_wrote_and_names_where_it_stopped` | rolling back, continuing past the failure, or leaving a tempfile under the roots |
| `a_credential_awaiting_confirmation_reaching_apply_is_an_internal_error` | guessing which way the gate was answered |
| `a_destination_that_disagrees_with_its_manifest_path_is_refused_before_a_byte_is_written` | writing item 1 before checking item 2's path |
| `a_writable_item_with_no_destination_is_a_bug_not_a_skip` | skipping it quietly |
| `applying_the_same_plan_twice_leaves_the_same_bytes_and_no_leftovers` | non-idempotent re-runs |
| `nothing_in_this_module_reaches_for_a_shared_temporary_directory` | any appearance of `temp_dir`, `/tmp"`, or `into_temp_path` in non-comment source |

## Deviations from Plan

### [Rule 1 — correctness] A per-item write failure returns `Ok`, not `Err`

Described in full under **Signature changes** above. The plan's own text ("its
`failed_at` carries the manifest path of the item that stopped the run") and
5-01's `run` and `report.rs` all require it; the tracer's `?` contradicted them.

### [Rule 2 — missing critical functionality] The path re-check moved to a whole-plan preflight

The plan puts `layout::from_manifest_path`'s re-check inside the write loop. It
now runs over every writable item first. Same check, strictly stronger property:
a mismatch at item 40 no longer leaves items 1–39 written *and* the report
discarded by `?`. It also makes `Err` from `apply` mean one thing — nothing was
written — instead of two.

### Scoped out: the `TMPDIR`-walking assertion

The plan asks the failure tests to walk a temp `TMPDIR` as well as the roots.
Observing that requires mutating `TMPDIR` for the process, which is
process-global, racy under the test harness's threads, `unsafe` in edition 2024,
and exactly the "branch on an ambient env var" the project's hermeticity rule
forbids. Replaced by a stronger, deterministic assertion: an `include_str!` of
this module's own source, comment lines stripped, must not contain `temp_dir`,
`/tmp"`, or `into_temp_path`. Plaintext has no route to a shared temp directory
because no such call exists — which is what the walk was trying to infer.

### Scoped out: the symlink refusal 5-01's doc comment assigned here

`layout.rs` defers symlinks to "the write boundary — plan 5-04 owns that", but
5-04's tasks, threat register and success criteria never mention them. Deliberately
not added, and the module doc now says why: nothing here ever creates a symlink,
`persist` is a rename so an existing symlink *at* the destination is replaced
rather than written through, and a symlinked directory *above* the destination is
a configuration the user made on this machine — `~/.claude` pointed at another
disk is a real setup. The bundle cannot create one (restore writes regular files
only, and `layout` refuses traversal), so refusing would break the legitimate case
far more often than the hostile one, which needs write access to the home
directory to arrange and would not need a restore to exploit it. **Worth a line in
the phase's security audit (2g) rather than silent omission.**

### Not asserted here: the second-`apply`-writes-zero property

The plan wants it "asserted end to end by re-planning". `merge::plan` is 5-03's
file, landing in parallel, and today still returns `Update` for an existing
destination — the assertion would fail against the current tree while being a
statement about someone else's module. The write-level half that D7 rests on
*is* asserted (`applying_the_same_plan_twice_leaves_the_same_bytes_and_no_leftovers`);
the `SkipIdentical` half belongs to 5-03's suite and to `mod.rs`'s end-to-end.

## Verification

| Gate | Result |
|---|---|
| `cargo test` | **1402 lib / 1443 total, 0 failing** (baseline 1388 / 1429 — +14, all new, all in this module) |
| `cargo clippy --all-targets -- -D warnings` | clean |
| `cargo fmt --check` | clean |
| `HOME= cargo test --lib sync::restore::write` | 14 passed |
| `grep -v '^\s*//' … \| grep -c 'temp_dir\|"/tmp"'` | 0 |
| `grep -v '^\s*//' … \| grep -c 'into_temp_path'` | 0 |
| `Cargo.toml` / `Cargo.lock` | unchanged — `FileTimes` and `DirBuilderExt` are stdlib |

## Commits

| Commit | What |
|---|---|
| `e8cbb89` | `test(5-04)` — 14 tests, 9 failing (RED) |
| `0aaf572` | `feat(5-04)` — the preflight, directory modes, mtime stamp, and partial-restore report (GREEN) |

## Self-Check: PASSED

- `src/sync/restore/write.rs` — present, modified, 14 tests green.
- `e8cbb89`, `0aaf572` — both in `git log` on `gsd/5-04`.
