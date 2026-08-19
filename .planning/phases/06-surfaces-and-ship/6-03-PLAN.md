---
phase: 06-surfaces-and-ship
plan: 03
type: execute
wave: 1
depends_on: []
files_modified:
  - src/widget/run.rs
  - src/config.rs
autonomous: true
requirements: [UX-06]
must_haves:
  truths:
    - "The widget exits 0 and renders the fallback `⚠` JSON for every sync-shaped failure, asserted by a test that injects the failure rather than by observation (D-03)."
    - "A `[sync]` section written by a newer build — unknown keys, wrong types, a garbage value — never stops the widget from rendering."
    - "The widget binary reaches no code path under the crate's sync module, so a sync failure is unreachable from it by construction, not by ordering."
    - "The `⚠` tooltip carries no repository URL, token, or HTTP response body from a sync failure."
    - "Every assertion runs through the real `run_once` writer seam, so it pins the shipped path and not just the private `fallback` helper."
    - "No test in this plan reads a real `$HOME`, the network, or the Keychain."
  artifacts:
    - "Tests in src/widget/run.rs pinning exit 0 plus the `⚠` payload for a poisoned `[sync]` config and for each sync-shaped `AppError` arm"
    - "A structural test asserting the widget render path holds no reference to the crate's sync module"
    - "`#[serde(default)]` / unknown-key tolerance on the sync config types in src/config.rs, if the round trip proves it missing"
  key_links:
    - "`widget::run::run_once` is the seam every assertion drives; `fallback` alone is not enough because a panic or an early return before it would still take the bar down"
    - "`WaybarOutput::error` is the single fallback constructor — pinning its shape here is what makes the Waybar contract testable"
    - "6-01 owns the *other* half of D-03: `sync status` exiting non-zero. This plan must not add an exit-code test to src/sync/cli.rs — that file belongs to another wave-1 plan"
---

<objective>
Pin the invariant the whole milestone leans on: whatever encrypted sync does, `ai-usagebar`
rendering into Waybar exits 0 with a payload Waybar will draw. Waybar hides modules that do
not, so a sync bug that took the status bar down would be indistinguishable from the tool
being uninstalled.

This is **D-03**'s widget half. `ai-usagebar sync …` returning non-zero on failure is the
other half and belongs to 6-01 — they are different binaries with deliberately different
contracts, and this plan makes that difference a test instead of a paragraph.

The strongest form of the invariant is structural: the widget render path never calls into
sync at all, so there is no sync failure for it to survive. Prove that, then prove it
survives the failures it *can* still see — a config file a `sync pull` rewrote.

Purpose: make UX-06 falsifiable.
Output: regression tests, and whatever tolerance the round trip proves missing.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/06-surfaces-and-ship/6-CONTEXT.md
@CLAUDE.md
@src/widget/run.rs
@src/waybar.rs
@src/error.rs
</context>

<source_audit>
This plan covers **UX-06** in full. The rest of the phase's source items are audited in
6-01's `<source_audit>`; `6-CONTEXT.md` numbers its decisions `D1`…`D5`, cited here as
`D-01`…`D-05`.

| Source | Item | Covered by |
|---|---|---|
| REQ | UX-06 the widget's exit-0 invariant holds — a sync failure never takes the status bar down | this plan |
| CONTEXT | D-03 the exit-0 invariant is the widget's, not sync's | this plan (widget half), 6-01 (sync half) |
| ROADMAP | "the widget fallback path with a test that injects a failing transport" | this plan |
| ROADMAP | Success criterion 1: with sync configured and the transport failing, the widget exits 0 and renders the fallback `⚠` JSON | this plan |
</source_audit>

<tasks>

<task type="tracer" tdd="true">
  <name>Task 1: A config a sync pull wrote must not be able to take the bar down</name>
  <precondition>Phases 2–5 have added a `[sync]` section to `config.toml`; read `src/config.rs` for its current shape before writing the fixtures, and use the real field names rather than invented ones.</precondition>
  <files>src/widget/run.rs, src/config.rs</files>
  <behavior>
    - A config whose sync section carries a key this build does not know still loads, and the widget renders normally — a newer machine's settings must not brick an older one after a restore.
    - A config whose sync section has a value of the wrong type produces the `⚠` fallback, and `run` returns 0.
    - A config file that is not valid TOML at all produces the `⚠` fallback, and `run` returns 0.
    - A config file that is unreadable (mode 000, Unix only) produces the `⚠` fallback, and `run` returns 0.
    - In every failing case stdout is exactly one line and parses as JSON with the keys Waybar requires.
    - The rendered tooltip in every failing case contains no `https://` URL and no `ghp_`/`github_pat_` prefixed string.
  </behavior>
  <action>
Work through `run_once`, not through `fallback`. The existing tests call the private helper
directly, which proves the helper formats correctly but not that the shipped path reaches it —
an early return or a propagated error before that point would still hide the module. Drive the
real entry with the `&mut impl Write` seam it already takes, assert on the captured bytes, and
assert the returned code.

Seed each fixture through the config-path injection the crate already has; if the only way in
is an ambient environment variable, add a path-taking seam beside the existing loader rather
than making the test read the process environment. Tests here must stay hermetic — the AUR
`check()` runs them on an installer's machine with their own config present.

For the unknown-key case, add whatever the sync config types need to tolerate it, most likely
`#[serde(default)]` plus not denying unknown fields. Do this only if a round trip proves it
missing: a restore from a machine running a newer build is the realistic path to an unknown
key, and refusing it would turn a successful sync into a broken status bar, which is precisely
the failure mode UX-06 forbids.

For the secret-leak assertions, build the expected-absent literals in the test from character
fragments rather than writing them out whole, so the assertion cannot match its own source
text if someone later greps the file.

Do not add an exit-code assertion for the sync subcommand here. That contract is real and is
tested — in `src/sync/cli.rs`, which another wave-1 plan owns. Two plans editing one file in
one wave is the merge conflict this phase's wave layout exists to avoid.
  </action>
  <verify>
    <automated>cargo test --lib widget::run</automated>
  </verify>
  <done>Four poisoned-config fixtures each yield exit 0 plus one line of valid Waybar JSON carrying `⚠`; the unknown-key fixture renders normally instead of falling back; no tooltip carries a URL or a token-shaped string.</done>
</task>

<task type="auto" tdd="true">
  <name>Task 2: The structural gate — the widget cannot reach sync at all</name>
  <files>src/widget/run.rs</files>
  <behavior>
    - The widget's own render path holds no reference to the crate's encrypted-backup module, so no failure originating there can propagate into it.
    - The check reads the shipped source with comment lines filtered out, so a doc comment mentioning the module cannot fail the gate and prose cannot silently satisfy it either.
    - The check fails loudly if the file it is meant to read is missing or empty, rather than passing vacuously.
  </behavior>
  <action>
Add one test that reads `src/widget/run.rs` and `src/widget/render.rs` via
`include_str!` — compile-time, so it needs no working directory and stays hermetic — strips
every line whose first non-whitespace characters begin a comment, and asserts the remaining
text contains no path reference into the crate's encrypted-backup module.

Assert first that the stripped text is non-empty and still contains a known marker from the
file (`WaybarOutput`, say). Without that guard the test passes just as happily against an
empty string, which is the classic way a structural gate rots into decoration.

Write the forbidden path as a runtime-assembled string, not as a literal in the test body,
so the file cannot contain the very text it forbids. A short comment above it should explain
why the indirection exists — otherwise the next reader "simplifies" it back into a literal
and the test starts failing on itself.

This is the real mitigation for UX-06: it is not that the widget recovers well from a sync
failure, it is that a sync failure is unreachable from the widget. The Task 1 tests cover the
one thing the two binaries genuinely share, which is the config file on disk.
  </action>
  <verify>
    <automated>cargo test --lib widget::run</automated>
  </verify>
  <done>The gate is green, and it fails when the forbidden path is temporarily added to a non-comment line of the render path — verify that once by hand before committing.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|---|---|
| `config.toml` → widget | A file another machine wrote and `sync pull` restored is parsed by a process whose contract is "always render" |
| widget stdout → Waybar | A non-zero exit or malformed line removes the module from the user's bar |
| `AppError` text → tooltip | Error text crosses into a rendered surface and could carry remote-supplied bytes |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|---|---|---|---|---|---|
| T-6-10 | Denial of service | widget exit code | high | mitigate | `run` returns 0 on every path; four poisoned-config fixtures assert it through the real `run_once` writer seam, not through the private fallback helper |
| T-6-11 | Denial of service | a restore from a newer build | high | mitigate | Unknown keys in the sync config section are tolerated; a successful sync must never be the thing that breaks the status bar |
| T-6-12 | Information disclosure | `⚠` tooltip | high | mitigate | Existing coverage keeps auth response bodies out of the tooltip; this plan extends it to repository URLs and token-shaped strings, with the expected-absent literals assembled at runtime so the assertion cannot match its own source |
| T-6-13 | Tampering | widget reachability into sync code | medium | mitigate | A structural test over the shipped source, comment-filtered and guarded against vacuous success, proves the render path holds no reference into the encrypted-backup module |
| T-6-14 | Denial of service | cache flock held by a concurrent sync | low | accept | Sync touches no per-vendor cache file, so the existing 15 s flock wait is unaffected. Recorded rather than tested: a hermetic test would have to fabricate contention that the design already prevents |
| T-6-SC | Tampering | dependency surface | low | accept | Zero new crates; `Cargo.toml` is not in this plan's `files_modified` |
</threat_model>

<verification>
- `cargo test --lib widget::run` is green.
- `cargo test --lib config` is green — the tolerance change must not loosen validation
  anywhere outside the sync section.
- The structural gate fails when the forbidden path is temporarily inserted into a
  non-comment line, and passes again when removed. Check this by hand once; a gate never
  observed failing is not a gate.
- `git diff --stat` touches no file under `gnome-extension/`, `kde-plasmoid/`, `omarchy/`, or
  `macos/`, and does not touch `Cargo.toml` or `src/sync/`.
</verification>

<success_criteria>
With sync configured and anything about it broken — a half-written config, an unknown key, an
unreadable file — `ai-usagebar` still exits 0 and prints one line of Waybar JSON showing `⚠`,
proven by injected failures. And the render path provably cannot reach sync code at all.
</success_criteria>

<output>
Create `.planning/phases/06-surfaces-and-ship/6-03-SUMMARY.md` when done.

Record whether the sync config types already tolerated unknown keys or had to be changed —
6-05's release notes need to say honestly whether restoring onto an older build is safe.
Record the name of the config-path seam the tests use, and state plainly that the sync
subcommand's non-zero exit contract is asserted in 6-01, not here.
</output>
