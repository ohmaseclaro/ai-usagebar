---
phase: 05-pull-and-restore
plan: 04
type: execute
wave: 2
depends_on: ["5-01"]
files_modified:
  - src/sync/restore/write.rs
autonomous: true
requirements: [SAFE-05]
must_haves:
  truths:
    - "Every decrypted byte reaches disk through a `NamedTempFile::new_in` created in the **destination's own directory**, chmod 0600 before any content is written, then `persist`d — so plaintext never exists at a path outside the destination directory, not even for an instant (SAFE-05)."
    - "Created directories are mode 0700 and are created before the tempfile, so the tempfile is never briefly parented by a world-readable directory."
    - "A restored file's mode is 0600 for every file and is **not** read from the manifest — the recorded mode is attacker-controllable and is ignored, which is recorded as a deliberate narrowing."
    - "A failure part-way through leaves no tempfile behind: the partial output is removed on every error path, asserted by scanning the destination directory after an injected failure."
    - "Killing the process mid-restore can leave an unpersisted `.tmp.` file inside the destination directory and nothing anywhere else — no plaintext in `/tmp`, no half-written credential at its real name."
    - "Each restored file's mtime is set to the snapshot's `created_at`, which is what makes 5-03's newer-local comparison exact instead of merely conservative."
    - "`layout::from_manifest_path` is re-run at the write boundary as defence in depth, and a destination that fails it at write time is a hard error, not a skip."
  artifacts:
    - src/sync/restore/write.rs — `apply`, the atomic dest-dir write path, the mode and time policy, and the failure cleanup
  key_links:
    - "`tempfile::NamedTempFile::persist` keeps the tempfile's mode, so the chmod happens on the tempfile *before* the content is written — chmod after persist leaves a window at the real name"
    - "`/tmp` is refused three times over: world-readable, often a different filesystem so `persist` degrades to a copy that leaves the plaintext original behind, and often tmpfs that can reach swap"
    - "`std::fs::File::set_times` is stdlib since 1.75; the mtime stamp costs no crate"
    - "the manifest's `mode` field is deliberately unused — narrowing to 0600/0700 is safe in the only direction that matters"
---

<objective>
The write path. Turn an `ItemPlan` plus the bytes `PackSource` hands over into a file on disk that
is atomic, mode 0600, timestamped to the snapshot, and that never existed as plaintext anywhere
outside its own destination directory.

Purpose: SAFE-05, and the interrupted-restore case. A half-written `config.toml` or credential file
is the worst outcome this phase can produce.

Output: `src/sync/restore/write.rs`, filled.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/05-pull-and-restore/5-CONTEXT.md
@.planning/phases/05-pull-and-restore/5-01-SUMMARY.md
@CLAUDE.md
@src/cache.rs
@src/sync/chunk.rs
@src/sync/restore/mod.rs
@src/sync/restore/layout.rs
@src/tui/settings.rs
</context>

<tasks>

<task type="auto" tdd="true">
  <name>Task 1: One file, written the only way it is allowed to be written</name>
  <files>src/sync/restore/write.rs</files>
  <behavior>
    - The tempfile is created in the destination's parent directory; a test asserts the temp path's parent equals the destination's parent.
    - The tempfile's mode is 0600 before the first byte is written; a test reads the mode between creation and write via an injected hook or by writing zero bytes and stating the order in the assertion.
    - The persisted file is mode 0600 and its bytes equal the plaintext exactly, for a file spanning several chunks.
    - The persisted file's mtime equals the snapshot's `created_at`.
    - Missing parent directories are created mode 0700, and an existing directory with a wider mode is tightened for directories the restore itself creates but left alone for pre-existing ones.
    - A write that fails after the tempfile exists leaves no `.tmp.` entry in the destination directory and does not touch the destination name.
    - Nothing anywhere in the module resolves `std::env::temp_dir`.
  </behavior>
  <action>
Fill the single-file write in `src/sync/restore/write.rs`. The module doc states the rule once, in
the imperative: decrypted plaintext is created in the directory it will live in, at a temporary
name, mode 0600, and reaches its real name only by `persist`. There is no other path.

Order matters and each step's position is the mitigation:

1. Re-run `layout::from_manifest_path` on the item's manifest path against `ctx.roots` and compare
   it to the `dest` the plan carries. They must be equal. This is defence in depth against a plan
   mutated between planning and applying, and it is cheap. A mismatch is a hard error that aborts
   the restore — not a skip, because a plan that disagrees with itself is not a situation to
   continue from.
2. `create_dir_all` the parent, then set each directory the restore itself created to 0700. Do this
   **before** the tempfile: a tempfile born in a 0755 directory is world-listable for the window
   between creation and the chmod of its parent, and the whole point is that there is no window.
   Do not re-chmod a directory that already existed — the user's own permissions on `~/.claude` are
   not this command's to narrow, and a surprise 0700 on a shared directory is its own bug.
3. `tempfile::NamedTempFile::new_in(parent)` with the crate's existing `.tmp.` prefix so
   `scope`'s exclusion rules already know to ignore it if a concurrent scan runs.
4. Set the tempfile's mode to 0600 **before** writing content, behind `#[cfg(unix)]`, using
   `PermissionsExt` on the open handle. `persist` preserves the tempfile's mode, so setting it here
   means the file is never readable by anyone else at any point, including at its real name. A
   chmod after `persist` leaves exactly the window this ordering removes. Do it anyway as a belt —
   `src/tui/settings.rs` already sets the mode explicitly for the same reason — but the load-bearing
   one is the pre-write chmod.
5. Write each chunk's plaintext as it comes from `PackSource`, holding a `Zeroizing` buffer and no
   accumulated `Vec` of the whole file. A multi-megabyte transcript should not be assembled in RAM
   just to be written out.
6. `sync_all`, then `persist`.
7. `File::set_times` with `FileTimes::new().set_modified(created_at.into())` on the persisted path.
   `std::fs::FileTimes` is stdlib; no crate. If the platform refuses, log nothing and carry on —
   the mtime is an optimisation for the next pull's comparison, not a correctness property, and a
   restore that fails because a filesystem does not support `utimes` would be absurd.

The manifest's `mode` field is **not** consulted. Say so in a comment with the reason: it is
attacker-controllable, and every value it could carry that differs from 0600 is worse. Files are
0600 and directories are 0700, full stop. Note the one user-visible consequence — a restored
executable does not come back executable — and that no category in the bundle contains one.

Never `std::env::temp_dir`, never a literal `/tmp`. Three independent reasons, all worth stating in
the comment: it is world-readable, so plaintext credentials sit where anyone can read them; it is
frequently a different filesystem, where `persist` cannot rename and degrades to a copy that leaves
the plaintext original behind at the temp path — the precise failure SAFE-05 names; and it is often
tmpfs, which can reach swap and outlive the process on disk.
  </action>
  <verify>
    <automated>cargo test --lib sync::restore::write</automated>
  </verify>
  <done>`cargo test --lib sync::restore::write` is green. The tempfile is created in the destination's parent, chmod 0600 before content, persisted, and stamped with the snapshot time. Directories the restore creates are 0700; pre-existing ones are untouched. No accumulated whole-file buffer. `grep -c` finds no `temp_dir` and no `/tmp` literal outside comments.</done>
</task>

<task type="auto" tdd="true">
  <name>Task 2: `apply` — the loop, and what it leaves behind when it fails</name>
  <files>src/sync/restore/write.rs</files>
  <behavior>
    - `apply` writes only items whose disposition is `Create`, `Update`, or `Overwrite`; the five other variants are counted and skipped, and a test asserts no file appears for any of them.
    - A failure on item K leaves items 1..K-1 persisted, item K absent, and no `.tmp.` entry anywhere under the roots — asserted by walking the roots after an injected failure.
    - The returned `Applied` names every item actually overwritten, in manifest order, so 5-06's summary can list them without re-deriving anything.
    - A second `apply` of the same plan against the already-restored tree writes zero files, because 5-03 marked them `SkipIdentical` — asserted end to end by re-planning, not by trusting the first plan.
    - A destination whose parent exists as a *file* is an error naming the path, not a panic.
    - Under an injected failure the process leaves nothing readable outside a destination directory: the test walks a temp `TMPDIR` as well as the roots and asserts both are clean.
  </behavior>
  <action>
Fill `pub fn apply(ctx: &RestoreCtx<'_>, plan: &RestorePlan, packs: &PackSource) -> Result<Applied>`.

Iterate `plan.items` in manifest order — deterministic order makes a partial restore
reproducible, and a partial restore that stops in a different place each time is not debuggable.
Write only `Create`, `Update`, and `Overwrite`. Every other disposition increments a counter.
`NeedsCredentialConfirm` reaching here at all is a bug: the CLI resolves it to `Overwrite` or
`SkipLocalNewer` before `apply` runs, so treat it as an internal error rather than guessing.

Cleanup is per item and unconditional: wrap the single-file write so that any error drops the
`NamedTempFile` — which removes it — before the error propagates. `NamedTempFile`'s `Drop` already
does this, so the requirement is negative: do not `into_temp_path().keep()` and do not persist to a
staging name to rename later. Assert it rather than assume it, by injecting a failure at each of
the write's steps and walking the destination directory afterwards.

`Applied` is already declared in `restore/mod.rs` by 5-01 — fill its use, do not redeclare it here.
Its `failed_at` carries the manifest path of the item that stopped the run, so the error the user sees
names where the restore reached — and the backup archive plan 5-05 already wrote is what makes
that recoverable.

A restore that fails part way is a **partial restore, reported as one**, not a rollback. Say that
in the doc comment and say why: automatically undoing the writes that succeeded would mean writing
again, from an archive, on a machine that just demonstrated it cannot complete a write — more
failure surface at exactly the wrong moment. The user gets the rollback command and decides.
  </action>
  <verify>
    <automated>cargo test --lib sync::restore::write</automated>
  </verify>
  <done>`cargo test --lib sync::restore::write` is green. Only the three writable dispositions produce files. An injected failure at every step of the write leaves no tempfile under the roots or under `TMPDIR`, and leaves the destination name untouched. `Applied` names the overwritten items in manifest order and records `failed_at`. `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` are clean.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| decrypted plaintext → filesystem | The only place in the crate where a synced file's plaintext reaches disk |
| manifest `mode` → file permissions | An attacker-chosen integer that could make a credential world-readable |
| destination directory → other local users | Anything readable between creation and chmod is readable by everyone |
| an interrupted process → what survives on disk | A kill leaves whatever the last completed step left |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|-----------|----------|-----------|----------|-------------|-----------------|
| T-5-30 | Information disclosure | plaintext at a path outside the destination | critical | mitigate | `NamedTempFile::new_in(parent)` only; `temp_dir` and `/tmp` appear nowhere, enforced by a grep in the phase gate as well as by review |
| T-5-31 | Information disclosure | a world-readable window before chmod | critical | mitigate | Mode 0600 is set on the tempfile **before** any content is written, and `persist` preserves it, so no readable window exists at either name |
| T-5-32 | Information disclosure | a credential inheriting the manifest's recorded mode | critical | mitigate | The manifest's `mode` field is never read; files are 0600 and directories 0700 unconditionally, a narrowing that is safe in the only direction that matters |
| T-5-33 | Information disclosure | a tempfile born in a world-listable directory | high | mitigate | Directories the restore creates are chmod 0700 **before** the tempfile is created |
| T-5-34 | Tampering | a half-written credential at its real name | critical | mitigate | The real name is only ever reached by `persist`, which is a rename; there is no path that opens the destination for writing |
| T-5-35 | Information disclosure | plaintext surviving an interrupted restore | high | mitigate | `NamedTempFile`'s `Drop` removes the partial output on every error path, asserted by injecting a failure at each step and walking both the roots and `TMPDIR` |
| T-5-36 | Tampering | a plan mutated between planning and applying | medium | mitigate | `layout::from_manifest_path` is re-run at the write boundary and a disagreement aborts the restore |
| T-5-37 | Denial of service | a multi-gigabyte transcript assembled in RAM | medium | mitigate | Chunks are written as they arrive; no whole-file buffer is accumulated |
| T-5-SC | Tampering | npm/pip/cargo installs | high | mitigate | No new crates — `FileTimes` is stdlib; `cargo machete` runs in the phase gate |
</threat_model>

<verification>
- `cargo test --lib sync::restore::write` green.
- `grep -v '^\s*//' src/sync/restore/write.rs | grep -c 'temp_dir\|"/tmp"'` is 0.
- `grep -v '^\s*//' src/sync/restore/write.rs | grep -c 'into_temp_path'` is 0.
- `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` clean.
- `HOME= cargo test --lib sync::restore::write` passes.
</verification>

<success_criteria>
1. Plaintext exists only inside its own destination directory, at mode 0600, at every instant.
2. The manifest's recorded mode is never applied.
3. An injected failure at any step leaves no tempfile and an untouched destination name.
4. Restored files carry the snapshot's mtime.
5. A partial restore is reported as one, with the item it stopped at named.
</success_criteria>

<output>
Create `.planning/phases/05-pull-and-restore/5-04-SUMMARY.md` when done, recording the exact write
ordering and the `Applied` shape 5-06 renders.
</output>
