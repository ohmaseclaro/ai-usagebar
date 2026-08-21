---
phase: 05-pull-and-restore
plan: 07
type: execute
wave: 3
depends_on: ["5-01", "5-02", "5-03", "5-04", "5-05", "5-06"]
files_modified:
  - src/sync/cli.rs
  - src/sync/index.rs
  - src/sync/plan.rs
autonomous: true
requirements: [UX-01, SAFE-03, SAFE-04, SYNC-06]
must_haves:
  truths:
    - "`sync pull` with no flags plans, reports, and exits 0 having written nothing; `--apply` runs the gate, takes the backup, writes, and prints the summary (UX-01, D1)."
    - "On a TTY without `--apply`, the one gate is offered and an affirmative proceeds — so `--apply` and an interactive confirm are the two ways to write, and there is no third."
    - "`--force` promotes locally-newer non-credential items; a locally-newer credential still stops at the second gate, which only `--force-credentials` or an explicit answer passes (SAFE-03, D2)."
    - "`--rebuild-index` discards the local SQLite index and reopens it empty; `--force-rehash` ignores every cached record for one run. Both degrade a sync to slow, never to data loss."
    - "A lost or corrupt index never changes what a pull writes — the restore side already hashes what is on disk — and a test asserts an identical pull outcome with the index deleted."
    - "Every failure path exits non-zero with one actionable message, and none is reported as success."
    - "No test reads a real `$HOME`, a real token, or opens a socket outside the injected `Endpoints` base."
  artifacts:
    - "`SyncAction::Pull` fully wired in `src/sync/cli.rs`, both gates included"
    - "`index::reset_at` and the `force_rehash` path through `plan::build_with_keys`"
  key_links:
    - "the flags were frozen by 5-01; this plan wires behaviour and declares nothing new, so no spelling changes under 5-06's feet"
    - "the credential gate runs **after** the apply gate and **before** the backup, so a user who stops at it has had nothing written and no archive taken"
    - "`--rebuild-index`/`--force-rehash` are push-side recovery reachable from the pull command because that is where a user lands after a machine loss; they change no restore behaviour"
---

<objective>
The command. Wire every frozen flag onto the five filled modules, in the order the safety
properties require, and add the two index-recovery escapes so a lost SQLite index degrades a sync
to slow rather than to data loss.

Purpose: UX-01 completes here — both directions exist, both with `--dry-run`.

Output: `sync pull` in its shipping shape, plus `--rebuild-index` and `--force-rehash`.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/05-pull-and-restore/5-CONTEXT.md
@.planning/phases/05-pull-and-restore/5-01-SUMMARY.md
@.planning/phases/05-pull-and-restore/5-03-SUMMARY.md
@.planning/phases/05-pull-and-restore/5-05-SUMMARY.md
@.planning/phases/05-pull-and-restore/5-06-SUMMARY.md
@CLAUDE.md
@src/sync/cli.rs
@src/sync/index.rs
@src/sync/plan.rs
@src/sync/restore/mod.rs
@src/widget/cli.rs
</context>

<tasks>

<task type="auto" tdd="true">
  <name>Task 1: `sync pull` wired, in the order the safety properties require</name>
  <files>src/sync/cli.rs</files>
  <behavior>
    - No flags, TTY: plan, report, gate offered; an affirmative applies and a refusal exits 0 having written nothing.
    - No flags, no TTY: plan, report, exit 0, nothing written, and the message names `--apply`.
    - `--apply`, no TTY: applies without a gate.
    - `--force` with a locally-newer routine: overwritten, and the summary names it.
    - `--force` with a locally-newer credential, no TTY, no `--force-credentials`: exits non-zero, names the credential, writes nothing, and takes no backup.
    - `--force --force-credentials`: overwrites it, and the summary names it.
    - `--allow-rollback` with a lower counter: proceeds; without it: exits non-zero naming the flag; with a mismatched `repo_id`: exits non-zero **even with** the flag.
    - Wrong passphrase, unreachable remote, no pointer, and a tampered pack each exit non-zero with a distinct message and write zero files.
    - The order gate → credential gate → backup → write is asserted, so a user who stops at the credential gate has no archive and no writes.
  </behavior>
  <action>
Replace 5-01's tracer arm in `src/sync/cli.rs` with the full `SyncAction::Pull` wiring, in both
`run` and Phase 3's injectable `run_with`.

Map flags onto `RestoreOptions` one for one — the struct was frozen by 5-01 precisely so this is a
transcription and not a design. `--dry-run` maps to nothing, because dry-run is the absence of
`apply`; accept the flag for symmetry with push and for UX-01's wording, and make it a clap
conflict with `--apply` so a user who passes both gets an error instead of a guess.

Sequence, and each position is load-bearing:

1. `restore::run` with `apply: false` — always. Every path starts by planning, including
   `--apply`, because the report is what the gates and the summary are built from.
2. `report::render_plan` to stdout.
3. If `apply` is unset: offer `report::confirm_apply` when `stdout().is_terminal()`, using the same
   `IsTerminal` check `sync/cli.rs` already makes; otherwise print the "pass --apply" line and
   return 0. A dry run is a success, not a failure — it did exactly what it was asked.
4. If the plan holds `NeedsCredentialConfirm` items, run `report::confirm_credentials` **now**,
   before anything is written and before the backup exists. A user who declines here has cost the
   machine nothing at all. Set `force_credentials` from the answer and re-plan the affected items
   rather than mutating dispositions in place — re-planning is cheap, and hand-editing a plan is
   how a disposition and its reason drift apart.
5. `restore::run` again with `apply: true`. The backup and the writes happen inside it, in the
   order `restore/mod.rs` froze.
6. `report::render_outcome`.

Exit codes follow the module's existing shape: 0 for a completed dry run or a completed apply, 1
for any error, message once to stderr. A declined gate is 0 — the user made a choice and the tool
honoured it.

Passphrase comes through the existing `keys_at`/`keyfile_path` pair. Do not add a flag, do not read
an env var. The keyfile for a pull comes from the **remote**, not from disk, so the local path is
used only for the passphrase prompt convention; make that explicit in a comment, because it is the
one place where push and pull differ in where the keyfile lives.
  </action>
  <verify>
    <automated>cargo test --lib sync::cli</automated>
  </verify>
  <done>`cargo test --lib sync::cli` is green. Every behaviour above has a test driven through `run_with` against mockito with injected roots. The gate → credential gate → backup → write order is asserted. A declined credential gate leaves no archive and no writes. `--allow-rollback` does not rescue a `repo_id` mismatch. Exit codes are 0 for a completed or declined run and 1 for every error.</done>
</task>

<task type="auto" tdd="true">
  <name>Task 2: Index recovery — `--rebuild-index` and `--force-rehash`</name>
  <files>src/sync/index.rs, src/sync/plan.rs</files>
  <behavior>
    - `index::reset_at` on an existing index removes the file and returns a fresh empty one at the same path, mode 0600.
    - `reset_at` on a path that does not exist is not an error — it is the same outcome.
    - `reset_at` refuses a path that is a directory, naming it, rather than removing anything recursively.
    - A plan built with `force_rehash` opens every file and matches a plan built against a deleted index, byte for byte — asserted through the existing `files_opened` counter, not by timing.
    - A plan built without it against a warm index opens zero files, unchanged from Phase 2's behaviour.
    - A pull run with the index deleted produces an identical `RestoreOutcome` to one with a warm index, proving the index never influences what restore writes.
    - `--rebuild-index` and `--force-rehash` together are accepted and mean what they say separately.
  </behavior>
  <action>
Add `pub fn reset_at(path: &Path) -> Result<Index>` to `src/sync/index.rs`: refuse a directory,
remove the file if present, then `Index::at(path)`, which already creates the schema at mode 0600.
Do not `DROP TABLE` inside the existing connection — a file that is corrupt enough to need this
escape is a file SQLite may not be able to open, and the recovery path must not depend on the thing
it is recovering from. Say that in the doc comment. `Index::at` already degrades a corrupt file to a
rescan and sets `was_rebuilt`; this is the explicit, user-invoked version of the same idea and the
doc comment should point at the automatic one.

Add a `force_rehash: bool` to the plan builder's inputs in `src/sync/plan.rs`, threaded to the one
place that consults `index.lookup`/`index.cached`, making it return `None` for the run. That is the
entire mechanism: skipping the cache makes pass one fall through to pass two, and pass two is
already the correct slow path. Do not add a second traversal, and do not clear the index — a
`--force-rehash` that also destroyed the cache would make the flag more destructive than its name.

Wire both onto the `RestoreOptions` fields 5-01 froze from within `sync/cli.rs`'s pull arm; the
options struct already carries them. Note in a comment why two push-side recovery flags live on the
pull command: a user reaches for them after losing a machine, and after a machine loss the command
they are running is `pull`. `sync push` gains the same two flags for symmetry if it does not already
have them, and that is a one-line clap addition, not a second implementation.
  </action>
  <verify>
    <automated>cargo test --lib -- sync::index sync::plan</automated>
  </verify>
  <done>`cargo test --lib -- sync::index sync::plan` is green. `reset_at` handles present, absent, and directory cases and yields a mode-0600 index. `force_rehash` reproduces a deleted-index plan exactly, asserted through `files_opened`. A pull with the index deleted produces an identical outcome to one with a warm index. `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, and `cargo machete` are clean.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| CLI flags → destructive writes | `--force` and `--force-credentials` authorise loss |
| a gate answer → the write path | The only consent the tool receives |
| the local SQLite index → sync correctness | A cache that must never be able to cause data loss |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|-----------|----------|-----------|----------|-------------|-----------------|
| T-5-60 | Elevation of privilege | `--force` overwriting a live credential | critical | mitigate | The credential gate runs before the backup and before any write, and `--force` does not answer it; only `--force-credentials` or an explicit typed answer does |
| T-5-61 | Repudiation | a declined gate that still left artifacts | high | mitigate | The credential gate precedes `backup::take`, so a decline leaves no archive and no writes, asserted by a test that walks the roots and the backups directory afterwards |
| T-5-62 | Spoofing | `--allow-rollback` used to import another bundle's counter | high | mitigate | The flag is passed to `anchor::accept`, whose `repo_id`-mismatch arm errors regardless; the CLI adds no bypass, asserted by a test |
| T-5-63 | Tampering | a corrupt index deciding what restore writes | high | mitigate | Restore hashes what is on disk and never consults the index; a test asserts an identical outcome with the index deleted |
| T-5-64 | Denial of service | a recovery path that needs the corrupt file to open | medium | mitigate | `reset_at` removes the file rather than issuing SQL against it |
| T-5-65 | Tampering | `--force-rehash` destroying the cache | medium | mitigate | The flag suppresses cache reads for one run and clears nothing; a test asserts the index still holds its rows afterwards |
| T-5-66 | Information disclosure | a passphrase reaching argv or an env var | critical | mitigate | No `--password` flag and no env fallback; the existing TTY/stdin/mode-0600-file path is reused unchanged |
| T-5-67 | Repudiation | a failure reported as success | high | mitigate | Every error path returns 1 with one message to stderr; a declined gate returns 0 because it is a completed choice, and both are asserted |
| T-5-SC | Tampering | npm/pip/cargo installs | high | mitigate | No new crates; `cargo machete` runs in this plan's gate |
</threat_model>

<verification>
- `cargo test --lib -- sync::cli sync::index sync::plan` green.
- `grep -rn "password" src/widget/cli.rs | grep -c "Pull"` is 0.
- `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, `cargo machete` clean.
- `HOME= cargo test --lib sync` passes.
</verification>

<success_criteria>
1. `sync pull` writes nothing without `--apply` or an interactive affirmative.
2. `--force` never overwrites a locally-newer credential on its own.
3. The credential gate precedes the backup, which precedes the first write.
4. `--rebuild-index` and `--force-rehash` degrade a sync to slow and never to data loss.
5. Every failure exits non-zero with one actionable message.
</success_criteria>

<output>
Create `.planning/phases/05-pull-and-restore/5-07-SUMMARY.md` when done, recording the final flag
set, the exit-code table, and the gate ordering plan 5-08 asserts end to end.
</output>
