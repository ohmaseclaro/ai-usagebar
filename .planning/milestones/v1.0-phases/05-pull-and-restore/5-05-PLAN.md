---
phase: 05-pull-and-restore
plan: 05
type: execute
wave: 2
depends_on: ["5-01"]
files_modified:
  - src/sync/restore/backup.rs
autonomous: true
requirements: [SAFE-04]
must_haves:
  truths:
    - "The archive exists and is closed before the first restore write is attempted — taken for a partial restore and taken under `--force`, because `--force` is exactly when it is needed (SAFE-04, D3)."
    - "It lands at `~/.claude-acc/backups/sync-restore-<YYYYmmdd-HHMMSS>.tar.gz`, the same directory and naming shape the account switcher already uses, so a user has one place to look for undo."
    - "The exact rollback command is printed, is copy-pasteable, and actually restores the prior state — proven by a test that runs it and compares trees."
    - "The archive is mode 0600 inside a mode 0700 directory, set before `tar` creates the file, because it contains credentials in the clear."
    - "Only paths the restore would actually write are archived; an item the plan skips is not put in the archive, so the archive is small and its contents are exactly the reversal set."
    - "A backup that cannot be taken aborts the restore. It is the last line of defence, so it is not a warning."
    - "No test spawns the real `tar`: the program path is injected, and the archive test uses a recorded invocation plus one hermetic round trip through the injected path."
  artifacts:
    - src/sync/restore/backup.rs — `take`, the archive naming, the mode discipline, and the rollback command string
  key_links:
    - "`/usr/bin/tar` at a fixed path with an injected override, following `claude_desktop::app::DesktopApp::archive` and `anthropic::keychain`'s `security(1)` convention — no new crate, nothing resolved off `PATH`"
    - "the archive is rooted at the *user's home* so one `-C` covers all four sync roots; members are the relative paths beneath it"
    - "`take` receives the destination paths from the plan, so it structurally cannot archive something the restore will not touch"
---

<objective>
The undo. Before the first byte of a restore is written, tar every path the restore would touch
into `~/.claude-acc/backups/sync-restore-<ts>.tar.gz`, at mode 0600 in a mode 0700 directory, and
hand back the exact `tar -xzf … -C …` line that puts it all back.

Purpose: SAFE-04. A wrong restore costs the credentials and history on the machine in front of you,
and this file is the only thing that makes that reversible.

Output: `src/sync/restore/backup.rs`, filled.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/05-pull-and-restore/5-CONTEXT.md
@.planning/phases/05-pull-and-restore/5-01-SUMMARY.md
@CLAUDE.md
@docs/claude-accounts.md
@src/claude_desktop/app.rs
@src/claude_desktop/mod.rs
@src/sync/restore/mod.rs
</context>

<tasks>

<task type="auto" tdd="true">
  <name>Task 1: The archive — naming, rooting, modes, and the members that are exactly the reversal set</name>
  <files>src/sync/restore/backup.rs</files>
  <behavior>
    - The archive path matches `sync-restore-<YYYYmmdd-HHMMSS>.tar.gz` under the injected backups directory, with the timestamp taken from `ctx.now` and never from the wall clock.
    - The backups directory is created 0700 before `tar` runs, and the archive is 0600 after it returns.
    - Members are the plan's destination paths, made relative to the archive root, deduplicated, and sorted — so two items in the same directory do not archive it twice.
    - A destination that does not exist yet contributes no member; a plan of only `Create` items produces an empty-member run that returns `None` for the archive rather than an empty tarball.
    - A `tar` that exits non-zero is an error carrying `tar`'s stderr, and `take` returns `Err` — the caller must not proceed to write.
    - The `tar` program path is injected; the default is the fixed `/usr/bin/tar`, matching `claude_desktop::app`, and no test invokes the real binary through `PATH`.
    - Nothing in the archive path or the printed command is interpolated from a manifest string — members come from resolved local paths only.
  </behavior>
  <action>
Fill `pub fn take(ctx: &RestoreCtx<'_>, targets: &[PathBuf]) -> Result<Option<BackupRecord>>` in
`src/sync/restore/backup.rs`.

Naming follows the account switcher exactly, because D3's point is that there is **one** place a
user looks for undo. `docs/claude-accounts.md` documents `~/.claude-acc/backups/`, and
`claude_desktop::mod` builds `switch-<stamp>-<label>.tar.gz` there. This produces
`sync-restore-<stamp>.tar.gz` in the same directory, with the same stamp shape
`claude_desktop::timestamp` renders. Reuse that helper rather than formatting a second timestamp;
if it is private, widen it to `pub(crate)` and leave a comment saying both callers now depend on
the shape. `backups_dir` arrives on `RestoreCtx`, injected, so no test writes to a real one.

Root the archive at the **user's home**, taken from `ctx.roots` rather than resolved here — the
four sync roots all sit beneath it on a real install, and one `-C` root means one rollback command
rather than four. If a root is somehow not beneath the home (a customised `CLAUDE_CONFIG_DIR`
elsewhere, which is a supported configuration), fall back to the longest common ancestor of the
targets and say so in the returned record, because the printed rollback command has to name the
same root the archive was created with or it silently restores into the wrong place. A test
constructs the disjoint-roots case deliberately.

Members: map each target to its path relative to the archive root, drop the ones that do not exist
on disk yet — a `Create` has nothing to back up — deduplicate, and sort. Sorting makes the archive
reproducible, which is what lets a test compare two runs. If no member survives, return `Ok(None)`:
a restore that only creates files has nothing to undo, and an empty tarball would be a misleading
artifact suggesting otherwise.

Directory and file modes before and after, exactly as `claude_desktop::app::archive` already does
it: `create_dir_all` the backups directory, `set_private_mode(parent, 0o700)` **before** `tar`
creates the file so even the pre-chmod window is contained, then `set_private_mode(archive, 0o600)`
after `tar` succeeds. This archive contains credentials in the clear; it is the one artifact of
this phase that is plaintext by design.

Invoke `tar` as `-czf <archive> -C <root> -- <members…>`, with `--` before the members so a member
beginning with a dash is never read as a flag. The program path is a field on the record's builder
with `/usr/bin/tar` as the default — fixed path, not `PATH` — following the same reasoning
`anthropic::keychain` uses for `security(1)`. Tests inject a recorder script path in a `TempDir`
and assert the argv; one test injects the real `/usr/bin/tar` behind a `#[cfg(unix)]` guard only if
it is present, and is skipped otherwise, so the AUR `check()` never depends on it.
  </action>
  <verify>
    <automated>cargo test --lib sync::restore::backup</automated>
  </verify>
  <done>`cargo test --lib sync::restore::backup` is green. The archive name, directory, and stamp match the account switcher's shape and reuse its timestamp helper. Modes are 0700 before and 0600 after. Members are deduplicated, sorted, existing-only, and `--`-separated. A `tar` failure is an `Err`. The program path is injected and no test depends on a binary being present.</done>
</task>

<task type="auto" tdd="true">
  <name>Task 2: The rollback command, proven by running it</name>
  <files>src/sync/restore/backup.rs</files>
  <behavior>
    - `BackupRecord::rollback_command` renders `tar -xzf <archive> -C <root>` with both paths shell-quoted, and the root is the same root the archive was created with.
    - A path containing a space, a quote, or a dollar sign renders as a single safe argument — asserted against a fixture path built to contain all three.
    - End to end on a temp tree: seed files, take a backup, overwrite them with different content, run the rendered command through a shell, and assert the tree matches the seeded state byte-for-byte including modes.
    - The record carries the member count and the archive's size on disk, so the report can say what was preserved without re-reading the archive.
  </behavior>
  <action>
`BackupRecord` and its `rollback_command` are declared in `restore/mod.rs` by 5-01 — implement the
body here, do not redeclare the type. `rollback_command` renders the exact line a user pastes.

Quote both paths. A home directory with a space in it is ordinary on macOS and a rollback command
that breaks on one is a rollback command that does not exist when it is needed. Single-quote and
escape embedded single quotes the standard way; do not reach for a crate for four lines of string
work.

The correctness test runs the command. Seed a temp tree, `take` a backup of it, clobber every file
with different bytes, execute the rendered string through `/bin/sh -c`, and compare the tree to the
seeded state — contents and Unix modes both. That is the only assertion that actually proves SAFE-04,
because a rollback command that is well-formed and wrong looks identical to one that works. Guard
the test on `/usr/bin/tar` and `/bin/sh` existing so the AUR `check()` on an unusual host skips
rather than fails, and say so in the test's doc comment.

The printed line goes into 5-06's report; this module renders the string and does not print. Keep
it that way — one place prints, and it is the report.
  </action>
  <verify>
    <automated>cargo test --lib sync::restore::backup</automated>
  </verify>
  <done>`cargo test --lib sync::restore::backup` is green. The rollback command round-trips a real temp tree through `tar` and `/bin/sh` and restores contents and modes exactly. Paths containing a space, a quote, and a dollar sign render as single safe arguments. `BackupRecord` carries the member count and byte size. Nothing in the module prints. `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` are clean.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| plaintext credentials → an archive on disk | The one artifact of this phase that is unencrypted by design |
| local paths → a subprocess argv | A path becomes an argument to `tar` |
| a rendered command → the user's shell | A string this code produces is executed by a human paste |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|-----------|----------|-----------|----------|-------------|-----------------|
| T-5-40 | Information disclosure | a world-readable credential archive | critical | mitigate | The backups directory is chmod 0700 **before** `tar` creates the file, and the archive is chmod 0600 after — the same ordering `claude_desktop::app::archive` already uses |
| T-5-41 | Tampering | a member path read as a `tar` flag | high | mitigate | `--` precedes the member list; members are resolved local paths, never manifest strings |
| T-5-42 | Elevation of privilege | a `tar` resolved off `PATH` | high | mitigate | Fixed `/usr/bin/tar`, overridable only through an injected field the CLI never sets |
| T-5-43 | Tampering | a rollback command that mis-parses on a path with spaces | high | mitigate | Both paths are shell-quoted, asserted against a fixture containing a space, a quote, and a dollar sign |
| T-5-44 | Repudiation | a rollback command that is well-formed and wrong | critical | mitigate | The test executes the rendered command through `/bin/sh` and compares the restored tree's contents and modes to the seeded state |
| T-5-45 | Denial of service | a backup failure treated as a warning | critical | mitigate | `take` returning `Err` aborts the restore; it is the last line of defence and is never advisory |
| T-5-46 | Information disclosure | archiving more than the restore would touch | medium | mitigate | Members come from the plan's destination paths only, so the archive is exactly the reversal set |
| T-5-SC | Tampering | npm/pip/cargo installs | high | mitigate | No new crates — `tar` is the system binary the switcher already uses; `cargo machete` runs in the phase gate |
</threat_model>

<verification>
- `cargo test --lib sync::restore::backup` green.
- `grep -v '^\s*//' src/sync/restore/backup.rs | grep -c 'println!\|print!'` is 0.
- `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` clean.
- `HOME= cargo test --lib sync::restore::backup` passes.
</verification>

<success_criteria>
1. The archive lands at the account switcher's directory and naming shape, 0600 in a 0700 directory.
2. Members are exactly the paths the restore would overwrite, deduplicated and sorted.
3. The rendered rollback command, actually executed, restores contents and modes exactly.
4. A backup that cannot be taken aborts the restore.
5. No test depends on the real `tar` being present.
</success_criteria>

<output>
Create `.planning/phases/05-pull-and-restore/5-05-SUMMARY.md` when done, recording the archive
naming, the archive root fallback rule, and the `BackupRecord` shape 5-06 renders.
</output>
