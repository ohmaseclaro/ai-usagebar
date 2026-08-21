---
phase: 05-pull-and-restore
plan: 05
subsystem: sync/restore
tags: [safety, backup, tar, safe-04, d3]
status: complete
requires: ["5-01"]
provides:
  - "backup::take — a real archive where 5-01 had a refusal"
  - "backup::rollback_command — the shell-quoted undo line 5-06 renders"
  - "backup::take_with — the injected-tar test seam"
affects: ["5-04 (write::apply runs after it)", "5-06 (report prints the record)"]
tech-stack:
  added: []
  patterns:
    - "system binary at a fixed path behind an injected program-path seam (claude_desktop::app, anthropic::keychain)"
    - "0700 the directory before the file exists, 0600 the file after"
key-files:
  created: []
  modified:
    - src/sync/restore/backup.rs
    - src/sync/restore/mod.rs
decisions:
  - "BackupRecord::rollback_command delegates to backup::rollback_command so the quoting lives with the archive that needs it"
  - "the stamp is ctx.now, not claude_desktop::timestamp, because that helper reads the wall clock"
  - "the archive root is the sync roots' common ancestor, falling back to the targets' own when one escapes it"
metrics:
  duration: ~35m
  completed: 2026-08-19
---

# Phase 5 Plan 05: The Pre-Restore Backup Summary

`backup::take` now writes a real gzipped tar of every path a restore would
overwrite, at mode 0600 inside a mode 0700 `~/.claude-acc/backups/`, and hands
back the copy-pasteable `tar -xzf … -C …` that puts it all back — proven by a
test that actually runs that string through a shell and compares contents and
modes.

## Signature change — read this first

**One signature moved, and it is a body, not a shape.**

`BackupRecord::rollback_command(&self) -> String` is unchanged in
`restore/mod.rs`. Its **body** now delegates:

```rust
pub fn rollback_command(&self) -> String {
    backup::rollback_command(self)
}
```

That is the only edit outside `src/sync/restore/backup.rs`, and it is confined
to the `impl BackupRecord` block — no sibling plan owns that block. It was
unavoidable: 5-01 declared the method in `mod.rs` with a naive `format!`, and
Task 2's shell quoting cannot be added from `backup.rs` without a duplicate
inherent method. The plan anticipated this ("implement the body here, do not
redeclare the type").

**Callers are unaffected.** The rendered string is byte-identical for any path
made of `[A-Za-z0-9-_./=:+,@%]`, which is every path in the existing
`the_rollback_command_is_copy_pasteable` test in `mod.rs` — it still passes
untouched. Only a path containing a space, a quote, a `$`, or anything else
shell-significant now renders single-quoted.

Two new names, both `pub(crate)` and both in `backup.rs`:

- `take_with(ctx, targets, tar: &Path)` — the injected-`tar` test seam. `take`
  is `take_with(.., Path::new("/usr/bin/tar"))`.
- `rollback_command(record: &BackupRecord) -> String`.

Nothing 5-04 or 5-06 calls changed. `take` and `BackupRecord` are exactly the
frozen shapes.

## What was built

### The archive

`take(&RestoreCtx, &[PathBuf]) -> Result<Option<BackupRecord>>`:

1. Filter `targets` to what exists on disk, sort, dedup. Empty → `Ok(None)`.
2. Pick the archive root (below).
3. Make every survivor relative to it; sort; dedup.
4. `create_dir_all(ctx.backups_dir)`, then `chmod 0700` — **before** `tar`
   creates anything, so there is no window where a plaintext credential archive
   is world-readable (T-5-40).
5. `tar -czf <archive> -C <root> -- <members…>`. The `--` means a member
   beginning with a dash is never read as a flag (T-5-41). `tar` is
   `/usr/bin/tar` at a fixed path, never off `PATH` (T-5-42).
6. Non-zero exit → `Err` carrying `tar`'s stderr and exit code.
7. `chmod 0600` the archive, stat it, return the record.

### Naming

`<backups_dir>/sync-restore-<YYYYmmdd-HHMMSS>.tar.gz` — the account switcher's
directory and shape, so a user has one place to look for undo (D3).
`backups_dir` arrives injected on `RestoreCtx`; no test writes to a real one.

**`claude_desktop::timestamp` is deliberately not reused.** It is
`chrono::Local::now().format("%Y%m%d-%H%M%S%.3f")` — it reads the wall clock and
takes no argument, and the must-have says the stamp comes from `ctx.now` and
never from the wall clock. Widening it to `pub(crate)` would also have meant
touching a second out-of-scope file to gain a function that cannot be called
here. The *shape* is reused; the *source* is `ctx.now`, minus the milliseconds
the plan's own naming spec (`<YYYYmmdd-HHMMSS>`) omits.

### The archive root, and its fallback

`SyncRoots` has no `home` field, so the root is the component-wise longest
common ancestor of `config_dir`, `desktop_data_dir`, `desktop_profiles_dir` and
`claude_home` — the user's home on any real install, which is why one `-C`
covers all four sync roots and there is one rollback command rather than four.
`config_file` is excluded (it is `config_dir`'s child) and `index_file` is
excluded (it lives in the cache dir, outside the home, and would drag the root
up).

If any target is not beneath that ancestor — a customised `CLAUDE_CONFIG_DIR`
elsewhere, which is supported — the root widens to the longest common ancestor
of the targets' **parents**. The chosen root travels in `BackupRecord.root` and
is what `rollback_command` prints, because a command naming a different root
than the archive was built with silently restores into the wrong place.

Comparison is component-wise, not string-prefix, so `/home/bo` is never treated
as an ancestor of `/home/bob`. There is a test for exactly that.

### `BackupRecord`, as 5-06 will render it

```rust
BackupRecord {
    archive: PathBuf,  // <backups_dir>/sync-restore-<stamp>.tar.gz
    root: PathBuf,     // the -C root; usually the home, sometimes wider
    members: usize,    // files preserved — never zero when Some
    bytes: u64,        // the archive's size on disk
}
record.rollback_command()  // "tar -xzf <archive> -C <root>", both shell-quoted
```

`members` and `bytes` let the report say what was preserved without re-reading
the archive. **This module renders and never prints** — verified,
`grep -c 'println!\|print!'` on non-comment lines is 0.

## The property, and how it is now held

5-01 held *nothing archived implies nothing overwritten* by refusing to run.
That refusal is gone; the property is not. `take` has exactly two successful
exits:

- `Ok(None)` — no target existed on disk, so `tar` was never invoked and no
  archive was created. A create-only restore has nothing to undo.
- `Ok(Some(..))` — `tar` ran to completion, exited 0, the file was chmodded
  0600 and stat'd.

There is no third exit. Every failure — unrunnable `tar`, non-zero `tar`, a
target outside the root, a failed chmod or stat — is `Err`, which aborts `run`
at step 5 before `write::apply` at step 6. A backup that cannot be taken is
never a warning (T-5-45).

## Proving the ordering

The prompt asked for this now that `take` archives something. The seam that
makes it observable is the injected `tar` program path, and three tests use it:

- **`nothing_on_disk_means_no_archive_and_no_tar`** — the recorder log does not
  exist and neither does the backups directory. `None` is reachable only when
  there was genuinely nothing to preserve.
- **`the_archive_is_complete_and_extractable_the_moment_take_returns`** — the
  archive is extracted immediately on return, into a directory nothing else
  wrote, and its contents are the pre-restore bytes. `take` returns a finished
  artifact, not a promise: a restore killed at step 6 still has a complete,
  findable archive.
- **`the_rendered_command_restores_contents_and_modes_exactly`** — seed, take,
  clobber exactly as a restore would (new bytes, `chmod 0644`), run the rendered
  string through `/bin/sh -c`, compare bytes and modes. This is the only
  assertion that proves SAFE-04, because a rollback command that is well-formed
  and wrong looks identical to one that works (T-5-44).

A recorder *trait* is still not here, and still would not add anything: the
step-5-before-step-6 ordering is 5-01's numbered code in `run`, and `write.rs`
is landing in a parallel worktree where a cross-module ordering test could not
compile against it. What is provable inside this file's boundary — that the
archive holds pre-write bytes and reverses the write — is proved by execution
rather than by a recorded call order.

## Mode preservation

A credential archived at 0600 and restored at 0644 is a leak created by the
safety mechanism, so the round-trip test asserts modes, not just bytes. The
fixtures are seeded 0600 and 0700 deliberately: `tar` extracting as a non-root
user applies the host's umask, and no umask can clear a user-only bit, so the
assertion holds on a 022, 002, or 077 host. That is also why the test does not
seed 0644 or 0666.

## Deviations from Plan

**1. [Rule 3 - Blocking] `rollback_command`'s body had to move to `backup.rs`**
- **Found during:** Task 2
- **Issue:** The prompt scoped this plan to `backup.rs` alone, but 5-01 declared
  `BackupRecord::rollback_command` in `mod.rs` with an unquoted `format!`. Rust
  cannot have a second inherent method of that name in a sibling module, so
  Task 2's quoting requirement (T-5-43, success criterion 3) was unreachable
  without touching `mod.rs`.
- **Fix:** Replaced the three-line body with `backup::rollback_command(self)`.
  Nothing else in `mod.rs` changed. See the signature note at the top.
- **Files modified:** `src/sync/restore/mod.rs`
- **Commit:** 600ade5

**2. [Rule 2 - Correctness] Quoting is conditional, not unconditional**
- **Found during:** Task 2
- **Issue:** The plan says "quote both paths". Doing that unconditionally breaks
  5-01's merged `the_rollback_command_is_copy_pasteable` test, which asserts an
  unquoted `/home/bob/…`, and would put quotes on every ordinary path a user
  reads.
- **Fix:** `shlex`-style quoting — bare when the path is entirely
  `[A-Za-z0-9-_./=:+,@%]`, single-quoted otherwise with `'\''` for embedded
  quotes. Both branches are asserted, the awkward fixture carrying a space, a
  single quote, and a `$` at once. 5-01's test passes untouched.
- **Files modified:** `src/sync/restore/backup.rs`
- **Commit:** 600ade5

**3. [Rule 3 - Blocking] `claude_desktop::timestamp` not reused**
- **Found during:** Task 1
- **Issue:** The plan asked to reuse it. It reads `chrono::Local::now()`, which
  the must-have "never from the wall clock" forbids, and it is private in a
  second out-of-scope file.
- **Fix:** `ctx.now.format("%Y%m%d-%H%M%S")` — the same shape the plan's naming
  spec gives, from the injected clock. A comment in `backup.rs` points here.
- **Files modified:** `src/sync/restore/backup.rs`
- **Commit:** 600ade5

**4. [Rule 3 - Blocking] `take_with`, not a builder field, for the `tar` path**
- **Found during:** Task 1
- **Issue:** The plan says "a field on the record's builder". `take`'s signature
  is frozen and `BackupRecord` has no builder; adding either would change a
  shape four sibling plans compile against.
- **Fix:** `take_with(ctx, targets, tar)`, the repo's existing seam convention
  (`fetch_snapshot_at`, `parse_cache_at`, `read_from`). `take` is the production
  wrapper; the CLI has no path to `take_with`, which is what keeps T-5-42 closed.
- **Files modified:** `src/sync/restore/backup.rs`
- **Commit:** 600ade5

**5. [Rule 2 - Correctness] `set_private_mode` is local, not reused**
- **Found during:** Task 1
- **Issue:** `claude_desktop::app::set_private_mode` is private; widening it is a
  third out-of-scope file for four lines.
- **Fix:** A local `#[cfg(unix)]` / `#[cfg(not(unix))]` pair, identical to the
  one it mirrors.
- **Files modified:** `src/sync/restore/backup.rs`
- **Commit:** 600ade5

## Threat mitigations delivered

| Threat | Where |
|---|---|
| T-5-40 world-readable credential archive | `chmod 0700` before `tar` runs, `0600` after; `the_archive_is_0600_inside_a_0700_directory` |
| T-5-41 member read as a `tar` flag | `--` before the member list; asserted in the argv test |
| T-5-42 `tar` off `PATH` | fixed `/usr/bin/tar`, overridable only through `take_with` |
| T-5-43 rollback breaks on a space | `shell_quote`; fixture with a space, a quote, and a `$` |
| T-5-44 well-formed but wrong rollback | the rendered string is executed through `/bin/sh` and the tree compared |
| T-5-45 backup failure treated as a warning | every failure is `Err`; two tests (`tar` exit 2, `tar` absent) |
| T-5-46 archiving more than the restore touches | members come from `targets` only, existing-only |
| T-5-SC dependency install | no new crate; `Cargo.toml` and `Cargo.lock` unchanged |

## Verification

| Check | Result |
|---|---|
| `cargo test --lib sync::restore::backup` | 12 passed, 0 failed |
| `cargo test` (lib) | **1400 passed**, 0 failed (baseline 1388, +12) |
| `cargo test` (total) | **1441 passed**, 0 failed, 16 ignored (baseline 1429, +12) |
| `cargo clippy --all-targets -- -D warnings` | clean |
| `cargo fmt --check` | clean |
| `HOME= cargo test --lib sync::restore::backup` | 12 passed |
| `grep -c 'println!\|print!'` (non-comment) | 0 |
| `git diff --stat Cargo.toml Cargo.lock` | empty |

Every test is hermetic: one `TempDir` per fixture holds the roots, the backups
directory, the anchor path, the recorder script and the extraction target. No
real `$HOME`, no `$XDG`, no network (the `Client` in the fixture points at
`127.0.0.1:1` and is never used — `RestoreCtx` merely borrows one), no wall
clock. The two tests that touch a real binary inject `/usr/bin/tar` explicitly
and return early when it or `/bin/sh` is absent, so the AUR `check()` skips
rather than fails on an unusual host.

## Known Stubs

None.

## Self-Check: PASSED

- `src/sync/restore/backup.rs` — FOUND
- `src/sync/restore/mod.rs` — FOUND
- commit `600ade5` — FOUND
