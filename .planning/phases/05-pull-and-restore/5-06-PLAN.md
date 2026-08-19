---
phase: 05-pull-and-restore
plan: 06
type: execute
wave: 2
depends_on: ["5-01"]
files_modified:
  - src/sync/restore/report.rs
autonomous: true
requirements: [SYNC-06, SAFE-03, UX-01]
must_haves:
  truths:
    - "A dry-run report lists every item that would be created, updated, or skipped, with the reason for each skip, grouped by category and bounded so a 5,000-file bundle does not print 5,000 lines (D1, UX-01)."
    - "Locally-newer items are surfaced **before** the gate, in their own block, because SAFE-03's requirement is that the user is told what would change first."
    - "There is exactly one interactive gate, at the start, and the detail lands in the summary afterwards — never a per-item prompt (D6)."
    - "The post-restore summary names every overwritten item, so SYNC-06's \"the user is told what was overwritten\" is satisfied by a list, not a count."
    - "The rollback command from the backup record is printed on both the success and the partial-failure path, because the partial path is when it is needed."
    - "The credential confirmation is a second, separate gate with its own wording that names the OAuth-token failure mode, and it is never auto-answered by `--yes` alone."
    - "Rendering is pure: the gate takes a reader and a writer, so no test needs a TTY and no test prompts."
  artifacts:
    - src/sync/restore/report.rs — `render_plan`, `render_outcome`, the two gates, and the line budget
  key_links:
    - "this is the only module in `restore/` that prints or reads from a terminal; the other five return values"
    - "the report renders `Disposition` variants exhaustively, so a new variant is a compile error rather than a silently unrendered case"
    - "`IsTerminal` decides interactivity, exactly as `sync/cli.rs` and `report.rs` already do; a non-TTY run with no `--apply` prints and exits, and with `--apply` needs `--yes`"
---

<objective>
The report. One gate at the start, a per-item table that a human can read, and an afterwards
summary that names what was lost.

Purpose: D1 and D6. A restore that asks 200 questions is a restore nobody finishes; a restore that
overwrites 200 files without naming them is a restore nobody trusts. This module is the difference.

Output: `src/sync/restore/report.rs`, filled.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/05-pull-and-restore/5-CONTEXT.md
@.planning/phases/05-pull-and-restore/5-01-SUMMARY.md
@CLAUDE.md
@src/sync/report.rs
@src/sync/cli.rs
@src/sync/restore/mod.rs
</context>

<tasks>

<task type="auto" tdd="true">
  <name>Task 1: `render_plan` — what would change, grouped, bounded, and honest about the skips</name>
  <files>src/sync/restore/report.rs</files>
  <behavior>
    - A plan with one item of every disposition renders one line for each, and the `match` over `Disposition` is exhaustive with no wildcard arm — asserted by a test that would fail to compile if a variant were added and unhandled.
    - Locally-newer items render in their own leading block with both timestamps, before the per-category table.
    - Rejected and excluded items render with their reason, so a tampered bundle is visible rather than absent.
    - Per category, above a line budget the body collapses to a count plus the first few paths and a "and N more" line; a 5,000-item plan renders under a bounded number of lines, asserted as a line count.
    - The headline states counts and the bytes that would be fetched, and states plainly that nothing was written.
    - Byte counts render through the crate's existing size formatting, not a second one.
    - A plan with zero writable items renders "already up to date" and says so without implying a failure.
  </behavior>
  <action>
Fill `pub fn render_plan(plan: &RestorePlan) -> String`, following the shape
`src/sync/report.rs`'s `render_status` and `render_dry_run` already established — same
category ordering (`SyncCategory::ALL`), same byte formatting helper, same tone.

Structure, top to bottom:

1. A headline: the snapshot's counter and `created_at`, the counts by disposition, and
   `bytes_to_fetch`. It ends with the sentence that nothing has been written.
2. **The locally-newer block, first.** Every `SkipLocalNewer` and `NeedsCredentialConfirm` item,
   with its path, its local mtime, and the snapshot's time. This block leads because SAFE-03's
   wording is that the user is told what would change *first* — putting it after a 200-line table
   satisfies the letter and not the requirement.
3. The refusals: `RejectedPath` and `ExcludedByPolicy` with their reasons. Small, and usually
   empty; when it is not empty it is the most interesting thing on the screen.
4. The per-category table: created, updated, identical-skipped, counts and bytes.
5. The per-item detail, per category, under a line budget.

The line budget is the difference between a report and a wall. Set `const MAX_ITEM_LINES_PER_CATEGORY`
to a small number, print the first that many paths, then a single "and N more" line. The
locally-newer and refusal blocks get their own, larger budget — those are the lines the user
actually has to read, and truncating them to the same limit as a list of ordinary creates would
hide the important half. Say that in a comment so a later tidy-up does not unify the two budgets.

`match` over `Disposition` exhaustively, with no `_` arm anywhere in the file. A new disposition
should break the build here; a wildcard would render it as nothing and the user would never know
it existed.
  </action>
  <verify>
    <automated>cargo test --lib sync::restore::report</automated>
  </verify>
  <done>`cargo test --lib sync::restore::report` is green. Every disposition renders, the match is exhaustive with no wildcard, locally-newer and refusals lead the report, and a 5,000-item plan renders under the asserted line ceiling. Category order and byte formatting match `sync/report.rs`. `grep` finds no `_ =>` in the file.</done>
</task>

<task type="auto" tdd="true">
  <name>Task 2: One gate, a second one for credentials, and the summary that names what was lost</name>
  <files>src/sync/restore/report.rs</files>
  <behavior>
    - `confirm_apply` writes the plan and a single question to its writer and accepts only an explicit affirmative from its reader; anything else, including empty input and EOF, is a refusal.
    - With `assume_yes` set it returns true without writing a question; with a non-interactive writer and no `assume_yes` it returns false and the message says which flag to pass.
    - `confirm_credentials` is a **separate** gate, reached only when the plan holds a `NeedsCredentialConfirm` item, listing those items by path and naming the stale-OAuth-token failure mode in one sentence.
    - `--yes` does not answer `confirm_credentials`; only `force_credentials` does, and a test asserts that `assume_yes` alone leaves it refusing.
    - There is no third prompt anywhere in the module: a test greps the file for read calls and asserts exactly two gate functions exist.
    - `render_outcome` lists every overwritten path by name, prints the backup archive and its rollback command, and on a partial failure names the item the restore stopped at and prints the rollback command again.
    - `render_outcome` on a dry run renders the plan and nothing about writes.
  </behavior>
  <action>
Add the two gates and the summary.

`pub fn confirm_apply(plan: &RestorePlan, opts: &RestoreOptions, out: &mut dyn Write, input: &mut dyn BufRead) -> Result<bool>`
takes its reader and writer, so no test needs a TTY and no test hangs. `assume_yes` short-circuits
to true. Interactivity is decided by the caller through `IsTerminal`, exactly as `sync/cli.rs`
already does; a non-interactive run without `--apply`/`--yes` returns false with a message naming
the flag rather than blocking on a stdin that will never arrive. Accept only an explicit
affirmative — a bare newline is a refusal, EOF is a refusal. This is the one gate D6 allows, and it
comes after `render_plan` so the user answers it having seen everything.

`pub fn confirm_credentials(items: &[&ItemPlan], out: &mut dyn Write, input: &mut dyn BufRead) -> Result<bool>`
is the second explicit confirmation D2 requires, and it is not covered by `--yes`. Its wording
names the actual failure mode in one sentence — that the local copy is newer and overwriting it can
put a stale OAuth token back over a live rotated one — because a confirmation that does not say
what is at stake is a keystroke, not a decision. Do not make it a `[y/N]`; require the word.

`pub fn render_outcome(outcome: &RestoreOutcome) -> String` is the afterwards. On an applied run:
counts written and skipped, then **every overwritten path by name** — SYNC-06 asks that the user be
told what was overwritten, and a count does not tell anyone anything — then the backup archive path
and its rollback command. On a partial failure the same, plus the manifest path the restore stopped
at, and the rollback command again at the bottom, because the bottom of the output is where a user
looks after a failure. On a dry run it delegates to `render_plan`.
  </action>
  <verify>
    <automated>cargo test --lib sync::restore::report</automated>
  </verify>
  <done>`cargo test --lib sync::restore::report` is green. Exactly two gate functions exist, both taking an injected reader and writer, and no test needs a TTY. `--yes` answers the first and not the second. `render_outcome` names every overwritten path and prints the rollback command on both the success and the partial paths. `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` are clean.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| manifest strings → terminal output | Attacker-chosen paths are rendered into a user's terminal |
| a gate answer → destructive writes | One keystroke authorises overwriting credentials |
| a truncated report → an uninformed decision | What is not printed is what the user cannot object to |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|-----------|----------|-----------|----------|-------------|-----------------|
| T-5-50 | Tampering | terminal escape sequences in a manifest path | high | mitigate | Paths are rendered through an escaping helper that strips or escapes control characters before printing; a path is data, and a terminal that interprets it is a terminal an attacker is scripting |
| T-5-51 | Repudiation | overwritten items reported as a count | high | mitigate | `render_outcome` lists every overwritten path by name; SYNC-06 is satisfied by the list, and the list is asserted in a test |
| T-5-52 | Elevation of privilege | `--yes` answering the credential gate | critical | mitigate | `confirm_credentials` reads `force_credentials` only; a test asserts `assume_yes` alone leaves it refusing |
| T-5-53 | Tampering | a hostile entry hidden by truncation | high | mitigate | The locally-newer and refusal blocks have their own larger budget and lead the report; only ordinary creates and updates are collapsed |
| T-5-54 | Denial of service | a 5,000-line wall nobody reads | medium | mitigate | Per-category line budget with an "and N more" line, asserted as a line ceiling on a 5,000-item plan |
| T-5-55 | Repudiation | a non-interactive run silently proceeding | high | mitigate | No TTY and no `--yes` returns false with a message naming the flag; it never blocks on a stdin that will not arrive, and never assumes consent |
| T-5-56 | Information disclosure | file contents in a report line | high | mitigate | The report renders paths, counts, byte totals, and timestamps only; no variant of `Disposition` or `RestoreOutcome` carries bytes |
| T-5-SC | Tampering | npm/pip/cargo installs | high | mitigate | No new crates; `cargo machete` runs in the phase gate |
</threat_model>

<verification>
- `cargo test --lib sync::restore::report` green.
- `grep -v '^\s*//' src/sync/restore/report.rs | grep -c '_ =>'` is 0.
- `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` clean.
- `HOME= cargo test --lib sync::restore::report` passes.
</verification>

<success_criteria>
1. Every disposition renders; the match is exhaustive with no wildcard.
2. Locally-newer items and refusals lead the report, before the gate.
3. Exactly two gates exist, both injectable, and `--yes` answers only the first.
4. Every overwritten path is named in the summary, and the rollback command prints on both paths.
5. A 5,000-item plan renders under a bounded line count.
</success_criteria>

<output>
Create `.planning/phases/05-pull-and-restore/5-06-SUMMARY.md` when done, recording the two gate
signatures and the line-budget constants plan 5-07 wires against.
</output>
