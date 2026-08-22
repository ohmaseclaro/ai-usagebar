---
phase: 06-surfaces-and-ship
plan: 01
type: execute
wave: 1
depends_on: []
files_modified:
  - src/widget/cli.rs
  - src/sync/cli.rs
  - src/sync/report.rs
  - macos/ai-usagebar-menubar.swift
  - macos/ai-usagebar-tests.swift
autonomous: true
requirements: [UX-05]
must_haves:
  truths:
    - "`ai-usagebar sync status --json` prints one JSON object on stdout and nothing else, and exits zero on success."
    - "The object carries `last_sync` and a pending-changes summary, so a stale backup is visible without opening a terminal (D-04)."
    - "The same command exits **non-zero** when the config or the roots cannot be resolved — sync owns a real exit code, unlike the widget (D-03)."
    - "The JSON contains no file contents: only paths that were already printed by the human renderer, counts, and bytes."
    - "The menu bar parses that JSON with every field optional, so an older binary that rejects `--json` degrades to a hidden row instead of a crash (D-01)."
    - "One dropdown row shows last-sync and whether local changes are pending; no sync logic exists in Swift (D-01)."
    - "Every new Rust test injects its roots and its `now`; none reads a real `$HOME`, so the AUR `check()` cannot fail on an installer's machine."
    - "Every new Swift test is a pure function over a `Data` literal; none spawns a process or reads a real config."
  artifacts:
    - "`--json` on `SyncAction::Status` in src/widget/cli.rs"
    - "`report::status_json(&StatusReport) -> serde_json::Value` in src/sync/report.rs — pure, no filesystem"
    - "`PendingSummary` on `StatusReport`, counted from index lookups with no file body read"
    - "`pub warnings: Vec<String>` on `StatusReport` — the field does not exist today; `StatusReport` currently carries `lines`, `last_sync`, and `index_path` only"
    - "`SyncStatus` + `parseSyncStatus(_ data: Data) -> SyncStatus?` in macos/ai-usagebar-menubar.swift"
    - "`syncSummaryLine(_:)` in macos/ai-usagebar-menubar.swift — the one dim row, pure"
    - "`testSyncStatus()` registered in macos/ai-usagebar-tests.swift's TestRunner"
  key_links:
    - "`status_json` is the single serializer; `sync::cli::status` picks text or JSON from one built `StatusReport`, so the two renderings can never disagree"
    - "`parseSyncStatus` mirrors `parseAccountStatus`: every key optional, a non-object or empty body yields nil — that is what makes the older-binary path safe"
    - "The sync row is fetched on `menuWillOpen` alongside `fetchAccountStatus`, off the main thread, under the existing `REFRESH_TIMEOUT` watchdog — no new timer, no new spawn shape"
    - "6-02 adds push/pull against this exact JSON shape; freezing `status_json`'s keys here is what lets 6-02 build without re-deriving them"
---

<objective>
The Phase 6 tracer: one thin path from the local index, through a new `--json` rendering of
the existing `sync status` model, out of the CLI, into the macOS menu bar, and onto one
dropdown row that says when sync last ran and whether anything is pending.

Nothing is pushed, nothing is pulled, no password is ever needed. This is the layer test:
it proves the surface can read the CLI before 6-02 lets the surface *act* through it.

It also freezes the JSON contract. 6-02 parses these keys; if they move afterwards, two
plans break instead of one.

Implements **D-01** (the surface calls the CLI, no sync logic in Swift), **D-03** (sync
exits non-zero; the widget's exit-0 contract is a different binary's, pinned in 6-03), and
**D-04** (sync state is surfaced, not just actions).

Purpose: prove the whole surface stack on one read-only command before anything can trigger
a remote write.
Output: `sync status --json`, and a menu-bar row that consumes it.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/06-surfaces-and-ship/6-CONTEXT.md
@CLAUDE.md
@src/sync/report.rs
@src/sync/cli.rs
@src/sync/index.rs
@src/account.rs
@macos/ai-usagebar-menubar.swift
</context>

<source_audit>
Phase-wide coverage audit. `6-CONTEXT.md` numbers its decisions `D1`…`D5`; they are cited
throughout Phase 6 as `D-01`…`D-05` so the identifiers are greppable. Same decisions, no
renumbering.

| Source | Item | Covered by |
|---|---|---|
| GOAL | Sync reachable from the TUI and the macOS menu bar | 6-01, 6-02, 6-04 |
| GOAL | A sync failure can never take the status bar down | 6-03 |
| GOAL | The release ships through the full checklist | 6-05 |
| REQ | UX-05 the menu bar exposes sync state and can trigger a push/pull, reusing the existing non-interactive-subprocess conventions | 6-01 (state), 6-02 (triggers) |
| REQ | UX-06 the widget's exit-0 invariant holds — a sync failure never takes the status bar down | 6-03 |
| ROADMAP | TUI sync panel: status, category toggles, last-sync, `toml_edit` conventions, post-save waybar signal | 6-04 |
| ROADMAP | Widget fallback path with a test that injects a failing transport | 6-03 |
| ROADMAP | README sync section, PAT recipe, honest limits (no recovery, rekey is not revocation, metadata leakage, AUP §9) | 6-05 |
| ROADMAP | Release checklist: versions matched, CHANGELOG, both PKGBUILDs, both `.SRCINFO`s regenerated before tagging, full gate | 6-05 |
| CONTEXT | D-01 surfaces call the CLI; they do not reimplement sync | 6-01, 6-02 (whole-phase invariant) |
| CONTEXT | D-02 non-interactive by construction; refuse with "run it in a terminal", never hang on stdin | 6-02 |
| CONTEXT | D-03 the exit-0 invariant is the widget's, not sync's | 6-01 (sync side), 6-03 (widget side) |
| CONTEXT | D-04 sync state is surfaced, not just actions — last-sync and pending changes | 6-01, 6-04 |
| CONTEXT | D-05 GNOME, KDE and Omarchy are out of scope | whole phase — no plan lists a file under `gnome-extension/`, `kde-plasmoid/`, or `omarchy/`; 6-05 keeps their contract suites green as a regression gate only |

**Deliberate exclusions, not gaps:** D-05's three frontends. `kde-plasmoid/package/metadata.json`'s
`KPlugin.Version` is bumped only if that tree changes, and D-05 guarantees it does not — 6-05
records that as a checked fact rather than an omitted step.
</source_audit>

<tasks>

<task type="tracer" tdd="true">
  <name>Task 1: `sync status --json` — the machine-readable contract</name>
  <precondition>Phases 3–5 are merged, so `SyncAction` may already carry variants beyond `Status` and `SyncConfig` may already carry a `repo` field. Read `src/widget/cli.rs` and `src/config.rs` first and extend what is there; do not recreate either.</precondition>
  <files>src/widget/cli.rs, src/sync/report.rs, src/sync/cli.rs</files>
  <behavior>
    - `status_json` over a `StatusReport` with one enabled category and a known `last_sync` produces an object whose `last_sync` is that RFC 3339 string and whose `categories` array has one entry with `category`, `enabled`, `files`, `bytes`, `capped`.
    - A report with `last_sync: None` produces JSON `null` for that key, never the string "never" — the caller decides the wording.
    - A report whose `index_path` is empty produces `null` for `index`, and the `warnings` array is non-empty.
    - `status_json` is pure: called twice on the same report it returns an equal `Value`.
    - The produced object contains no key whose value is a file's contents — asserted by walking the `Value` and requiring every string leaf to be a path, a label, an RFC 3339 timestamp, or a warning drawn from a fixed set.
    - A `StatusReport` carrying a `PendingSummary` of 3 files / 4096 bytes produces `pending: true`, `pending_files: 3`, `pending_bytes: 4096`; a summary of zero files produces `pending: false`; an absent summary (no index) produces `pending: null`.
  </behavior>
  <action>
Add a `--json` flag to the `Status` variant of `SyncAction` in `src/widget/cli.rs`, with the
same doc-comment style as `Usage { json }` two variants above it. Do not add a top-level flag
and do not touch any other variant.

In `src/sync/report.rs`:

Add three fields to `StatusReport`, which today carries `lines`, `last_sync`, and
`index_path` and nothing else — check that before assuming any of them is already there.

`pub warnings: Vec<String>`, default empty. `build_status` cannot fill it (it has no failure
of its own to report), so the caller pushes into it: today's only entry is the
index-unavailable message that `sync::cli::status` currently sends to stderr and drops. A
JSON consumer that never sees it would render "last sync: never" for a machine that syncs
hourly.

`PendingSummary { pub files: usize, pub bytes: u64 }` and
`pub pending: Option<PendingSummary>` on `StatusReport`. `None` means the index was not
available, which is a third state and must not be flattened into "nothing pending" — a
backup nobody can tell is stale is exactly what D-04 exists to prevent.

Fill it inside `build_status`, which already receives `Option<&Index>`. For each scanned
file, ask the index whether it has an unchanged record; count the misses. Reuse the same
short-circuit the planner already uses — `Index::lookup` over the entry's
`(size, mtime_ns, ctime_ns, inode)`. **Never open a file body here.** `sync status` is
advertised as costing a stat sweep, and a status call that hashes a 50 MB transcript to draw
a menu row would break that promise silently. Add a test that seeds a tree, builds a status
twice, and asserts the byte count read from disk is zero on the second pass, in the shape
Phase 2 already uses for its read counter.

Add `pub fn status_json(report: &StatusReport) -> serde_json::Value`, built with the
`serde_json::json!` macro exactly as `account::status` builds its object — that is the
established precedent and it keeps the shape visible in one screen instead of scattered
across derives. Keys, all present on every run so the consumer never has to distinguish
"absent" from "null":

`last_sync` (RFC 3339 string or null), `pending` (bool or null), `pending_files` (number or
null), `pending_bytes` (number or null), `categories` (array of
`{category, enabled, files, bytes, capped}` in `SyncCategory::ALL` order), `total_files`,
`total_bytes`, `index` (path string or null), `warnings` (array of strings, empty when clean —
serialized straight from the new field, never re-derived).

Reuse `CategoryLine`'s fields verbatim; do not invent parallel names. Leave the object open
for 6-02 and for Phases 3–5 fields such as `repo` — a consumer that ignores unknown keys is
what makes that safe, and the Swift parser in Task 2 is written that way.

`render_status` stays exactly as it is. Both renderings are derived from one built
`StatusReport`, so they cannot drift.

In `src/sync/cli.rs`: thread the flag through `run` into `status(json: bool)`. On success,
print `serde_json::to_string(&status_json(&report))` plus a newline when the flag is set, and
the existing text otherwise; return 0. On a config or roots failure, keep the existing
`eprintln!` and the non-zero return — a script piping this deserves a real exit code, and
that is the whole point of D-03's split. Push the index-unavailable message into the report's new
`warnings` field as well as stderr, so a JSON consumer sees it too.

Refactor `status()` so the config/roots/index resolution and the printing are separable, and
add a `status_with(roots, cfg, index, now, json) -> (i32, String)` seam that the tests drive.
Nothing under test may call `Config::load`, `SyncRoots::resolve`, `index::default_path`, or
`Utc::now`.
  </action>
  <verify>
    <automated>cargo test --lib -- sync::report sync::cli</automated>
  </verify>
  <done>`--json` prints one line of valid JSON carrying last-sync and the pending summary; the text rendering is byte-identical to before; a resolution failure still exits non-zero; no test reads a real `$HOME` and no status call opens a file body.</done>
  <reversibility rating="costly">The JSON key set becomes a consumer-facing contract the moment 6-02 parses it and a user's menu bar ships against it. Renaming a key later is a compatibility break, so name them once, here.</reversibility>
</task>

<task type="auto" tdd="true">
  <name>Task 2: The Swift parser — every field optional, like `parseAccountStatus`</name>
  <files>macos/ai-usagebar-menubar.swift, macos/ai-usagebar-tests.swift</files>
  <behavior>
    - A full object parses into a `SyncStatus` with `lastSync`, `pending`, `pendingFiles`, `pendingBytes` and one entry per category.
    - An object with only `{"last_sync":null}` parses, with `pending` nil — nil and false are distinct and must stay distinct.
    - Empty `Data`, `"error: unrecognized subcommand"`, and `"[1,2,3]"` each parse to nil, never a crash — this is the older-binary path.
    - An unknown extra key is ignored rather than rejected, so a later phase can add `repo` without breaking a shipped menu bar.
    - `syncSummaryLine` renders "Sync: nunca" for a nil last-sync, "Sync: <relative> · pendente" when pending is true, "Sync: <relative>" when false, and the empty string when the whole status is nil, so the caller can hide the row.
    - A `last_sync` string that is not RFC 3339 yields a nil date and the "nunca" wording rather than a formatted garbage date.
  </behavior>
  <action>
In `macos/ai-usagebar-menubar.swift`, next to `AccountStatus` and its parser, add:

`struct SyncCategoryLine: Equatable { var category: String; var enabled: Bool; var files: Int; var bytes: Int }`
and `struct SyncStatus: Equatable` with `lastSync: Date?`, `pending: Bool?`,
`pendingFiles: Int?`, `pendingBytes: Int?`, `categories: [SyncCategoryLine]`,
`warnings: [String]`.

`func parseSyncStatus(_ data: Data) -> SyncStatus?` built the same way as
`parseAccountStatus`: `JSONSerialization`, a guarded cast to `[String: Any]`, every field
read through an optional cast with a default. Decode the timestamp with an
`ISO8601DateFormatter` configured for fractional seconds, falling back to one without —
`chrono`'s `to_rfc3339` emits nanoseconds and the strict formatter rejects them.

`func syncSummaryLine(_ status: SyncStatus?) -> String` — pure, returns "" for nil so the
caller hides the row. Portuguese wording, matching every other string in this file. Use a
relative rendering ("há 2 h") via `RelativeDateTimeFormatter`; it is in Foundation, so the
single-file no-dependency build is unaffected.

Every label that reaches an `NSMenuItem` must go through the existing `stripMarkup` if it
came from the binary. The category names are a fixed Rust enum, but `warnings` is free text
that can carry a path, so treat it as untrusted before display.

Add the fetch, modelled on `fetchAccountStatus` and sharing its shape exactly: resolve the
binary, bump a generation counter, dispatch to `.utility`, arm the `REFRESH_TIMEOUT`
watchdog, read the pipe before `waitUntilExit`, and hop back to main guarded by the
generation. Arguments are `["sync", "status", "--json"]`. A non-zero termination status or a
nil parse leaves the previous value alone, exactly as the account fetch does. Call it from
`menuWillOpen` beside the existing account fetch, behind the same 5-second staleness check —
no new timer.

Add one `syncInfoItem` to `buildMenu`, placed after `accountsInfoItem`, disabled, hidden when
`syncSummaryLine` returns empty, rendered with `run(line, .secondaryLabelColor)` like the
accounts line. Do not add a submenu, an action, or a keyboard shortcut here — actions are
6-02's, and a row that can be clicked before the confirmation flow exists is a row that can
push by accident.

In `macos/ai-usagebar-tests.swift` add `testSyncStatus()` covering every case above and
register it in `TestRunner.main()`. Follow the file's existing style: `assertEqual` /
`assertNil` on pure calls over `Data` literals, no process spawn, no filesystem outside
`NSTemporaryDirectory()`.
  </action>
  <verify>
    <automated>./macos/run-tests.sh</automated>
  </verify>
  <done>The harness passes with the new cases; a malformed or absent payload hides the row instead of crashing; the menu bar contains no code that reads a keyfile, a token, or a pack.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|---|---|
| local index + scanned tree → `status_json` | Paths and sizes of credential-bearing files cross into a machine-readable document |
| `ai-usagebar` stdout → menu-bar process | Subprocess output is parsed and rendered in a UI that also renders trusted chrome |
| binary-supplied strings → `NSMenuItem` titles | Free text (warnings, paths) reaches a UI surface |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|---|---|---|---|---|---|
| T-6-01 | Information disclosure | `status_json` | high | mitigate | Only counts, byte totals, category labels, an index path, and fixed-vocabulary warnings are serialized. A test walks the produced `Value` and fails on any string leaf outside that vocabulary, so a future field cannot smuggle a file body in |
| T-6-02 | Information disclosure | `warnings` array | medium | mitigate | Warnings are composed from the crate's own error text, which already excludes secrets; the Swift side passes them through `stripMarkup` before display so a path cannot inject UI markup |
| T-6-03 | Denial of service | `sync status --json` on a huge transcript tree | medium | mitigate | The pending count uses index lookups only and opens no file body; the walker's existing entry cap is reported through `capped`, which the JSON carries |
| T-6-04 | Denial of service | menu-bar subprocess hang | medium | mitigate | The existing `REFRESH_TIMEOUT` watchdog terminates the child; the generation guard discards a late reply so a slow call cannot overwrite a newer one |
| T-6-05 | Tampering | a hostile `ai-usagebar` earlier on `PATH` | low | accept | `resolveBinary` already prefers a configured path then fixed system locations; an attacker who can write those directories already owns the account. Unchanged from the existing account-status path |
| T-6-06 | Spoofing | malformed JSON read as a valid state | medium | mitigate | Every field is optional and a non-object parses to nil; nil, false and true stay three distinct states so "unknown" is never rendered as "up to date" |
| T-6-SC | Tampering | dependency surface | low | accept | Zero new crates and zero new Swift dependencies. `serde_json` and `chrono` are already declared; `ISO8601DateFormatter` and `RelativeDateTimeFormatter` are Foundation. `Cargo.toml` is not in this plan's `files_modified` — an edit there means the design drifted |
</threat_model>

<verification>
- `cargo test --lib -- sync::report sync::cli` is green. Multi-filter uses the
  `cargo test --lib -- a b` form; `cargo test --lib a b` takes one positional only.
- `./macos/run-tests.sh` is green.
- `ai-usagebar sync status --json | python3 -m json.tool` parses, and
  `ai-usagebar sync status --json` writes exactly one line to stdout.
- `git diff --stat` touches no file under `gnome-extension/`, `kde-plasmoid/`, or `omarchy/`
  (D-05), and does not touch `Cargo.toml`.
- No new Rust test constructs `Config::load`, `SyncRoots::resolve`, `index::default_path`, or
  `Utc::now`; no new Swift test spawns a `Process`.
</verification>

<success_criteria>
The menu bar shows when sync last ran and whether local changes are pending, reading it from
the CLI's own JSON, with no sync logic in Swift and no way to trigger a remote operation yet.
An older binary that does not know `--json` hides the row rather than breaking the menu.
</success_criteria>

<output>
Create `.planning/phases/06-surfaces-and-ship/6-01-SUMMARY.md` when done.

Record the **exact key set** of `status_json`'s object, including which keys are nullable —
6-02 parses it in a separate worktree and must not re-derive it from the diff. Record the
signatures of `status_with`, `PendingSummary`, `parseSyncStatus`, `syncSummaryLine`, and the
name of the menu item 6-02 will attach actions beside.

Record whether Phases 3–5 already supplied a `repo` field or a non-interactive flag on the
sync subcommands, and if so their exact spelling — 6-02's precondition turns on that answer.
</output>
