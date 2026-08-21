---
phase: 06-surfaces-and-ship
plan: 02
type: execute
wave: 2
depends_on: [06-01]
files_modified:
  - macos/ai-usagebar-menubar.swift
  - macos/ai-usagebar-tests.swift
  - src/sync/cli.rs
  - src/widget/cli.rs
autonomous: true
requirements: [UX-05]
must_haves:
  truths:
    - "The dropdown can trigger a push and a pull, and both run through `ai-usagebar sync …` — no crypto, no transport, no credential handling in Swift (D-01)."
    - "A sync that needs a password it cannot prompt for reports \"run it in a terminal\" and exits; it never blocks on a stdin that is not a terminal (D-02)."
    - "Pull is confirmed before it runs and is preceded by a dry run whose result is what the user confirms — a restore overwrites local files and must never be one click away."
    - "The confirmation is *bound* to what it showed: the dry run emits the resolved remote reference, the real pull is passed it as `--expect`, and the CLI aborts non-zero before any write if the remote moved in between."
    - "A dry run that failed or printed nothing produces no dialog and no pull — a confirmation cannot be built from an empty body."
    - "A password piped on stdin still works: only the *interactive prompt* is refused on a non-terminal, because a pipe is one of the three sanctioned password inputs and cannot hang (D-02)."
    - "A failed sync surfaces the CLI's own actionable message in an alert; the menu bar invents no wording of its own for a failure it did not diagnose."
    - "Neither action can start while one is already in flight, and both are disabled during it — same guard as `accountSwitchInFlight`."
    - "The alert text carries no token, no repository URL, and no HTTP response body."
    - "Every Swift test is a pure function over literals; none spawns a process, opens a network socket, or reads a real config."
    - "Every new Rust test drives an injected seam; none reads a real `$HOME` or the network."
  artifacts:
    - "`--non-interactive` on the sync push/pull actions in src/widget/cli.rs, honoured at the prompt call site in src/sync/cli.rs"
    - "A distinct, machine-detectable outcome for \"a password is required and cannot be prompted for\""
    - "`sync pull --dry-run` printing the resolved remote reference, and `sync pull --expect <ref>` refusing non-zero when the remote has moved"
    - "`syncActionArgs(_:dryRun:expect:)` and `syncFailureMessage(_:exitCode:)` in macos/ai-usagebar-menubar.swift — pure, tested"
    - "`pullConfirmation(dryRunOutput:exitCode:) -> PullConfirmation?` and `syncMenuEnabled(inFlight:)` in macos/ai-usagebar-menubar.swift — pure, tested"
    - "The Sync submenu in `buildMenu`, with its in-flight disable and its confirmation flow"
    - "`testSyncActions()` registered in macos/ai-usagebar-tests.swift's TestRunner"
  key_links:
    - "`pullConfirmation` is the single gate: nil means no dialog is shown *and* no pull runs. Splitting it into a body formatter and a separate ref parser would give the flow two nil checks and one of them would eventually be skipped"
    - "`--expect` is the staleness bound. A dialog left open an hour is harmless because the check happens at execution against the live remote, not at dialog-open against a cached one — so no UI-side timer is needed"
    - "6-01 froze `sync status --json`'s key set and the `syncInfoItem` row; this plan attaches the submenu beside that row and reuses its parser for the post-action refresh"
    - "`runAccountSwitch` is the shape both actions copy: in-flight flag, off-main subprocess, merged stdout+stderr, non-zero termination surfaced in an `NSAlert`, re-fetch on completion"
    - "`switchArgs`'s `-y` is the existing precedent for D-02 — the menu asks, then tells the binary the question was answered, so the binary never prompts on a stdin it does not have"
    - "src/sync/cli.rs is edited here and in 6-01; the waves are what keep that safe — 6-01 is wave 1 and must be merged before this plan starts"
---

<objective>
The acting half of the macOS surface: a Sync submenu that can push and pull, built entirely
out of subprocess calls to `ai-usagebar sync …`.

A menu-bar agent cannot prompt. It has no controlling terminal, so a CLI that reaches for a
password on stdin does not ask — it hangs, silently, forever, holding a worker. **D-02** makes
that structurally impossible: the surface always says it cannot answer, and the CLI always
refuses fast and says where to go instead. The account-switch flow already set this precedent
with `-y`; this plan follows it rather than inventing a second convention.

The other half is that a menu item here is a *destructive remote operation*. Push publishes
bytes that cannot be un-published; pull overwrites local credentials. Neither may be one
unconfirmed click away, and pull is confirmed against a real dry run rather than a guess.

Implements **D-01** (surfaces call the CLI) and **D-02** (non-interactive by construction),
and completes **UX-05**.

Purpose: make sync reachable without a terminal, without making it reachable by accident.
Output: the Sync submenu, and the CLI's non-interactive refusal.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/06-surfaces-and-ship/6-CONTEXT.md
@.planning/phases/06-surfaces-and-ship/6-01-SUMMARY.md
@CLAUDE.md
@macos/ai-usagebar-menubar.swift
@macos/ai-usagebar-tests.swift
@src/sync/cli.rs
@src/widget/cli.rs
</context>

<source_audit>
This plan completes **UX-05**. The phase-wide audit is in 6-01. `6-CONTEXT.md` numbers its
decisions `D1`…`D5`, cited here as `D-01`…`D-05`.

| Source | Item | Covered by |
|---|---|---|
| REQ | UX-05 the macOS menu bar exposes sync state **and can trigger a push/pull**, reusing the existing non-interactive-subprocess conventions | 6-01 (state), this plan (triggers) |
| ROADMAP | "macOS menu bar (`macos/`) — sync state plus push/pull triggers, following the existing non-interactive-subprocess conventions" | this plan |
| ROADMAP | Success criterion 2: the menu bar shows last-sync state and can trigger a push and a pull; a sync needing a password it cannot prompt for reports that clearly instead of hanging | 6-01 (state), this plan (triggers + refusal) |
| CONTEXT | D-01 surfaces call the CLI; they do not reimplement sync | this plan |
| CONTEXT | D-02 non-interactive by construction — an already-unlocked key, or a clear refusal, never a silent stdin hang | this plan |
| CONTEXT | D-05 GNOME, KDE and Omarchy out of scope | no file under those trees is touched |
</source_audit>

<tasks>

<task type="tracer" tdd="true">
  <name>Task 1: The CLI's non-interactive refusal — fail fast, say where to go</name>
  <precondition>Phases 3–5 own `sync push` and `sync pull` and may already have a non-interactive flag or a TTY check. Read `src/widget/cli.rs`, `src/sync/cli.rs`, and 6-01's SUMMARY first; if the behaviour already exists under another spelling, adopt that spelling and record it rather than adding a second one. If `sync push`/`sync pull` do not exist at all, stop and report — this plan cannot invent them.</precondition>
  <files>src/widget/cli.rs, src/sync/cli.rs</files>
  <behavior>
    - With the non-interactive flag set and a passphrase required, the command returns a distinct non-zero exit code and prints a message naming the exact terminal command to run.
    - With the flag set and the key already available for the session, the command proceeds normally — the flag suppresses prompting, it does not disable the operation.
    - Without the flag and on a real terminal, behaviour is exactly as Phases 3–5 shipped it.
    - Without the flag and with a **piped** password on a non-terminal stdin, the password is still read and the command proceeds — `ai-usagebar sync push < passfile` keeps working. Only the interactive prompt is refused on a non-TTY, because a pipe cannot hang.
    - Without the flag, on a non-terminal stdin, with nothing piped and no key available, the command refuses with the same named code rather than prompting into a void.
    - The refusal is emitted before any network call and before any file is written, so a refused run has no side effect at all.
    - The refusal message contains no password, no token, and no repository URL.
    - The distinct exit code is a named constant, not a literal repeated at three call sites.
    - `sync pull --dry-run` prints the resolved remote snapshot reference it planned against, on its own line, in a form a caller can pass straight back.
    - `sync pull --expect <ref>` proceeds when the remote's current reference equals `<ref>`.
    - `sync pull --expect <ref>` aborts with a distinct non-zero code, **before any write and before the pre-restore backup**, when the remote's reference differs — and names both references in the message.
    - `--expect` with a syntactically malformed value is rejected as a usage error, never silently treated as "no expectation".
  </behavior>
  <action>
Add a `--non-interactive` flag to the sync actions that can require a passphrase, doc-commented
as "never prompt; refuse and say where to run this instead". Prefer one shared flag over a
per-action copy.

In `src/sync/cli.rs`, honour it at the single point where a passphrase would be requested. Two
outcomes only: the key is already available for this session, so continue; or it is not, so
return the named non-zero exit code with a message naming the literal command the user should
run in a terminal. There is no third branch. A prompt attempted "just in case" on a
non-terminal stdin is the hang this decision exists to prevent, and it is invisible until a
user reports a menu that stopped updating.

Guard the **prompt**, not the command. `src/sync/passphrase.rs` documents three sanctioned
password inputs — a reader (`read_line`, for a pipe or stdin), a mode-0600 file
(`read_from_file`), and a TTY prompt owned by whichever surface holds the terminal. Only the
third can hang. So the `IsTerminal` check belongs at the prompt call site: about to prompt,
stdin is not a terminal, refuse with the named code. A blanket "non-TTY means refuse" would
kill `ai-usagebar sync push < passfile` for every CI user in order to close a hang that the
pipe path cannot cause — a real regression traded for no safety.

Add the exit code as a public named constant so the Swift side can match on it by number
rather than by scraping a message that translation or rewording will change.

**Then bind the dry run to the run.** A confirmation dialog that shows change-set A while the
remote advances to change-set B authorises a restore the user never saw. Make `sync pull
--dry-run` print the resolved remote snapshot reference it planned against — the pointer sha,
the snapshot counter, whatever Phase 5's pull resolves; take the identifier that already
exists rather than minting one — on its own clearly-labelled line. Add `--expect <ref>` to
`sync pull`: re-resolve the remote at the top of the run and abort with a second distinct
non-zero code if it differs, naming both values.

Place the check **before the pre-restore backup**, not after. A refused pull must leave the
machine byte-identical, and a backup written for a restore that never happened is confusing
debris the user has to reason about later.

This is CLI-side on purpose. The surface holds an opaque string and passes it back; it does
not compare references, and it does not learn what a snapshot is. That is D-01, and it is also
what keeps a second, subtly different freshness rule from growing inside a Swift file.

Drive the tests through an injected seam in the shape 6-01 established, with an explicit
"passphrase availability" input. Nothing under test may open a real credential store, resolve
a real `$HOME`, or read the process's real stdin.
  </action>
  <verify>
    <automated>cargo test --lib sync::cli</automated>
  </verify>
  <done>A push or pull that would prompt for a passphrase it cannot ask for exits with the named code and a message naming the terminal command, with no network call, no write, and no prompt — while a piped password still works. `sync pull --dry-run` prints a reference that `--expect` accepts, and a moved remote aborts before any write.</done>
  <reversibility rating="costly">Both exit codes and the `--dry-run` reference line become a contract the shipped menu bar matches on and parses. Choose them once, name them, and record them in the summary.</reversibility>
</task>

<task type="auto" tdd="true">
  <name>Task 2: The Swift side — argument builders and failure wording, pure and tested</name>
  <files>macos/ai-usagebar-menubar.swift, macos/ai-usagebar-tests.swift</files>
  <behavior>
    - `syncActionArgs(.push, dryRun: false, expect: nil)` yields the push argument vector with the non-interactive flag present; `dryRun: true` adds the dry-run flag and nothing else.
    - `syncActionArgs(.pull, dryRun: true, expect: nil)` likewise, and pull's vector never omits the non-interactive flag — a pull that could prompt is a pull that could hang.
    - `syncActionArgs(.pull, dryRun: false, expect: "abc123")` appends the expect flag and its value; a nil `expect` appends neither, and a nil `expect` on a non-dry-run pull is what Task 3 is forbidden from producing.
    - `pullConfirmation(dryRunOutput:exitCode:)` returns nil for any non-zero exit code, nil for empty or whitespace-only output, and nil for output carrying no parsable reference line.
    - `pullConfirmation` on a well-formed dry run returns a body containing the change lines and an `expect` equal to the reference the CLI printed.
    - `pullConfirmation` strips markup and caps the body length, so a long or markup-bearing dry run cannot produce an unbounded or markup-active dialog.
    - `syncMenuEnabled(inFlight: true)` is false and `syncMenuEnabled(inFlight: false)` is true.
    - No builder ever emits a password, a passphrase, or a file path to one — asserted by scanning the produced vectors for the relevant option spellings.
    - `syncFailureMessage` given the passphrase-required exit code returns the "run it in a terminal" wording naming the command, regardless of what the child printed.
    - `syncFailureMessage` given any other non-zero code returns the child's trimmed output, capped in length like the account-switch path caps its detail.
    - `syncFailureMessage` given an empty child output falls back to a message naming the exit code, so an alert is never blank.
    - `syncFailureMessage` strips markup from the child's output before returning it, so binary-supplied text cannot reach the alert as markup.
  </behavior>
  <action>
Add a small `SyncAction` enum (push, pull) and `syncActionArgs(_ action:dryRun:expect:)`
returning `[String]`, mirroring `switchArgs` — pure, one line of logic, fully tested. It is the
only place the argument vector is spelled, so the alternative is that spelling drifting across
three call sites.

Add `struct PullConfirmation { let body: String; let expect: String }` and
`func pullConfirmation(dryRunOutput: String, exitCode: Int32) -> PullConfirmation?`. **One
function, one nil gate**, returning both the text to show and the reference to pass back. The
obvious alternative — a body formatter plus a separate reference parser — gives the flow two
nil checks, and the day someone skips the second one the menu bar runs an unpinned pull. Nil
means: show nothing, run nothing. Parse the reference from the labelled line Task 1 added.

Add `func syncMenuEnabled(inFlight: Bool) -> Bool`. It is one negation; it exists so the
in-flight rule is asserted by a test rather than living only inside a menu-rebuild closure
that no harness can reach.

Add `syncFailureMessage(_ output: String, exitCode: Int32) -> String`, matching the named exit
code recorded by Task 1. Everything else passes the child's own message through, because the CLI
is the thing that diagnosed the failure; a surface that rewrites a diagnosis it did not make
turns an actionable error into a generic one.

Reuse `stripMarkup` on the child output before it reaches an alert, exactly as the panel path
already does for binary-supplied labels.

The password-absence assertions are the important ones. Build the forbidden option spellings in
the test from character fragments rather than as whole literals, so the assertion cannot be
satisfied by its own source text.

Add `testSyncActions()` to `macos/ai-usagebar-tests.swift` and register it in
`TestRunner.main()`. Pure calls over literals only — no `Process`, no filesystem.
  </action>
  <verify>
    <automated>./macos/run-tests.sh</automated>
  </verify>
  <done>The argument builder, the failure formatter, the confirmation gate, and the enable predicate are all pure, covered, and provably incapable of emitting a secret or a secret-bearing path. A failed, empty, or reference-less dry run yields nil.</done>
</task>

<task type="auto" tdd="true">
  <name>Task 3: The Sync submenu — confirm, run off-main, report, refresh</name>
  <files>macos/ai-usagebar-menubar.swift, macos/ai-usagebar-tests.swift</files>
  <behavior>
    - The pull flow calls `pullConfirmation` on the dry run's output **and** its exit code; a nil result shows an error alert naming the failure and returns without showing any confirmation and without invoking a pull.
    - The pull flow passes the `expect` value from that same `pullConfirmation` into `syncActionArgs(.pull, dryRun: false, expect:)`, so the run is pinned to the plan the user approved.
    - No call site constructs a non-dry-run pull argument vector with a nil `expect` — asserted by a source-level check over the file, since a UI closure cannot otherwise be reached from the harness.
    - Both submenu items are disabled while a sync is in flight, driven by `syncMenuEnabled`, and re-enabled when it clears.
    - A sync in flight does not disable the account submenus, and an account switch in flight does not disable the sync submenu — the two flags are independent.
    - The push flow shows a confirmation before invoking anything, and a declined confirmation invokes nothing.
  </behavior>
  <action>
Add a Sync submenu to `buildMenu`, placed with the other action items and attached beside the
`syncInfoItem` row 6-01 added. Three items: "Enviar agora" (push), "Restaurar deste backup…"
(pull), and "Atualizar estado" (re-run the 6-01 status fetch).

**Push.** Confirm with an `NSAlert` first, stating plainly that published bytes cannot be
un-published. Then run `syncActionArgs(.push, dryRun: false)` through the same subprocess shape
as `runAccountSwitch`: an in-flight flag set before dispatch and cleared on the main thread,
merged stdout and stderr on one pipe, read before `waitUntilExit`, the `REFRESH_TIMEOUT`
watchdog armed, and a non-zero termination surfaced through `syncFailureMessage` in a warning
alert. On success, re-fetch the status so the row updates rather than showing a stale time.

**Pull.** Three steps, always, and the middle one is a gate rather than a formatting step.

1. Run `syncActionArgs(.pull, dryRun: true, expect: nil)` and capture output **and** exit code.
2. Feed both to `pullConfirmation`. **Nil means stop**: show `syncFailureMessage` in a warning
   alert and return. No confirmation dialog is displayed and no pull is invoked. Without this
   gate a failed dry run yields an empty dialog body, the user clicks OK on nothing, and the
   restore runs unconfirmed — the worst outcome in the phase, reached by the most natural click.
3. On confirm, run `syncActionArgs(.pull, dryRun: false, expect: confirmation.expect)`. The
   `expect` value comes from the *same* `pullConfirmation` result that produced the text the
   user read. Never re-derive it, never default it to nil, and never fall back to an unpinned
   pull when parsing failed — step 2 already returned nil in that case.

The pin is what makes the confirmation mean anything. Between the dry run and the click,
another machine can push and advance the remote; the user approved change-set A and change-set
B would land on their credential files. Task 1's `--expect` makes the CLI abort instead. It is
also why no dialog timeout is needed: the check runs at execution against the live remote, so a
dialog left open an hour either still matches or is refused — staleness is bounded by the
reference, not by a clock the UI would have to own.

Disable every item in the submenu while any sync is in flight, driven through
`syncMenuEnabled`, and re-render the submenu when it clears — the account submenus already do
exactly this with `accountSwitchInFlight`. Add a second, independent flag rather than reusing that one: an
account switch and a push are unrelated operations and blocking each on the other would be a
puzzling freeze.

Do not add a global keyboard shortcut for either action. Every other item in this menu is
read-only or locally reversible; a hot-key that publishes bytes is not something to hand out by
accident.

Nothing in this task parses or constructs sync arguments inline, and nothing in it decides
whether to show a dialog. It calls Task 2's builders and Task 2's gate. The UI closures stay
thin enough that the tested pure functions really are the decision logic, which is the only
reason a harness that cannot click a menu can still cover the dangerous path.

Extend `testSyncActions()` in `macos/ai-usagebar-tests.swift` with the flow-level assertions
listed in `<behavior>`: the nil-result cases driven straight through `pullConfirmation`, the
pinned-argument case driven through `syncActionArgs` with the value `pullConfirmation`
returned, and `syncMenuEnabled` in both states. Add the source-level check for the forbidden
unpinned-pull call site by reading the app file with `String(contentsOfFile:)` under the
harness and asserting no non-dry-run pull construction passes `expect: nil` — guard it first
with a positive assertion that the file was read and contains a known marker, so it cannot
pass against an empty string.
  </action>
  <verify>
    <automated>./macos/run-tests.sh</automated>
    <human-check>Build the app (`swiftc -O -parse-as-library macos/ai-usagebar-menubar.swift -o /tmp/aiub-menubar`), run it, and confirm: the Sync submenu appears; push asks before running; pull shows the dry-run result before running; both items grey out while one is in flight; killing the network produces an alert carrying the CLI's own message and no URL or token. Then the two that matter most — with the network down, click pull and confirm you get an **error** and never a confirmation dialog; and with a valid dry run showing, push from another machine before clicking OK, and confirm the pull aborts naming both references instead of restoring.</human-check>
  </verify>
  <done>Push and pull are reachable from the dropdown, each confirmed, each run off the main thread under the existing watchdog, each reporting the CLI's own failure text, with the status row refreshed afterwards — and no path can prompt. A failed or empty dry run shows an error and runs nothing; a successful one pins the pull to the reference it displayed. Every one of those decisions is asserted in the harness, not only in the hand check.</done>
  <reversibility rating="costly">The code change is a normal revert. What it exposes is not: a push publishes ciphertext to a remote that may already be cloned or forked, and cannot be un-published. No `checkpoint:decision` precedes this task because the decision is already locked — UX-05 requires the trigger and D-01 fixes how it is built; re-asking would re-open a settled artifact. The controls that make a menu item an acceptable trigger are in-task: the push confirmation, and the dry run the pull confirmation is built from.</reversibility>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|---|---|
| menu click → remote write | A UI event initiates an irreversible publication of encrypted credentials |
| menu click → local overwrite | A UI event initiates a restore over existing credential files |
| subprocess stdout/stderr → `NSAlert` | Child output, possibly carrying remote-supplied text, is rendered |
| menu-bar process → CLI stdin | A stdin that is not a terminal is where a prompt becomes a hang |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|---|---|---|---|---|---|
| T-6-30 | Denial of service | a CLI prompt on a non-terminal stdin | high | mitigate | The guard sits at the prompt call site — about to prompt with a non-terminal stdin, refuse with the named code — before any network call or write. The sanctioned pipe input is untouched, so closing the hang does not break `sync push < passfile`; the `REFRESH_TIMEOUT` watchdog is the backstop (D-02) |
| T-6-31 | Elevation of privilege | a single click publishing credential ciphertext | critical | mitigate | Push is behind an explicit confirmation naming the irreversibility; no keyboard shortcut is registered; the private-repo gate from Phase 3 still runs inside the CLI on every push |
| T-6-32 | Tampering | a single click overwriting local credentials | critical | mitigate | Pull always runs `--dry-run` first and `pullConfirmation` gates on it, so the confirmation shows a real change set; Phase 5's pre-restore backup remains the recovery path |
| T-6-38 | Tampering | the remote advancing between the dry run and the click (time-of-check / time-of-use) | critical | mitigate | The dry run emits the resolved remote reference, the real pull is invoked with `--expect <ref>` taken from the same result, and the CLI re-resolves and aborts non-zero **before the pre-restore backup** if it moved. The reference is also the staleness bound, so no UI-side dialog timeout is needed |
| T-6-39 | Elevation of privilege | a confirmation built from a failed or empty dry run | critical | mitigate | `pullConfirmation` is one nil gate over output **and** exit code; nil shows an error and invokes nothing, so an empty dialog can never be the thing the user clicks OK on. Covered in the harness, plus a source-level check that no call site builds a non-dry-run pull with a nil `expect` |
| T-6-33 | Information disclosure | token or URL in an alert | high | mitigate | The alert shows the CLI's message, which Phase 3's taxonomy already excludes secrets from; output is `stripMarkup`ed and length-capped, and a test asserts no argument vector carries a password option |
| T-6-34 | Information disclosure | a secret passed through argv | critical | mitigate | No builder emits a password or a path to one, asserted by a test that scans the produced vectors; the CLI has refused argv passwords since Phase 1 |
| T-6-35 | Repudiation | no record of what the menu bar pushed | medium | accept | The CLI's own output is the record and the remote carries the snapshot counter. A separate UI-side audit log would be a second source of truth about a thing the CLI already tracks |
| T-6-36 | Denial of service | two concurrent syncs, or a sync racing an account switch | medium | mitigate | A dedicated in-flight flag disables the submenu for the duration, independent of `accountSwitchInFlight` so the two operations do not block each other |
| T-6-37 | Spoofing | a hostile `ai-usagebar` earlier on `PATH` | low | accept | Unchanged from every existing subprocess call in this file; `resolveBinary` prefers the configured path then fixed system locations |
| T-6-SC | Tampering | dependency surface | low | accept | Zero new Swift dependencies — the binary stays a single file built with `swiftc -O -parse-as-library`. Zero new crates; `Cargo.toml` is not in this plan's `files_modified` |
</threat_model>

<verification>
- `./macos/run-tests.sh` and `cargo test --lib sync::cli` are green.
- `swiftc -O -parse-as-library macos/ai-usagebar-menubar.swift -o /tmp/aiub-menubar` succeeds —
  the harness compiles both files together, so a build without the test file is the check that
  the shipped binary still stands alone.
- No new `import` appears in `macos/ai-usagebar-menubar.swift`.
- No new Swift test constructs a `Process`; no new Rust test reads a real `$HOME`, opens a
  socket, or reads the process's real stdin.
- `git diff --stat` touches nothing under `gnome-extension/`, `kde-plasmoid/`, or `omarchy/`
  (D-05), and does not touch `Cargo.toml`.
</verification>

<success_criteria>
The macOS menu bar can push and pull. Push is confirmed and irreversible-by-nature is stated;
pull is confirmed against its own dry run. A sync needing a password it cannot ask for exits
immediately telling the user which command to run in a terminal, and never hangs. All of it is
subprocess calls to `ai-usagebar sync …` — there is no sync logic in Swift.
</success_criteria>

<output>
Create `.planning/phases/06-surfaces-and-ship/6-02-SUMMARY.md` when done.

Record the exact flag spelling and the numeric value of the passphrase-required exit code —
6-05 documents both in the README, and a shipped menu bar matches on the number.

Record whether Phases 3–5 already provided the non-interactive behaviour under a different name,
and if the pull dry-run output needed a machine-readable form to be presentable in an alert.
</output>
