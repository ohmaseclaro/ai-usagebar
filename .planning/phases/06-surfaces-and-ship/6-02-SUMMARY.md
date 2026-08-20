---
phase: 06-surfaces-and-ship
plan: 02
subsystem: sync/cli + macos menu bar
status: complete
tags: [sync, menubar, d-01, d-02, ux-05, consent, terminal-delegation]
requires:
  - "sync::cli::{local_keyfile, sync_password} (2-01, 5-06) — the two password reads that had drifted"
  - "sync::passphrase::read_line (1-05) — the sanctioned reader both arms now share"
  - "macos: runInTerminal / addAccountScript / resolveBinary / stripMarkup — the delegation shape copied"
  - "6-01: SyncStatus, SyncCategoryLine, parseSyncStatus, syncInfoItem, fetchSyncStatus, renderSyncRow"
provides:
  - "`ai-usagebar sync push` works when typed at a terminal — it asks for the password instead of refusing"
  - "sync::cli::sync_password_from(impl BufRead) — the injected-reader seam for the one password read"
  - "sync::cli::local_keyfile(&Path, may_prompt: bool)"
  - "macos: SyncAction, shellQuote, syncCommand, syncCommandLine, syncTerminalScript, SyncPrompt, syncPrompt, syncCategoryRows"
  - "macos: AppDelegate.{syncSubmenu, syncMenuItem, renderSyncSubmenu, refreshSyncStatus, pushSync, pullSync, confirmSyncInTerminal}"
affects:
  - "6-05 — the README documents `ai-usagebar sync push` at a prompt, which is now true"
  - "6-04 — the TUI faces the same password question and can reuse this answer"
tech-stack:
  added: []
  patterns:
    - "delegate, do not approximate: an operation whose consent this surface cannot carry is handed to a terminal, not run with the flags that skip the questions"
    - "one password read for both arms, over an injected reader, so neither needs a terminal to be covered"
    - "the dangerous property is negative, so the guards are source-level and mutation-checked"
key-files:
  created: []
  modified:
    - src/sync/cli.rs
    - macos/ai-usagebar-menubar.swift
    - macos/ai-usagebar-tests.swift
decisions:
  - "**Option 2 — refuse, executed as delegation.** The menu confirms and opens Terminal.app on the command; it never runs `sync push`/`sync pull` as a subprocess. `read_from_file` stays unwired."
  - "No `--non-interactive` flag, no named exit code, no `--expect`, no dry-run reference line. Under this design every one of them would have had zero production callers — instance 12 of this milestone's most repeated defect."
  - "`sync push` at a terminal now asks for the password (Rule 1). It refused outright, while `sync pull` asked — one password, two reads, drifted apart, and the push side lost."
  - "`keys_at` keeps the refusal: `sync status` and `push --dry-run` only *want* the third column, so they must not block a terminal on a question they can do without."
  - "`SyncStatus.categories` is now rendered as the submenu's per-category rows, resolving 6-01's render-or-delete."
metrics:
  duration: ~2h
  completed: 2026-08-20
---

# Phase 6 Plan 02: the Sync submenu — Summary

The menu bar can now start a push and a restore. It does it by confirming, showing
the exact command, and opening Terminal.app on it — **not** by running a subprocess.
Along the way `ai-usagebar sync push` became a command you can actually type at a
prompt, which its own documentation has claimed since Phase 3 and which the code
refused to do.

---

## THE PLAN'S PREMISE WAS FALSE, AND THE FINDING IS THE DELIVERABLE

The plan assumed a `--non-interactive` flag with two outcomes: *the key is already
available for this session, so continue*, or *it is not, so refuse*. **There is no
such thing as a session key in this codebase.** Every `sync push`, `sync prune`,
`sync rekey` and `sync pull` derives from a password read fresh off stdin — no
agent, no cache, no unlocked-for-the-session state anywhere. So a
`--non-interactive` push would have refused **100% of the time**, and the Sync
submenu would have been two buttons that always say "run it in a terminal."

Three more facts, each verified in the source rather than assumed:

| what | where | consequence |
|---|---|---|
| `sync pull` reads the password **before it can plan** | `cli.rs::pull` → `sync_password` then `resolve` → `restore::run` | there is no dry run without the password, so the plan's "dry run → confirm → pinned pull" flow needs the password *twice* from a surface that has it zero times |
| both of pull's confirmations are offered **only on a terminal** | `PullIo::gate` is `Some` only when `stdin().is_terminal()` | piped, they are not asked — they are *answered*, by `--apply`, `--yes` and `--force-credentials` |
| `sync push --dry-run` also wants the password | `dry_run` → `try_plan` → `keys_at` → `local_keyfile` | it degrades (`no_key`, third column omitted) rather than refusing — which is why `keys_at` must keep its non-asking behaviour |

Put together: **a menu-driven pull could only reach a restore by passing the three
flags that skip the two human confirmations.** That is an easier path to an
irreversible write than the CLI offers, which is precisely the regression this
phase's hard rule forbids. It does not matter that every line would work.

## THE DECISION — Option 2, executed as delegation

`runInTerminal` already exists in this file, and "Adicionar conta…" already uses
it for exactly this reason: a sign-in is interactive, so the menu collects what it
can in a dialog and hands the operation to a terminal. Sync is the same shape.

So the two items **confirm, show the command, and open Terminal.app**. In that
window the CLI asks for the password itself, prints the restore plan itself, and
runs both of its gates itself. Nothing is approximated, nothing is skipped, and no
password touches Swift — where a `String` cannot be zeroized anyway, undoing the
care `Zeroizing` takes on the Rust side.

**Option 1 (`read_from_file`) was rejected.** It would have required the menu to
name a path to a plaintext passphrase on disk in argv — which the plan's own Task-2
assertion forbids, and which removes the human from an irreversible remote
publication. `passphrase::read_from_file` therefore **still has zero callers**;
6-01 logged it in `deferred-items.md` and it stays there. Not wiring it is the
honest outcome, not an oversight: the only caller that would have justified it is
the caller this plan deliberately does not create.

**A third way was considered and rejected too**, and is recorded so it is not
re-proposed: a native `NSSecureTextField` piping the password to the child's stdin.
Stdin is a sanctioned input, so it is not forbidden by the letter of the rule — but
a Swift `String` cannot be reliably wiped, may be copied by CoW, and can land in a
crash dump. It would trade a real memory-hygiene guarantee for a convenience.

### What that means for UX-05

**Met, with the mechanism stated plainly:** the dropdown can start a push and a
restore, and it does so "reusing the existing non-interactive-subprocess
conventions" by concluding — correctly — that these two operations are not
non-interactive-subprocess work. The ROADMAP's success criterion 2 is met in full:
a sync needing a password this surface cannot ask for says so, names where to go,
and never hangs.

**Left out, and said so:** there is no in-menu dry-run preview and no in-menu
`--apply`. Both need the password. The pull confirmation names `--apply` so the
user types it themselves, in the terminal where the credential gate can answer.

---

## THE RUST FIX — one password, one read (Rule 1)

`docs/sync-github.md` documents this:

```bash
ai-usagebar sync push
```

Typed at a prompt, it answered `the sync password must be piped in on stdin; this
build has no interactive prompt` and did nothing. Meanwhile `sync pull` on the same
terminal announced `The sync password for this bundle. It is echoed…` and asked.
One password, two reads, written two phases apart, drifted.

Root cause, not symptom: **push, prune and rekey all route through
`local_keyfile`**, so one change fixes all three.

```rust
// the tested half — no terminal, no real stdin
fn sync_password_from(r: impl BufRead) -> Result<Zeroizing<String>, String>;
// the production wrapper: announce the echo, then read stdin
fn sync_password(interactive: bool) -> Result<Zeroizing<String>, String>;

fn local_keyfile(path: &Path, may_prompt: bool) -> Result<LocalKeyfile, String>;
```

`may_prompt` is the whole of the split:

- `push` / `prune` / `rekey` → `true`. They **need** the password; on a terminal
  they now ask for it, through the read `sync pull` has used since Phase 5.
- `keys_at` (the only caller, from `try_plan`) → `false`. `sync status` and
  `sync push --dry-run` only *want* the third column and print the report without
  it. Asking would turn a read-only command that answers instantly into one that
  blocks a terminal. It keeps a refusal, reworded so the two are distinguishable:
  `…this command does not ask for one`.

No new flag, no new exit code, no new dependency, nothing added under `src/sync/`
that the environment/argv guard could catch.

### An aside that is evidence, not trivia

`cargo test --lib sync::cli` **hangs** when its stdin is an inherited open pipe —
a stale hung test binary from a prior session was still resident when this plan
started. That is T-6-30 reproducing itself inside our own test harness: a stdin
read on a non-terminal that never closes. It is the strongest argument available
for why the menu bar must not run these as a background subprocess, where its stdin
is whatever the launch agent handed it. Run it as `cargo test --lib … < /dev/null`.
Shipped behaviour is unaffected and unchanged by this plan.

---

## THE SWIFT SURFACE

```swift
enum SyncAction: CaseIterable { case push, pull }

func shellQuote(_ s: String) -> String
func syncCommand(_ action: SyncAction) -> [String]            // ["sync","push"] | ["sync","pull"]
func syncCommandLine(binary: String, _ action: SyncAction) -> String
func syncTerminalScript(binary: String, _ action: SyncAction) -> String
struct SyncPrompt { var title, body, go: String }
func syncPrompt(_ action: SyncAction) -> SyncPrompt
func syncCategoryRows(_ status: SyncStatus?) -> [String]
```

**The Sync submenu**, attached beside the account submenus (the info row stays
where 6-01 put it, as a sub-line of the header — every item that *does* something
lives in the action block):

```
Sync ▸  Configuração: 2 arquivos (4 KB)
        Credenciais: 1 arquivo (700 bytes)
        Rotinas: vazio
        Transcrições: desativado
        ────────────────
        Atualizar estado
        ────────────────
        Enviar agora…
        Restaurar deste backup…
```

Hidden entirely until `sync status --json` answers, on the same condition as the
row — a binary that predates the flag shows the menu it always did.

**`SyncStatus.categories` is now rendered**, resolving 6-01's render-or-delete: a
category that is switched **off is shown as off** rather than omitted, because
"Credenciais: desativado" is exactly what a user needs to discover *before* the
machine they thought they were backing up dies.

**`shellQuote` was hoisted** out of `addAccountScript`'s nested copy and is now
shared. One quoting rule, two script builders.

**No `keyEquivalent` on either action.** Every other item in this menu is read-only
or locally reversible; a hot-key that publishes bytes is not something to hand out
by accident.

**No in-flight flag**, deliberately, and this is a real difference from the plan:
nothing is in flight. `confirmSyncInTerminal` shows a modal and calls `osascript`.
There is no subprocess to guard, no watchdog to arm, and no failure text to
surface — the CLI's own output goes to the terminal window the user is looking at.
Adding `accountSwitchInFlight`'s twin would have been a flag that is never true.

---

## CALL-SITE AUDIT — every symbol added, and its production reader

Requested explicitly. **Zero symbols have zero production callers.**

| symbol | production call site |
|---|---|
| `sync_password_from` | `sync_password` |
| `sync_password` | `local_keyfile` (→ push, prune, rekey), `pull` |
| `ECHOED_PROMPT` | `sync_password` |
| `NO_PASSWORD` | `sync_password_from` |
| `NO_PROMPT_HERE` | `local_keyfile` |
| `local_keyfile(_, may_prompt)` | `keys_at` (false), `push`, `prune` (true); `rekey` via `local_keyfile_with` |
| `SyncAction` | `syncCommand`, `syncPrompt`, `syncCommandLine`, `syncTerminalScript`, `confirmSyncInTerminal` |
| `shellQuote` | `syncCommandLine`, `addAccountScript` |
| `syncCommand` | `syncCommandLine` |
| `syncCommandLine` | `syncTerminalScript`, `confirmSyncInTerminal` (the alert body) |
| `syncTerminalScript` | `confirmSyncInTerminal` |
| `SyncPrompt` / `syncPrompt` | `confirmSyncInTerminal` |
| `SYNC_CATEGORY_NAMES` | `syncCategoryRows` |
| `syncCategoryRows` | `renderSyncSubmenu` |
| `renderSyncSubmenu` | `renderSyncRow` (← `fetchSyncStatus`'s main-thread hop) |
| `syncSubmenu` / `syncMenuItem` | `buildMenu`, `renderSyncSubmenu` |
| `refreshSyncStatus` / `pushSync` / `pullSync` | `renderSyncSubmenu`'s `addAction` |
| `confirmSyncInTerminal` | `pushSync`, `pullSync` |
| **`SyncStatus.categories` / `SyncCategoryLine`** | **`syncCategoryRows`** — zero readers in 6-01, one now |

**Still zero, reported rather than hidden:** `sync::passphrase::read_from_file`.
Unchanged by this plan, and the decision above is why.

`menubarSourcePath` is test-only by construction — it is the source-guard's path
helper and lives in the harness file.

---

## Tests

**Rust (3 new)** — all in `src/sync/cli.rs`'s `#[cfg(test)]`, all hermetic:

- `the_sync_password_is_one_line_off_the_reader_and_an_empty_stream_is_refused` —
  drives `sync_password_from` over byte slices. Both empty forms (`""` and `"\n"`)
  refuse with the named message rather than spending a gibibyte hashing the empty
  string, only the first line is taken, and no refusal echoes what it read.
- `a_missing_or_unreadable_keyfile_is_refused_before_any_password_is_wanted` — for
  **both** `may_prompt` values, so the ordering property is pinned on both arms.
  This is also why the test cannot hang: neither arm reaches a read.
  Uses `.err()` rather than `expect_err`, because `LocalKeyfile` holds live keys
  and must never gain a `Debug` a panic message could print.
- `the_read_only_report_says_it_does_not_ask_rather_than_asking` — the two messages
  stay distinguishable.

None reads a real `$HOME`, opens a socket, or touches the process's real stdin.

**Swift (47 new assertions, 192 → 239)** — `testSyncActions()`, registered in
`TestRunner.main()`. Pure calls over literals; no `Process`, no network, no clock.
The interesting ones are negative:

- **Nine forbidden spellings** — `--apply`, `--yes`, `--force`,
  `--force-credentials`, ` -y`, `--password`, `--password-file`, `--passphrase`,
  `--non-interactive` — asserted absent from every string either action produces,
  built from character fragments so the assertion cannot be satisfied by its own
  source text.
- **Two source-level guards**, because "no call site does X" is not otherwise
  reachable from a harness that cannot click a menu. Both are preceded by a
  positive marker assertion so they cannot pass against an empty read:
  1. the app builds **exactly one** sync argument vector and it is
     `["sync","status","--json"]`;
  2. `syncCommand`'s output never appears on a line with `arguments` or `Process`,
     and `syncTerminalScript` is handed to `runInTerminal`.
- **Both guards were mutation-checked**: adding `--apply` to `syncCommand(.pull)`
  and swapping the status vector for `["sync","push"]` each produced failures, so
  neither guard is vacuous.
- Quoting: a binary path containing `'` is escaped, not passed through.
- The confirmations: push states the publication is irreversible and that the
  password is asked for in the terminal; pull states the bare command writes
  nothing and names `--apply`.

---

## Deviations from Plan

### 1. [Rule 4 → resolved in-plan by the phase's own hard rule] No subprocess, no `--non-interactive`, no `--expect`

- **Found during:** Task 1's precondition, which asks exactly this question.
- **Issue:** the plan's Rust half exists to serve a background-subprocess caller.
  That caller cannot supply a password, and could reach a restore only by passing
  the flags that skip pull's two confirmations.
- **Fix:** the caller is not created. `src/widget/cli.rs` is **unmodified** — the
  flag, the named exit code, `sync pull --expect <ref>` and the dry-run reference
  line are all absent, because under this design each would have had zero
  production callers. No architectural checkpoint was raised: the phase's own
  instruction is that an action whose confirmation cannot be carried in a menu is
  left out and reported, which is what this is.
- **Commits:** 89a10ed, 389f86e

### 2. [Rule 1 — bug] `sync push` refused on a terminal while its docs documented it

- **Found during:** designing the delegation — the command the menu hands over did
  not work.
- **Issue:** `local_keyfile` refused on a TTY; `sync_password` (pull) asked.
- **Fix:** one shared read, `may_prompt` splitting the required path from the
  optional one. Root cause: all three of push/prune/rekey route through the one
  function.
- **Commit:** 89a10ed

### 3. [design] `syncCategoryRows` instead of `pullConfirmation`

6-01 parked `SyncStatus.categories`. With no dry-run dialog to build, the honest
render is the submenu's per-category breakdown. Two formerly-dead types now have a
reader.

### 4. [design] No in-flight flag

Nothing is in flight. See above.

---

## Verification

| gate | result |
|---|---|
| `cargo test` | **1534 lib / 1594 total passed, 0 failed** (baseline 1531 / 1591 → **+3**) |
| `cargo clippy --all-targets -- -D warnings` | clean |
| `cargo fmt --check` | clean |
| `make test` | green — cargo plus the GNOME, KDE and Omarchy Node contract suites |
| `./macos/run-tests.sh` | green — **239 assertions** (baseline 192 → **+47**) |
| `swiftc -O -parse-as-library` without the harness flag | the shipped app binary builds standalone, no warnings |
| `Cargo.toml` / `Cargo.lock` | **unchanged** — zero new crates, zero new Swift dependencies (T-6-SC) |
| `git diff --name-only` | exactly three files. Nothing under `gnome-extension/`, `kde-plasmoid/`, `omarchy/` (D-05); nothing under `src/tui/` (6-04's); `src/widget/cli.rs` untouched |

**`./macos/run-tests.sh` is not part of `make test`** — `make test` is `cargo test`
plus three Node suites, and the Swift harness needs `swiftc`. Run explicitly here
because this plan touches `macos/*.swift`.

**Not run:** the plan's `<human-check>` for Task 3 no longer applies as written —
there is no background subprocess to kill the network on, and no dry-run dialog to
race. What remains checkable by hand is: the Sync submenu appears with its category
rows, each action confirms before doing anything, and confirming opens a Terminal
window running the command shown in the dialog. Recorded for 6-05's UAT sweep.

---

## Known Stubs

None. Every symbol added has a production reader (audit above), and no placeholder
value reaches the UI.

**One pre-existing zero-caller function is left in place deliberately:**
`sync::passphrase::read_from_file`. It is 6-01's finding, already logged in
`deferred-items.md`, and this plan's decision is that the caller which would
justify it is one this surface must not have. Wiring it would mean a plaintext
passphrase path on disk named in argv by a menu click.

## Threat Flags

None. No new network endpoint, no new auth path, no new file access, no schema
change. Two registered threats are closed by construction rather than mitigated:
**T-6-38** (remote moving between dry run and click) and **T-6-39** (a confirmation
built from a failed dry run) cannot occur, because the menu builds no dry run and
shows no plan. **T-6-30** (a prompt hanging on a non-terminal stdin) is closed the
same way: the menu starts no process that could read stdin. **T-6-31/T-6-32** keep
their confirmations *and* gain the CLI's own — the push confirmation names the
irreversibility, and the restore's two gates run in the terminal where they were
designed to.

One behaviour change worth a reviewer's eye: `sync push`, `sync prune` and
`sync rekey` now read a password at a terminal where they previously refused. The
password is echoed, exactly as `sync pull` has echoed it since Phase 5, and
`ECHOED_PROMPT` says so before the read.

## Self-Check: PASSED

`src/sync/cli.rs`, `macos/ai-usagebar-menubar.swift` and
`macos/ai-usagebar-tests.swift` all present and modified; commits `89a10ed` and
`389f86e` verified in `git log`.
