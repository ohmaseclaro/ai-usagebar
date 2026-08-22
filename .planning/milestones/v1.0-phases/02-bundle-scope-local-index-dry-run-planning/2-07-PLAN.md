---
phase: 2-bundle-scope-local-index-dry-run-planning
plan: 07
type: execute
wave: 4
depends_on: ["2-05"]
files_modified:
  - src/sync/report.rs
  - src/sync/cli.rs
  - src/sync/plan.rs
  - src/widget/cli.rs
autonomous: true
requires_phase_1_merged: true
requirements: [SCOPE-03, SCOPE-04, UX-02]
user_setup: []

must_haves:
  truths:
    - "`ai-usagebar sync push --dry-run` prints, per category, file count, raw bytes, and bytes that would actually upload."
    - "It prints totals and the resulting snapshot size."
    - "It creates no file anywhere outside the index and makes no network call."
    - "`sync status` shows the same would-change figures plus the last-sync time."
    - "A second dry-run over an unchanged tree reports zero bytes to upload."
    - "The dry-run uses Phase 1's real chunk-id function, so the byte figure is the one a push would actually send."
  artifacts:
    - src/sync/report.rs
  key_links:
    - "This is the single call site where Phase 1's keyed chunk-id function is supplied to plan 2-05's `build`."
    - "`CHUNK_BYTES` becomes a re-export of Phase 1's constant here, so the two modules cannot drift."
---

<objective>
Deliver the phase's user-facing surface: `ai-usagebar sync push --dry-run` in D4's shape, the
same figures folded into `sync status`, and the one line that wires Phase 1's real chunker
into plan 2-05's planner.

Purpose: the would-upload column is the number that matters for the user's fast-and-light
goal, and it is what makes SYNC-02's near-zero no-op *visible* rather than merely claimed.
Output: both commands complete; nothing in this phase transmits anything.

**Phase 1 coupling:** this is the only plan in the phase that needs Phase 1's concrete code.
Sequence it after Phase 1 merges.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/02-bundle-scope-local-index-dry-run-planning/2-CONTEXT.md
@.planning/phases/01-encrypted-bundle-core/1-CONTEXT.md
@.planning/phases/02-bundle-scope-local-index-dry-run-planning/2-05-SUMMARY.md
@.planning/phases/02-bundle-scope-local-index-dry-run-planning/2-01-SUMMARY.md
@CLAUDE.md
</context>

<tasks>

<task type="auto" tdd="true">
  <name>Task 1: wire Phase 1's chunker and render D4's dry-run</name>
  <files>src/sync/plan.rs, src/sync/report.rs</files>
  <read_first>The Phase 1 summaries for the crypto and chunk modules — specifically the exact signatures for deriving `name_key` from the master key and for `chunk_id`, and Phase 1's fixed chunk-size constant. Do not guess these; read them.</read_first>
  <behavior>
    - A dry-run report over a seeded tree renders one row per category with file count, raw bytes and would-upload bytes.
    - A disabled category renders as off in all three columns, not as three zeros.
    - The transcripts row, when enabled and bounded, also reports how many files and bytes the D3 bounds excluded.
    - Totals row sums the three columns across enabled categories only.
    - A snapshot-size line follows the totals.
    - A plan with nothing to upload renders a zero would-upload total and says plainly that a push would send nothing.
    - When the index was rebuilt this run, the report says so, since that explains a slow first run.
    - Rendering is pure — it takes a `SyncPlan` and returns a `String`, with no filesystem access.
  </behavior>
  <action>
In src/sync/plan.rs replace the locally-declared `CHUNK_BYTES` with a re-export of Phase 1's
own chunk-size constant. Plan 2-05 duplicated it for exactly one wave so it could be built
against the contract while Phase 1 was still landing; leaving two constants alive is how the
chunker and the planner silently disagree later.

Add `pub fn build_with_master_key(roots, cfg, index, now, master_key) -> Result<SyncPlan>` to
plan.rs: derive `name_key` per Phase 1's key hierarchy and call the existing generic `build`
with a closure capturing it. This is the single call site that binds the two phases; the
generic `build` stays as-is so its tests keep running with a toy hasher and no key material.
The closure is why `build` is generic rather than taking a fn pointer — the key has to be
captured. Do not let a key, a passphrase or a chunk plaintext reach a `Debug` impl, an error
message or a log line.

In src/sync/report.rs add `pub struct DryRunReport { pub plan: SyncPlan, pub index_rebuilt:
bool }` and `pub fn render_dry_run(&DryRunReport) -> String`, in D4's shape: per category a
file count, raw bytes, and bytes that would actually upload — new chunks only; then totals;
then the resulting snapshot size. Reuse the `human_bytes` helper and the column layout
`render_status` already established rather than inventing a second table style.

Extend `StatusReport` and `render_status` to carry the same would-upload column plus the
last-sync time, so UX-02's "what would change now" is answered by the same numbers as the
dry-run. `build_status` gains an optional `SyncPlan`; with none it renders the counts-only
form plan 2-01 shipped, so status still works before the first plan is ever built.
  </action>
  <verify>
    <automated>cargo test --lib sync::report sync::plan</automated>
  </verify>
  <done>Every bullet in `&lt;behavior&gt;` has a passing test over a constructed `SyncPlan` — the renderer tests need no filesystem at all. One integration-style test seeds a `TempDir`, runs `build_with_master_key` with a test master key and cheap KDF parameters, and asserts the rendered totals match the plan's fields.</done>
</task>

<task type="auto" tdd="true">
  <name>Task 2: `sync push --dry-run` on the CLI</name>
  <files>src/sync/cli.rs, src/widget/cli.rs</files>
  <read_first>src/widget/cli.rs — the `SyncAction` enum plan 2-01 added, and the surrounding subcommand doc-comment style.</read_first>
  <behavior>
    - `sync push --dry-run` parses and reaches the dry-run path.
    - `sync push` without `--dry-run` exits non-zero with a message saying the push transport arrives in a later phase — never a silent success and never a partial attempt.
    - The dry-run path creates no file outside the local index and opens no socket.
    - A dry-run with no password available still produces per-category counts and raw bytes, and says why the would-upload column is unavailable rather than printing a wrong zero.
  </behavior>
  <action>
Add `Push { #[arg(long)] dry_run: bool }` to `SyncAction` in src/widget/cli.rs.

In src/sync/cli.rs handle `SyncAction::Push { dry_run: true }`: load config, resolve roots,
open the index, obtain the master key through Phase 1's passphrase path, build the plan, print
`render_dry_run`, return 0. Handle `dry_run: false` by returning a non-zero exit with a message
naming the phase that adds the transport. This phase is offline; there must be no code path
here that could attempt a network call, and none that half-executes a push.

Obtaining the key means an Argon2id derivation at m = 1 GiB, which is a real wait on modest
hardware. Surface it as a deliberate "deriving key…" line before it starts, as Phase 1's UX
decision requires — a dry-run that appears frozen for a second and a half reads as a hang.

If no passphrase is available — non-interactive, or none configured — do not fail the whole
command. Print the per-category file counts and raw bytes, which need no key at all, and state
that the would-upload column needs the sync password. The counts are the half of SCOPE-04 that
is always answerable, and a user checking what is in scope should not have to authenticate.

Update `SyncAction::Status` to build a plan too when a key is available, passing it to
`build_status`, so UX-02 reports what would change now.
  </action>
  <verify>
    <automated>cargo test --lib sync::cli widget::cli</automated>
  </verify>
  <done>A clap parse test asserts `sync push --dry-run` and `sync status` both parse to the right variants. A test asserts the no-key dry-run path still renders counts and raw bytes and names the missing password. `cargo test --lib sync` is green.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| passphrase → key derivation | The sync password enters the process to derive the key that names chunks. |
| plan → terminal | File counts and byte totals derived from credential trees cross into a terminal that may be logged or shared. |
| this phase → network | The boundary that must not be crossed at all: `--dry-run` measures a push without performing one. |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|-----------|----------|-----------|----------|-------------|-----------------|
| T-2-29 | Information disclosure | key handling in `cli::run` | critical | mitigate | The master key and the derived `name_key` are held in Phase 1's zeroizing types, are never formatted into an error or a log line, and never reach a `Debug` impl. The password arrives through Phase 1's TTY/stdin/mode-0600-file path — never argv, never an env var. |
| T-2-30 | Information disclosure | dry-run output | high | mitigate | The report prints category names, counts and byte totals. No file path, no chunk id, and no file content is rendered. |
| T-2-31 | Spoofing | `sync push` without `--dry-run` | high | mitigate | Returns non-zero with an actionable message. No transport code exists in this phase, so there is nothing to half-execute; the private-repo gate that must precede any upload lands in Phase 3. |
| T-2-32 | Tampering | a wrong would-upload figure | medium | mitigate | The figure comes from Phase 1's real chunk-id function through the single `build_with_master_key` call site, and `CHUNK_BYTES` is re-exported from Phase 1 rather than duplicated, so the number the user sees is the number a push would send. |
| T-2-33 | Denial of service | Argon2id at m = 1 GiB | medium | accept | A deliberate cost, announced before it starts. Phase 1 owns the parameters and their low-RAM refusal. |
| T-2-SC | Tampering | npm/pip/cargo installs | high | accept | This plan adds no dependency; every crate it uses arrived with Phase 1. `cargo machete` runs in the phase-end gate. |
</threat_model>

<verification>
`cargo test --lib sync` is green. Renderer tests touch no filesystem; CLI tests inject their
roots. No test and no production path in `src/sync/` opens a socket.
</verification>

<success_criteria>
`sync push --dry-run` prints per-category file counts, raw bytes and would-upload bytes, plus
totals and the snapshot size, and creates nothing outside the injected roots and the index. A
second dry-run over an unchanged tree reports zero bytes to upload. `sync status` reports the
same figures plus last-sync.
</success_criteria>

<output>
Create `.planning/phases/02-bundle-scope-local-index-dry-run-planning/2-07-SUMMARY.md` when done.
Record the observed would-upload total for the real default bundle if you ran it against this
machine, and compare it to plan 2-06's CAL-4 figure — a large disagreement means one of the two
is measuring the wrong thing.
</output>
