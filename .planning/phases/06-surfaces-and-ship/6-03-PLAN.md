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
    - "The widget **render path** — `run.rs`, `render.rs`, `pretty.rs` — holds no reference to the crate's sync module, so a sync failure is unreachable from it by construction, not by ordering."
    - "The `⚠` tooltip carries no repository URL, token, or HTTP response body from a sync failure."
    - "Every assertion runs through the real `run_once` writer seam — which this plan first has to make testable, by threading an optional config path through it and giving it an exit code to return."
    - "A fetch that fails for a non-config reason also lands in the `⚠` fallback at exit 0 through that same seam, so the coverage is not config-only."
    - "No test in this plan reads a real `$HOME`, the network, or the Keychain."
  artifacts:
    - "`run_with(cli, config_path)` and `run_once(cli, out, config_path) -> i32` in src/widget/run.rs — today `run_once` returns `()` and reaches config only through `Config::load()`, so neither an exit code nor a hermetic config exists to assert against"
    - "Tests in src/widget/run.rs pinning exit 0 plus the `⚠` payload for a poisoned `[sync]` config and for a failing fetch, both through that seam"
    - "A structural test asserting the render path — run.rs, render.rs, pretty.rs — holds no reference to the crate's sync module"
    - "`#[serde(default)]` / unknown-key tolerance on the sync config types in src/config.rs, if the round trip proves it missing"
  key_links:
    - "`widget::run::run_once` is the seam every assertion drives; `fallback` alone is not enough because a panic or an early return before it would still take the bar down. It is not usable as-is — `Config::load()` inside `build_output` reads the real `$HOME`, and `run_once` returns `()`. Both are fixed here"
    - "`Config::load_from(&Path)` already exists in src/config.rs and is what the threaded path calls; `Config::load()` stays the `None` case, so no production behaviour changes"
    - "`run(cli)` keeps its signature and delegates to `run_with(cli, None)`, so src/bin/ai-usagebar.rs is untouched and stays out of files_modified"
    - "`WaybarOutput::error` is the single fallback constructor — pinning its shape here is what makes the Waybar contract testable"
    - "6-01 owns the *other* half of D-03: `sync status` exiting non-zero. This plan must not add an exit-code test to src/sync/cli.rs — that file belongs to another wave-1 plan"
---

<objective>
Pin the invariant the whole milestone leans on: whatever encrypted sync does, `ai-usagebar`
rendering into Waybar exits 0 with a payload Waybar will draw. Waybar hides modules that do
not, so a sync bug that took the status bar down would be indistinguishable from the tool
being uninstalled.

This is **D-03**'s widget half. `ai-usagebar sync …` returning non-zero on failure is the
other half and belongs to 6-01 — two commands with deliberately opposite exit contracts, and
this plan makes that difference a test instead of a paragraph.

One correction to carry forward: `6-CONTEXT.md`'s D3 calls these "different binaries". They are
not — `ai-usagebar` is a single binary, `sync` is a subcommand of it, and the render path is
what runs when no subcommand is given. The *contract* split D3 describes is exactly right and
is not being re-opened; only the framing is wrong, and it matters here because it is the reason
the structural test below is scoped to the render path rather than to the binary. 6-05 must not
repeat the "different binary" wording in the README.

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
| ROADMAP | "the widget fallback path with a test that injects a failing transport" | this plan — **substituted**: an absent credential under injected roots fails the fetch before a socket opens, giving the same end-to-end path through `run_once` with no network. A true transport injection would need a client seam `build_output` does not have; adding one to reach an outcome already reachable would be scaffolding. Recorded here and in the SUMMARY rather than dropped |
| ROADMAP | Success criterion 1: with sync configured and the transport failing, the widget exits 0 and renders the fallback `⚠` JSON | this plan |
</source_audit>

<tasks>

<task type="tracer" tdd="true">
  <name>Task 1: A config a sync pull wrote must not be able to take the bar down</name>
  <precondition>Phases 2–5 have added a `[sync]` section to `config.toml`; read `src/config.rs` for its current shape before writing the fixtures, and use the real field names rather than invented ones. `Config::load_from(&Path)` already exists there — this task threads it in, it does not add it.</precondition>
  <files>src/widget/run.rs, src/config.rs</files>
  <behavior>
    - `run_once` returns an exit code and accepts a config path, so a test can assert both without touching `$HOME`. Today it returns `()` and reaches config only through `Config::load()`; neither is assertable.
    - `run(cli)` still compiles and behaves identically for its existing caller — the path parameter defaults to `None`, which resolves exactly as before.
    - A config whose sync section carries a key this build does not know still loads, and the widget renders normally — a newer machine's settings must not brick an older one after a restore.
    - A config whose sync section has a value of the wrong type produces the `⚠` fallback, and `run` returns 0.
    - A config file that is not valid TOML at all produces the `⚠` fallback, and `run` returns 0.
    - A config file that is unreadable (mode 000, Unix only) produces the `⚠` fallback, and `run` returns 0.
    - In every failing case stdout is exactly one line and parses as JSON with the keys Waybar requires, and the returned exit code is 0.
    - A **non-config** failure lands the same way: a valid config selecting a vendor whose credential file is absent under the injected roots yields the `⚠` fallback and exit 0, with no socket opened. This is the injected-failure case the ROADMAP asks for, reachable without a network.
    - The rendered tooltip in every failing case contains no `https://` URL and no `ghp_`/`github_pat_` prefixed string.
  </behavior>
  <action>
**First make the seam real; it is not usable as written today.** `run_once(cli, out)` returns
`()`, so there is no code to assert, and it reaches configuration only via `build_output` →
`Config::load()`, which resolves the real `$HOME`. Asserting hermetically *and* through
`run_once` *and* on an exit code is impossible against the current signatures — do not pick two
and leave the third silently unmet.

The change, precisely:

- `async fn build_output(cli: &Cli, config_path: Option<&Path>)` — call `Config::load_from(p)`
  when a path is given, `Config::load()` when it is not. `load_from` already exists in
  `src/config.rs` and is what `load` itself delegates to, so the `None` path is unchanged
  production behaviour, not a parallel loader.
- `async fn run_once(cli: &Cli, out: &mut impl Write, config_path: Option<&Path>) -> i32` —
  returns 0 unconditionally. That constant *is* the invariant; returning it from the function
  under test is what lets a test fail if someone later makes it conditional.
- `pub async fn run_with(cli: Cli, config_path: Option<&Path>) -> i32`, with the existing
  `pub async fn run(cli: Cli) -> i32` delegating `None`. `src/bin/ai-usagebar.rs` is untouched
  and stays out of `files_modified`; a signature change there would be a wider blast radius
  than this invariant is worth.

Then drive every fixture through `run_once` with a `TempDir` config path, assert on the
captured bytes, and assert the returned code. Tests here must stay hermetic — the AUR `check()`
runs them on an installer's machine with their own config present.

For the non-config failure, point a valid config at a vendor whose credential file does not
exist under the injected roots. The fetch fails before any socket is opened, so it is both a
genuine end-to-end failure through the shipped path and offline. That is the substitution for
"inject a failing transport": a real transport failure needs either a network or a client seam
`build_output` does not have, and adding one to reach an outcome an absent credential already
produces would be scaffolding. Record that reasoning in the SUMMARY so the substitution is a
decision on the record rather than a quiet omission.

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
  <done>`run_once` takes a config path and returns an exit code; `run` is unchanged for its caller. Four poisoned-config fixtures and one absent-credential fixture each yield exit 0 plus one line of valid Waybar JSON carrying `⚠`; the unknown-key fixture renders normally instead of falling back; no tooltip carries a URL or a token-shaped string; no test resolves `$HOME` or opens a socket.</done>
</task>

<task type="auto" tdd="true">
  <name>Task 2: The structural gate — the widget cannot reach sync at all</name>
  <files>src/widget/run.rs</files>
  <behavior>
    - The widget's render path — all three of its files — holds no reference to the crate's encrypted-backup module, so no failure originating there can propagate into it.
    - The gate covers `pretty.rs` as well as `run.rs` and `render.rs`: `print_pretty` is called directly from the function under test, so a gate that skipped it would leave a third of the path unchecked.
    - The check reads the shipped source with comment lines filtered out, so a doc comment mentioning the module cannot fail the gate and prose cannot silently satisfy it either.
    - The check fails loudly if the file it is meant to read is missing or empty, rather than passing vacuously.
  </behavior>
  <action>
Add one test that reads `src/widget/run.rs`, `src/widget/render.rs` **and
`src/widget/pretty.rs`** via `include_str!` — compile-time, so it needs no working directory and
stays hermetic — strips every line whose first non-whitespace characters begin a comment, and
asserts the remaining text contains no path reference into the crate's encrypted-backup module.

All three, not two: `run_once` calls `print_pretty` on every non-JSON render, so `pretty.rs` is
on the shipped path and a gate that omits it proves less than it claims. If a future refactor
adds a fourth file to that path, this test is where it has to be added — say so in a comment
above it.

Assert first that the stripped text is non-empty and still contains a known marker from the
file (`WaybarOutput`, say). Without that guard the test passes just as happily against an
empty string, which is the classic way a structural gate rots into decoration.

Write the forbidden path as a runtime-assembled string, not as a literal in the test body,
so the file cannot contain the very text it forbids. A short comment above it should explain
why the indirection exists — otherwise the next reader "simplifies" it back into a literal
and the test starts failing on itself.

This is the real mitigation for UX-06: it is not that the render path recovers well from a sync
failure, it is that a sync failure is unreachable from it. Scope the claim to the render path,
not to the binary — `ai-usagebar` is one binary that carries both the sync subcommand and the
render path, so "the binary does not link sync" would be false and a test asserting it would be
asserting the wrong thing. The Task 1 tests cover what the two paths genuinely share, which is
the config file on disk.
  </action>
  <verify>
    <automated>cargo test --lib widget::run</automated>
  </verify>
  <done>The gate covers all three render-path files, is green, and fails when the forbidden path is temporarily added to a non-comment line of any of them — verify that once by hand before committing.</done>
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
| T-6-13 | Tampering | render-path reachability into sync code | medium | mitigate | A structural test over all three shipped render-path files (`run.rs`, `render.rs`, `pretty.rs`), comment-filtered and guarded against vacuous success, proves the render path holds no reference into the encrypted-backup module. Scoped to the path, not the binary — they are the same binary |
| T-6-14 | Denial of service | cache flock held by a concurrent sync | low | accept | Sync touches no per-vendor cache file, so the existing 15 s flock wait is unaffected. Recorded rather than tested: a hermetic test would have to fabricate contention that the design already prevents |
| T-6-SC | Tampering | dependency surface | low | accept | Zero new crates; `Cargo.toml` is not in this plan's `files_modified` |
</threat_model>

<verification>
- `cargo test --lib widget::run` is green.
- `cargo build` is green with `run`'s public signature unchanged — `git diff src/bin/` is empty.
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

Record the final signatures of `run_with`, `run_once`, and `build_output`, and the substitution
argument for the failing-transport case, so a later reader sees it was decided rather than
skipped. Record whether the sync config types already tolerated unknown keys or had to be changed —
6-05's release notes need to say honestly whether restoring onto an older build is safe.
Record the name of the config-path seam the tests use, and state plainly that the sync
subcommand's non-zero exit contract is asserted in 6-01, not here.
</output>
