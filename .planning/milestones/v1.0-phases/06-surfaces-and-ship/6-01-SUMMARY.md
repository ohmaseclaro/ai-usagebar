---
phase: 06-surfaces-and-ship
plan: 01
subsystem: sync/report + macos menu bar
status: complete
tags: [sync, json-contract, menubar, d-01, d-02, d-03, d-04, pending, tracer]
requires:
  - "sync::report::{build_status, render_status, StatusReport, CategoryLine} (2-01, 2-07, 3-07)"
  - "sync::index::Index::lookup (Phase 2) — the metadata-only short-circuit `pending` reuses"
  - "sync::plan::FilePlan::reused (Phase 2) — the same short-circuit, already recorded"
  - "macos parseAccountStatus / fetchAccountStatus / stripMarkup / resolveBinary — the shapes copied verbatim"
provides:
  - "`ai-usagebar sync status --json` — SyncAction::Status { json: bool }"
  - "sync::report::status_json(&StatusReport) -> serde_json::Value — the single serializer"
  - "sync::report::PendingSummary + StatusReport::pending (three states)"
  - "sync::report::{WARN_INDEX_UNAVAILABLE, WARNINGS} + StatusReport::warnings"
  - "sync::cli::status_with — the injected seam"
  - "macos: SyncStatus, SyncCategoryLine, parseSyncStatus, parseSyncDate, syncSummaryLine, AppDelegate.syncInfoItem"
affects:
  - "6-02 — parses this exact key set and attaches its actions beside `syncInfoItem`"
  - "6-04 — the TUI sync panel shows the same last-sync/pending facts"
tech-stack:
  added: []
  patterns:
    - "one built StatusReport, two renderings, so text and JSON cannot drift"
    - "a fixed warning vocabulary instead of a format string, so a machine-readable document has no hole to smuggle bytes through"
    - "the machine-readable form resolves strictly *less* than the human one"
key-files:
  created: []
  modified:
    - src/widget/cli.rs
    - src/sync/report.rs
    - src/sync/cli.rs
    - macos/ai-usagebar-menubar.swift
    - macos/ai-usagebar-tests.swift
decisions:
  - "`--json` builds no plan and makes no request — the object has no key for either, and a menu-bar subprocess cannot answer a password prompt (D-02) or afford a network round-trip behind a menu gesture (T-6-04)"
  - "`warnings` is filled by `build_status`, not pushed by the caller: with a fixed vocabulary there is no error text to interpolate, and `index == None` is exactly the condition that makes `pending` null"
  - "one date formatter with the fraction stripped, not two ISO8601 formatters — chrono emits 9 fractional digits and `ISO8601DateFormatter`'s fractional mode does not accept them"
  - "the menu row carries no action at all: `sync push`/`sync pull` require consent this surface cannot ask for"
metrics:
  duration: ~2h
  completed: 2026-08-20
---

# Phase 6 Plan 01: `sync status --json` and the menu bar row — Summary

One read-only path proved end to end: local index → `build_status` → `status_json`
→ `ai-usagebar sync status --json` → `parseSyncStatus` → one dim row that says when
sync last ran and how much is not in it. Nothing is pushed, nothing is pulled, and
no password is ever wanted.

---

## THE FROZEN KEY SET — what 6-02 parses

`report::status_json` emits **every key on every run**, so a consumer never has to
tell "absent" from "null".

| key | type | `null` when |
|---|---|---|
| `last_sync` | string, RFC 3339 · **or `null`** | no last-sync record, or no index. **Never the string `"never"`** — the wording belongs to the surface |
| `pending` | bool · **or `null`** | the index would not open. `null` ≠ `false` |
| `pending_files` | number · **or `null`** | same condition as `pending` |
| `pending_bytes` | number · **or `null`** | same condition as `pending` |
| `categories` | array — **never null**, may be empty | — |
| `categories[].category` | string | — |
| `categories[].enabled` | bool | — |
| `categories[].files` | number | — |
| `categories[].bytes` | number | — |
| `categories[].capped` | bool | — |
| `total_files` | number — **enabled categories only** | — |
| `total_bytes` | number — **enabled categories only** | — |
| `index` | string, path · **or `null`** | the index could not be opened |
| `warnings` | array of string — **never null**, empty when clean | — |

`categories[].category` is `SyncCategory::label()`, in `SyncCategory::ALL` order:
`config`, `credentials`, `routines`, `chat_index`, `transcripts`.

`warnings` is a **closed vocabulary**, not free text — `report::WARNINGS`, today one
entry:

```
"the local index is unavailable, so last-sync and pending changes are unknown"
```

There is deliberately no `{}` in it. A format string with a caller-supplied hole is
how a file's bytes reach a document that promised to carry counts and paths (T-6-01).
A new warning is a new `pub const`, added to `WARNINGS` too.

The object is **open**: unknown keys are ignored by the Swift parser, so 6-02 may add
`repo` or anything else without breaking a shipped menu bar.

Real output from the built binary (this machine, `warnings` empty):

```json
{"categories":[{"bytes":183,"capped":false,"category":"config","enabled":true,"files":1},
 …],"index":"/Users/…/Caches/ai-usagebar/sync/index.sqlite3","last_sync":null,
 "pending":true,"pending_bytes":104338300,"pending_files":1650,
 "total_bytes":104338300,"total_files":1650,"warnings":[]}
```

One line on stdout, **zero bytes on stderr**, exit 0.

---

## SIGNATURES 6-02 NEEDS

**Rust — `src/sync/report.rs`**

```rust
pub const WARN_INDEX_UNAVAILABLE: &str = …;
pub const WARNINGS: [&str; 1] = [WARN_INDEX_UNAVAILABLE];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PendingSummary { pub files: usize, pub bytes: u64 }

// new fields on the existing StatusReport (which already carried
// `lines`, `last_sync`, `index_path`, `plan`, `repo` — the plan's claim that it
// held only the first three was stale by Phases 3–5):
pub pending: Option<PendingSummary>,
pub warnings: Vec<String>,

pub fn status_json(report: &StatusReport) -> serde_json::Value;
```

**Rust — `src/sync/cli.rs`** (private to the module; drive it from a `#[cfg(test)]`
in the same file)

```rust
fn status_with(
    roots: &SyncRoots,
    cfg: &SyncConfig,
    index: Option<&Index>,
    now: DateTime<Utc>,
    plan: Option<plan::SyncPlan>,
    repo: Option<RepoSection>,
    json: bool,
) -> (i32, String);
```

Seven arguments deliberately — an eighth trips clippy's `too_many_arguments` under
`-D warnings`.

**Rust — `src/widget/cli.rs`**

```rust
Status { #[arg(long)] json: bool },   // was a unit variant
```

**Swift — `macos/ai-usagebar-menubar.swift`**

```swift
struct SyncCategoryLine: Equatable { var category: String; var enabled: Bool
                                     var files: Int; var bytes: Int }
struct SyncStatus: Equatable { var lastSync: Date?; var pending: Bool?
                               var pendingFiles: Int?; var pendingBytes: Int?
                               var categories: [SyncCategoryLine] = []
                               var warnings: [String] = [] }

func parseSyncStatus(_ data: Data) -> SyncStatus?
func parseSyncDate(_ s: String) -> Date?
func syncSummaryLine(_ status: SyncStatus?, now: Date = Date()) -> String
```

**The menu item 6-02 attaches beside:** `AppDelegate.syncInfoItem`, added in
`buildMenu()` immediately after `accountsInfoItem`. It is `isEnabled = false`, has
no `action` and no `keyEquivalent`. Its companions are `lastSyncStatus`,
`syncStatusFetchedAt`, `syncStatusGeneration`, `fetchSyncStatus()`, `renderSyncRow()`.

---

## WHAT PHASES 3–5 ALREADY SUPPLIED — 6-02's precondition

**A `repo` field: yes.** `SyncConfig.repo: Option<String>`, spelled `[sync] repo =
"owner/name"` in TOML. `StatusReport.repo: Option<RepoSection>` also already existed.
Neither is in `status_json`'s key set — 6-02 may add `repo` freely.

**A non-interactive flag on the sync subcommands: no — there is no such flag.**
Non-interactivity is decided by `std::io::stdin().is_terminal()`, not by an argument.
The flags that do exist, verified in `src/widget/cli.rs`:

| subcommand | flags |
|---|---|
| `sync status` | `--json` *(new, this plan)* |
| `sync setup` | — |
| `sync push` | `--dry-run` · `--allow-rollback` · `--rebuild-index` · `--force-rehash` |
| `sync prune` | — |
| `sync rekey` | — |
| `sync pull` | `--apply` · `--dry-run` (conflicts with `--apply`) · `--force` · `--force-credentials` (`requires = "force"`) · `--allow-rollback` · `-y/--yes` · `--rebuild-index` |

**The blocker 6-02 must design around:** the sync password arrives on **stdin only**
(`cli.rs:512`, `cli.rs:820` → `passphrase::read_line`) — never argv, never an
environment variable — and `local_keyfile` *refuses outright* when stdin is a
terminal ("this build has no interactive prompt"). A menu-bar subprocess therefore
has **no** password path today. `sync::passphrase::read_from_file(&Path)` exists,
checks `0o077` on the opened handle, and **has no caller anywhere in `src/`**; it is
the closest thing to one. Logged in `deferred-items.md`. 6-02 must decide
deliberately: wire it, or refuse the action with "run it in a terminal" (D-02).

---

## CALL-SITE AUDIT — every symbol added, and its production reader

Requested explicitly, because a surface asserting behaviour that does not exist is
this milestone's most repeated defect.

| symbol | production call site |
|---|---|
| `report::status_json` | `sync::cli::status_with` → `status` → `run_with` → `run` |
| `report::PendingSummary` | written by `pending_of_scans` / `pending_of_plan`, both in `build_status`; read by `status_json` |
| `report::WARN_INDEX_UNAVAILABLE` | `build_status`, report.rs:219 |
| `pending_of_scans` / `pending_of_plan` | `build_status`, both arms |
| `cli::status_with` | `cli::status` |
| `SyncAction::Status { json }` | clap → `run_with` match arm |
| `parseSyncStatus` | `fetchSyncStatus` |
| `parseSyncDate` | `parseSyncStatus` |
| `syncSummaryLine` | `renderSyncRow` |
| `pendingLabel` | `syncSummaryLine` |
| `fetchSyncStatus` | `menuWillOpen` |
| `renderSyncRow` | `fetchSyncStatus`'s main-thread hop |
| `syncInfoItem` | `buildMenu` (added), `renderSyncRow` (written) |

**Two with zero production readers, both reported rather than hidden:**

1. **`report::WARNINGS`** — read only by the T-6-01 test, where it *is* the
   allow-list the walk enforces. A test oracle, not a behaviour claim.
2. **`SyncStatus.categories` / `SyncCategoryLine`** — parsed, never rendered. Kept
   because the plan freezes the contract here so 6-02 need not re-derive it, and
   because a parsed field that never reaches an `NSMenuItem` cannot mislead a user.
   If 6-02 does not consume it, delete it there.

**The menu names exactly one command:** `["sync", "status", "--json"]`. Verified
against `src/sync/cli.rs` and against the built binary (exit 0, one line). No other
command name and no other flag string appears anywhere in the Swift diff — grepped,
not assumed. Nothing on this surface can trigger a remote or destructive action, so
no consent is approximated: the row is `isEnabled = false` with no `action`.

---

## Deviations from Plan

### Auto-fixed / design corrections

**1. [Rule 2 — missing safety control] `--json` builds no plan and makes no request**

- **Found during:** Task 1, tracing what `status` actually resolves.
- **Issue:** `status` calls `try_plan` (which reaches `local_keyfile` → `read_line(stdin)`)
  and `resolve_repo_section` (a network round-trip). The JSON key set has no key
  derived from either. A menu bar calling this on every menu open would put a
  stdin read and a GitHub request behind a UI gesture — the exact hang D-02 forbids,
  and file-body hashing `sync status` promises never to do.
- **Fix:** `status(json: true)` passes `plan: None, repo: None`. Every shared key
  (`categories[].files/bytes`, `total_*`, `last_sync`, `index`, `pending`) is
  identical either way, so the two renderings still cannot disagree.
- **Consequence for D-03:** `--json` exits non-zero only on the config/roots failure
  in `run()`. The repository incident's non-zero exit remains on the text path,
  where the section is actually rendered. Pinned by
  `status_json_wants_no_password_and_makes_no_request`.
- **Commit:** 9a03739

**2. [Rule 3 — blocking] `warnings` filled in `build_status`, not pushed by the caller**

- **Issue:** the plan had `sync::cli::status` push the message so the rusqlite error
  text could travel. That text is free-form, which is exactly what T-6-01 says must
  not enter this document — and an eighth parameter on `status_with` trips clippy's
  `too_many_arguments` under `-D warnings`.
- **Fix:** a fixed `WARN_INDEX_UNAVAILABLE` constant, pushed by `build_status` under
  `index == None` — the same condition that makes `pending` null, so the two can
  never disagree. The detailed error still goes to stderr from `open_index`,
  unchanged.
- **Commit:** 9a03739

**3. [Rule 1 — bug] One date formatter, fraction stripped, not two ISO8601 formatters**

- **Issue:** the plan's "fractional formatter, falling back to one without" does not
  actually work. `chrono::to_rfc3339` emits up to **nine** fractional digits;
  `ISO8601DateFormatter.withFractionalSeconds` accepts three. Both formatters would
  reject a nanosecond timestamp and the row would read "nunca" for a machine that
  synced a minute ago.
- **Fix:** strip `\.\d+` and parse with one `.withInternetDateTime` formatter. Less
  code and strictly more robust. Pinned by "a nanosecond timestamp still decodes".
- **Commit:** 2fdfe71

**4. [Rule 2 — D-04] The unknown pending state is rendered, not silently dropped**

- **Issue:** the plan's wording list covered `pending` true and false but not `null`.
  Rendering null as "nothing pending" would draw a machine whose index will not open
  as up to date — the one thing surfacing sync state exists to prevent.
- **Fix:** a fourth case appending the binary's own warning (through `stripMarkup`,
  since it crosses a trust boundary), or "estado desconhecido" if none arrived.
- **Commit:** 2fdfe71

**5. [design] The row carries the pending count and size**

- `pendingFiles` / `pendingBytes` were otherwise parsed-and-never-read. The row now
  says "3 pendentes (99 MB)", degrading to a bare "pendente" when the binary sent no
  count. Same D-04 question, better answered, and two fewer dead fields.
- **Commit:** 2fdfe71

### Not deviations, recorded because the plan's prose was stale

`StatusReport` already carried `plan` and `repo` from Phases 3–5; the plan's
"`lines`, `last_sync`, and `index_path` only" was written before those merged.
`build_status` already took six arguments. Both were extended, not recreated, as the
task precondition required.

---

## Tests

**Rust (11 new)** — `src/sync/report.rs` (8), `src/sync/cli.rs` (3), plus one
assertion in `src/widget/cli.rs`'s existing parse test.

Notable ones:

- `every_string_in_the_document_is_a_label_a_path_a_timestamp_or_a_known_warning` —
  T-6-01. Seeds a config file whose whole contents are `a-secret-that-must-not-travel`,
  builds the document, walks **every string leaf**, and fails on anything that is not
  a category label, a `WARNINGS` entry, an RFC 3339 timestamp, or an absolute path. A
  future field carrying a file body has to break this first.
- `the_pending_count_never_opens_a_file_body` — `chmod 000`s the seeded file and
  asserts it is still counted. Fails the moment anyone adds a read to this path.
  `#[cfg(unix)]`; restores mode 0600 so the `TempDir` cleans up.
- `pending_counts_the_files_the_index_does_not_vouch_for` — 2 files pending, then the
  planner records them, then 0 — via the scan **and** via a plan whose files were all
  short-circuited, so both `build_status` arms are pinned.
- `pending_is_true_false_or_unknown_and_never_flattens_the_third`.
- `the_text_rendering_is_byte_identical_to_what_it_always_printed`.

**Hermetic:** every new Rust test injects roots (`SyncRoots::at`), the index
(`Index::at`), and `now` (`fixed_now()` / `NOW`, fixed timestamps). None constructs
`Config::load`, `SyncRoots::resolve`, `index::default_path` or `Utc::now`. KDF
parameters are `m_kib: 8, t: 1, p: 1` — microseconds, because the AUR `check()` runs
these on an installer's machine. No network: `status_with` takes no `Endpoints`, so
the JSON path structurally cannot make a request.

**Swift (26 new assertions)** — all pure calls over `Data` literals and `Date`
values. No `Process`, no filesystem, no wall clock (`now:` is injected).

---

## Verification

| gate | result |
|---|---|
| `cargo test` | **1523 lib / 1583 total passed, 0 failed** (baseline 1512 / 1572 → **+11**) |
| `cargo clippy --all-targets -- -D warnings` | clean |
| `cargo fmt --check` | clean |
| `make test` | green — cargo + GNOME, KDE and Omarchy Node contract suites |
| `./macos/run-tests.sh` | green — **192 assertions** (baseline 166 → **+26**) |
| `swiftc -O -parse-as-library` without the harness flag | the real app binary builds, no warnings |
| `sync status --json \| python3 -m json.tool` | parses; 1 line on stdout, 0 bytes on stderr, exit 0 |
| `Cargo.toml` / `Cargo.lock` | **unchanged** — zero new crates, zero new Swift dependencies (T-6-SC) |
| `git diff --name-only` | exactly the five files in `files_modified`; nothing under `gnome-extension/`, `kde-plasmoid/`, `omarchy/` (D-05); none of 6-03/6-04's files |

**`./macos/run-tests.sh` is not part of `make test`.** `make test` is `cargo test`
plus three Node suites; the Swift harness needs `swiftc` and runs separately. It was
run explicitly here because this plan touches `macos/*.swift`.

---

## Known Stubs

None. `SyncStatus.categories` is parsed without being rendered — recorded in the
call-site audit above — but it is not a stub: no placeholder value reaches the UI,
and the row is fully wired to real data.

## Threat Flags

None. No new network endpoint, no new auth path, no new file access, no schema
change. The one new trust boundary (binary stdout → `NSMenuItem`) is the one the
threat model already registered as T-6-02 and is mitigated by `stripMarkup` on the
only binary-supplied string that reaches a menu item.

## Self-Check: PASSED

All five modified files present; commits `9751a13`, `9a03739`, `a3a8a8e`, `2fdfe71`
verified in `git log`.
