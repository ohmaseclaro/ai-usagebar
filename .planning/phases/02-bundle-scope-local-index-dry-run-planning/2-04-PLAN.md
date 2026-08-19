---
phase: 2-bundle-scope-local-index-dry-run-planning
plan: 04
type: execute
wave: 2
depends_on: ["2-01"]
files_modified:
  - src/sync/transcripts.rs
autonomous: true
requirements: [SCOPE-01, SCOPE-02, SCOPE-03]
user_setup: []

must_haves:
  truths:
    - "Transcripts are absent from the default category set, so an unconfigured user syncs none of them."
    - "With transcripts enabled, a .jsonl older than transcript_days is excluded and a newer one is included."
    - "With transcripts enabled, selection stops once transcript_max_bytes would be exceeded, newest first."
    - "Whichever of the two bounds binds first wins, proven in both directions."
    - "Selection is always per whole file; no partial file is ever selected."
    - "The scan reports how many files and bytes the bounds excluded, so the user can see what was left behind."
  artifacts:
    - src/sync/transcripts.rs
  key_links:
    - "`collect_bounded` is reached only through `scope::collect`'s Transcripts arm, so D2 exclusions and the symlink guard apply unchanged."
    - "Both bounds read from `SyncConfig`, not from constants, so a user can widen or tighten them without a rebuild."
---

<objective>
Implement the opt-in transcripts category and its D3 bounding: 30 days and 2 GiB, both
applied, newest first, whichever binds first winning — and always per whole file.

Purpose: this machine holds 4.0 GB across 4110 `.jsonl` files. Unbounded, that category does
not fit the remote and would need a second code path. Bounded, it fits and stays one path.
A truncated JSONL is worse than an absent one, so the bound selects files, never offsets.
Output: `src/sync/transcripts.rs` complete; the fifth category is real.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/2-bundle-scope-local-index-dry-run-planning/2-CONTEXT.md
@.planning/phases/2-bundle-scope-local-index-dry-run-planning/2-01-SUMMARY.md
@CLAUDE.md
@src/context/mod.rs
</context>

<tasks>

<task type="auto" tdd="true">
  <name>Task 1: the bounded transcripts collector</name>
  <files>src/sync/transcripts.rs</files>
  <read_first>src/context/mod.rs lines 104-140 (`default_projects_path` and `scan_dir` — the existing bounded, hermetic reader over this exact tree, including its newest-first sort and its cap) and lines 142-208 (`discover`).</read_first>
  <behavior>
    - Transcripts disabled: `collect_bounded` returns an empty scan and performs no directory read.
    - Enabled, one .jsonl with mtime one day old and transcript_days 30: included.
    - Enabled, one .jsonl with mtime 31 days old and transcript_days 30: excluded, and counted in the excluded totals.
    - Enabled, three files of 100 bytes each with transcript_max_bytes 250: the two newest are selected, the oldest is excluded, and the selected total does not exceed the budget.
    - A file larger than transcript_max_bytes on its own is excluded entirely rather than partially selected.
    - The age bound binding first and the byte bound binding first are two separate assertions.
    - A non-.jsonl file under the projects tree is not selected.
    - A missing projects root yields an empty scan, not an error.
  </behavior>
  <action>
Fill in the body of `pub fn collect_bounded(roots: &SyncRoots, cfg: &SyncConfig, now: DateTime&lt;Utc&gt;) -> CategoryScan`
in src/sync/transcripts.rs. Plan 2-01 already declared this exact signature, already wired the
`scope::collect` Transcripts arm to it, and already added `excluded_files` / `excluded_bytes`
to `CategoryScan`. **Edit no file but src/sync/transcripts.rs** — plan 2-02 owns scope.rs in
this same wave and both run in isolated worktrees. If something genuinely required is missing
from scope.rs, stop and report it rather than editing across the boundary.

The `now` argument is the point: bounding is time-dependent logic, and the project's rule is
that such logic takes `now` as an argument so no test reads the wall clock.

Return `CategoryScan::empty` immediately when `!cfg.includes(SyncCategory::Transcripts)` — the
default per SCOPE-02 and D1, so the common case costs no directory read at all.

When enabled, walk `roots.claude_home.join("projects")` through plan 2-01's `walk`, keeping
only regular files with a `.jsonl` extension. Then apply both D3 bounds to the resulting
`FileEntry` list:

1. Sort newest-first by mtime, tie-breaking on path so the selection is deterministic across
   runs — a non-deterministic selection would make two consecutive dry-runs disagree.
2. Drop every entry older than `cfg.transcript_days` before `now`.
3. Accumulate in that order, stopping before any entry that would push the running total past
   `cfg.transcript_max_bytes`. Stop at that point rather than skipping the oversized entry and
   continuing with smaller ones: a newest-first budget that back-fills older files is not the
   working window the user asked for.

Selection is per file. There is no partial-file path in this function and none is to be added
— a truncated JSONL restores as a corrupt transcript, which is worse than the file being
absent.

Populate `excluded_files` and `excluded_bytes` with everything the two bounds left behind, so
`sync status` can tell the user how much of their 4.0 GB archive is outside the working
window rather than leaving them to infer it.

Reuse `context::scan_dir`'s conventions but not its body: that scanner reads bounded tails to
extract usage, which this does not need. Cost here is one `stat` per file across 4110 files.
Read no file body.
  </action>
  <verify>
    <automated>cargo test --lib sync::transcripts</automated>
  </verify>
  <done>Every bullet in `&lt;behavior&gt;` has a passing test that seeds its tree under a `TempDir`, sets mtimes explicitly, and passes a fixed `now`. No test reads the wall clock and none reads the real `~/.claude/projects`.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| Claude Code transcript tree → collector | 4110 files of undocumented, schema-tolerant JSONL containing full conversation text cross into a set destined for another machine. |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|-----------|----------|-----------|----------|-------------|-----------------|
| T-2-16 | Information disclosure | default category set | high | mitigate | Transcripts are absent from the D6 default set, so conversation content is never carried without an explicit opt-in. Asserted by a test on the default config. |
| T-2-17 | Denial of service | unbounded selection | high | mitigate | Both D3 bounds are applied on every run; 4.0 GB unbounded does not fit the remote. The bounds are config values, so a user can tighten them without a rebuild. |
| T-2-18 | Tampering | partial-file selection | medium | mitigate | Selection is per whole file. No offset-based path exists, so a bound can never produce a truncated transcript that restores as corrupt. |
| T-2-19 | Information disclosure | symlinked transcript tree | high | mitigate | Collection funnels through plan 2-01's `walk`, whose symlink guard is already tested; this plan adds no second traversal. |
| T-2-SC | Tampering | npm/pip/cargo installs | high | accept | This plan adds no dependency. `cargo machete` runs in the phase-end gate. |
</threat_model>

<verification>
`cargo test --lib sync::transcripts` is green. Every test injects roots via `SyncRoots::at`
over a `TempDir` and passes an explicit `now`.
</verification>

<success_criteria>
Transcripts are off by default; when enabled they are bounded by 30 days and 2 GiB, both
applied newest-first, per whole file, with the excluded remainder reported.
</success_criteria>

<output>
Create `.planning/phases/2-bundle-scope-local-index-dry-run-planning/2-04-SUMMARY.md` when done.
Record the measured shape of the bounds on real data if you have it — how many of the 4110
files land inside 30 days — since plan 2-06's CAL-4 quotes a bundle size to the user.
</output>
