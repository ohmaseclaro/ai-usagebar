---
phase: 6
plan: 15
subsystem: sync
tags: [claude-desktop, restore, app-control, consent, macos]
requires:
  - 6-14 (the cookie jar and its store — the writes this brackets)
  - 6-13 (the files-before-stores write order the bracket wraps whole)
  - the local account switch's `claude_desktop::app::AppControl`, reused rather than re-invented
provides:
  - "`sync pull --apply` stops Claude Desktop before the first write it owns and starts it again after the last"
  - "`AppControl::running` — the liveness question on the same seam as the two verbs, so no test asks a real Mac"
  - "`layout::is_claude_desktop_state` + `RestorePlan::touches_claude_desktop` — which bundle entries belong to the running app, derived from the plan's items"
  - "`Disposition::writes_after_consent` — what a pre-gate plan will write once the credential gate is answered"
affects:
  - src/sync/cli.rs
  - src/sync/restore/mod.rs
  - src/sync/restore/layout.rs
  - src/sync/restore/report.rs
  - src/claude_desktop/app.rs
tech-stack:
  added: []
  patterns:
    - "the absence of a control is the macOS gate — `None`, not a stub that pretends to close an app that does not exist"
    - "derive disruption from the plan's items, never from a flag"
    - "a pre-gate plan must be read through what the gate will promote, not through what it writes today"
    - "refuse before the backup: a refusal that costs the machine nothing is a refusal a user can act on"
key-files:
  created: []
  modified:
    - src/sync/cli.rs
    - src/sync/restore/mod.rs
    - src/sync/restore/layout.rs
    - src/sync/restore/report.rs
    - src/claude_desktop/app.rs
    - src/claude_desktop/mod.rs
    - src/claude_desktop/capture.rs
decisions:
  - "Liveness moved onto `AppControl` rather than being read from the free `is_running`. The code that decides *whether* to stop the app has to be as injectable as the code that stops it, or the deciding half is untestable — and a second seam beside the existing one is what the brief forbade."
  - "Claude-Desktop-owned = its live data directory, the `desktop-state` captured out of it, and the two sealed stores that finish that state. Not `meta.json`, not config, not transcripts, not another application's credential."
  - "The bracket lives in `pull_with_parts`, not in `restore::run`. It wraps `backup::take` as well as `write::apply`, which is right — an archive of files a live app is rewriting is an archive of a moving target — and it keeps `restore` free of a host effect."
  - "`Disposition::writes_after_consent`, not `writes`. Restoring a Desktop login onto a Mac already signed in as somebody else is `ReplacesLiveCredential`, which writes nothing until the credential gate says so — the ordinary case, and the one a naive read misses."
  - "A failed quit refuses; a failed relaunch warns. Writing under a live app is the condition this exists to prevent, so it cannot be answered by writing anyway. An app that did not come back is annoying, not data loss."
  - "The disruption is stated in `render_plan`'s footer, above the question `confirm_apply` asks, so the existing consent covers it and `--yes` answers it like the rest. Worded on the plan and not on liveness, so a dry run does not run `osascript` to print a sentence."
metrics:
  duration: ~2h
  completed: 2026-08-21
status: complete
---

# Phase 6 Plan 15: the restore closes the app before it writes what the app has open — Summary

**What a user gets after this:** `ai-usagebar sync pull --apply` on the second
Mac quits Claude Desktop before it writes the app's own state and reopens it
when the run ends, so 6-14's re-sealed cookies actually survive instead of being
overwritten by the running app's in-memory copy on quit. A restore that carries
only config or only transcripts does not touch the app at all. An app the user
had already closed stays closed.

## The judgment calls, and how each was decided

### Only when Desktop's own state is in the plan

Derived per item from the manifest path, in `layout::is_claude_desktop_state`,
and folded over the plan by `RestorePlan::touches_claude_desktop`. Three shapes
count:

| shape | why it is the app's |
|---|---|
| `desktop-data/…` | its live data directory — `claude-code-sessions/**` is what a real bundle lands there, via `chat_index` and `routines` |
| `desktop-profiles/<p>/desktop-state/…` | the Chromium cookie jar and LevelDB trees, captured *out of* the app and destined straight back into it |
| `keystore/desktop-cookies/…`, `keystore/desktop-token-cache/…` | the sealed halves of that same `desktop-state`, asked of `Store::from_manifest_path` rather than re-spelled |

And the exclusions are the half that matters, because they are what keeps the
app open:

`config/…`, `claude-home/…` (including Claude Code's own `.credentials.json`
and every transcript), `cursor-user/…`, `keystore/claude-code-oauth`,
`keystore/cursor-auth`, and — deliberately — `desktop-profiles/<p>/meta.json`,
which is claude-acc's bookkeeping and something the app has never heard of.
`desktop-state` is matched as a whole component, so a file named
`desktop-state-notes` is not mistaken for it, and `desktop-datastore/` is not
mistaken for `desktop-data/`.

The two slashed prefixes are checked against `ROOT_PREFIXES` by a test rather
than being a second spelling of the root table.

### Only relaunch what you stopped

`stop_desktop` returns `Ok(true)` **only** when it really ran `quit`. Every
other path — no control on this platform, nothing of the app's in the plan, the
app already closed — returns `Ok(false)`, and `relaunch_desktop` is reached
through `stopped.then(…)`. A restore never opens an app the user had
deliberately quit.

### A dry run must never quit anything

`stop_desktop` is called at step 4 of `pull_with_parts`, *below* the
`if !opts.apply` arm that returns after rendering. The write path is not
reachable from there, which is D1's existing structure; the app control simply
sits inside it. Asserted on both dry-run arms — the piped one and a gate the
user declines.

### Quitting is disruptive and must not be a surprise

The sentence lives in `report::footer`, so it is inside the report
`confirm_apply` prints before its one question and inside the report the
`--apply` arm prints. Consent for the restore is therefore consent for this, and
`--yes` answers it exactly like the rest — no second prompt was added.

It is conditional on the **plan**, not on liveness ("*If* Claude Desktop is
running, it is closed…"), for two reasons: whether the app is up is a question
for the moment of the write, not for the moment of the report, and asking it
here would run `osascript` on every dry run.

### A failed quit must abort before writing

`stop_desktop` returns `Err`, `pull_with_parts` refuses, and nothing downstream
runs — including `backup::take`. A user who hits this has spent a download and
nothing else: no archive, no partial tree, exactly the posture a declined
credential gate already has. The message names the app and says nothing was
written.

### A failed relaunch is a warning, not a failed restore

`relaunch_desktop` returns the failure as text rather than as an error, and
`relaunch_line` renders it — on stdout under the `RESTORED` summary, and on
stderr when the restore itself failed, so the user is never silently left
without their app. Same shape as `prune_warning` on the push side. Exit code
stays 0.

## Order against 6-14 — held

The quit runs before `restore::run`, which is before `backup::take`, which is
before `write::apply`; the relaunch runs after `restore::run` returns, which is
after `write::apply`'s store half. So the bracket contains the whole of
6-13's frozen `sort_by_key` order — the file carrier landing `Cookies` *and* the
keystore's row-write on top of it — rather than sitting between them.

The ordering is asserted against the **write**, not merely as a step list: the
test double samples whether the restored file exists at each verb, and the
assertion is `[("quit", false), ("relaunch", true)]`. A bare `["quit",
"relaunch"]` would pass with the bracket in the wrong place, which is the
negative control below.

## Deviations from plan

**[Rule 1 — bug] `Disposition::writes` was the wrong question, and the case it
missed is the ordinary one.**

`pull_with_parts` plans with `apply` off, runs the gates against that plan, and
only then re-plans and writes. Reading the first plan with `writes()` misses the
two dispositions the credential gate promotes — and one of them,
`ReplacesLiveCredential`, *is* "restoring a Claude Desktop login onto a Mac
already signed in as somebody else". That is the common second-Mac path, and the
one write the app most needs to be closed for. It would have read as "writes
nothing of the app's" and left the app running.

Fixed with `Disposition::writes_after_consent`, which is not a widening of
`writes`: by the time anything reads it, the gate has been answered `yes` or the
run has already returned. Committed separately (`ca96ce4`) with its own test.

## Non-negotiables, held

- **No crate added.** `Cargo.toml` and `Cargo.lock` are byte-identical —
  `git diff HEAD~2 --name-only | grep -c Cargo` → **0**; the seven files this
  plan touches are all under `src/`.
- **macOS-gated, with no Linux stub.** Every other platform gets `None`, which
  `stop_desktop` reads as "nothing to stop". There is no second implementation
  of `quit`/`relaunch` anywhere, and nothing on Linux pretends there is an app.
- **No test touches the real app.** `AppControl` is injected end to end; the
  double is local to `sync::cli`'s tests and its `archive`/`restore` arms are
  `unreachable!()`. `Recorder` answers `running` with `false` — a recorder
  stands in for a run with no host effect, so the answer that causes none is the
  honest one. Nothing in the crate calls `app::is_running` from a test.
- **No secret and no attacker-controlled path in any message.** The three new
  message sites carry a fixed sentence plus an `AppError` this crate wrote; no
  manifest path, no cookie value, no credential. `render_plan`'s new lines are
  constant text.
- **The existing consent gates are unchanged.** No new prompt, no new flag.

## Tests

`cargo test` **1865 passing, 0 failing** (baseline 1853, **+12**).
`cargo clippy --all-targets -- -D warnings` clean, `cargo fmt --check` clean,
`make test` green including the GNOME, KDE and Omarchy contract suites.

**Linux** in `rust:1.88` under Docker: clippy clean, `cargo test --lib` 1792
passed / 1 failed — `supergrok::acp::tests::missing_binary_has_a_clear_non_secret_error`,
which fails identically on untouched `main`. Pre-existing, not this plan's.

By the property asked for:

| property | test |
|---|---|
| plan touches Desktop state, app running → quit before the first write, relaunch after the last | `cli::a_restore_of_desktop_state_stops_the_app_before_the_first_write_and_starts_it_after` |
| app not running → neither is called, and the restore still writes | `cli::an_app_that_is_not_running_is_neither_stopped_nor_started_and_the_restore_still_writes` |
| plan touches no Desktop state → neither is called even with the app running | `cli::a_restore_that_writes_nothing_of_the_apps_leaves_a_running_app_alone` |
| dry run → neither is called, ever (piped *and* declined gate) | `cli::a_dry_run_never_touches_the_app` |
| quit fails → nothing written, no archive taken | `cli::an_app_that_will_not_stop_aborts_the_restore_before_anything_is_written` |
| …and the error names the app | `cli::the_refusal_names_claude_desktop_and_says_nothing_was_written` |
| relaunch fails → the restore still reports success, with a warning | `cli::a_relaunch_that_fails_is_a_warning_on_a_restore_that_still_succeeded` |
| the consent report says it, and only when the plan writes the app's state | `cli::the_report_says_the_app_will_be_closed_only_when_the_plan_writes_its_state` |
| which manifest entries are the app's, and which are not | `layout::only_the_apps_own_state_is_the_apps` |
| the owned prefixes are the root table's, not a second spelling | `layout::the_owned_prefixes_are_the_ones_in_the_root_table` |
| the predicate over a whole plan, in its three answers | `restore::only_a_plan_that_really_writes_the_apps_state_reaches_it` |
| a login awaiting the credential consent still counts | `restore::a_desktop_login_awaiting_the_credential_consent_still_counts` |

### Negative controls — run after committing, each reverted

| break | test that failed |
|---|---|
| move the quit *after* `restore::run` | `a_restore_of_desktop_state_…` — `[("quit", true), …]` vs `[("quit", false), …]` |
| drop the `touches_claude_desktop()` check, quit always | `a_restore_that_writes_nothing_of_the_apps_leaves_a_running_app_alone` |
| ignore the liveness answer, relaunch regardless | `an_app_that_is_not_running_is_neither_stopped_nor_started_…` |
| answer a failed quit with `Ok(false)` and write anyway | `an_app_that_will_not_stop_aborts_the_restore_before_anything_is_written` |
| treat a failed relaunch as a failed restore | `a_relaunch_that_fails_is_a_warning_on_a_restore_that_still_succeeded` |
| read the pre-gate plan with `writes()` instead of `writes_after_consent()` | `a_desktop_login_awaiting_the_credential_consent_still_counts` |
| move the bracket above the apply gate, so a dry run closes the app | `a_dry_run_never_touches_the_app` |

Seven breaks, seven distinct failures, tree restored and re-verified green
(1865 / 0, clippy clean, fmt clean) afterwards.

## Production call sites — every symbol added, enumerated

Counted over production code only. **No symbol has zero.**

| symbol | sites | reached from |
|---|---|---|
| `AppControl::running` | 1 | `cli::stop_desktop`; implemented by `DesktopApp` over the existing `app::is_running` |
| `layout::is_claude_desktop_state` | 1 | `RestorePlan::touches_claude_desktop` |
| `RestorePlan::touches_claude_desktop` | 2 | `cli::stop_desktop`, `restore::report::footer` |
| `Disposition::writes_after_consent` | 1 | `RestorePlan::touches_claude_desktop` |
| `cli::stop_desktop` | 1 | `pull_with_parts` step 4 |
| `cli::relaunch_desktop` | 1 | `pull_with_parts` step 6 |
| `cli::relaunch_line` | 2 | the succeeded and the failed arms of `pull_with_parts` |
| `layout::DESKTOP_DATA_PREFIX` | 1 | `is_claude_desktop_state` (plus the root-table drift test) |
| `layout::DESKTOP_PROFILES_PREFIX` | 1 | `is_claude_desktop_state` (plus the root-table drift test) |

`PullIo::app` is populated in `pull()` on both cfg arms.

## What this does *not* do

- **It does not make the app pick up a restored profile.** The bundle carries
  the claude-acc profile store; putting a profile into the live app is still the
  account switch's job, and that already brackets itself. What this plan
  protects is the write itself — and `desktop-data/claude-code-sessions/**`,
  which does land in the live directory.
- **It is not verified against a real Claude Desktop.** Every test drives a
  double, by design. The one acceptance test still outstanding is 6-14's, and it
  is unchanged: watch the app open signed in on Mac B.
- **Linux and Windows get nothing here**, deliberately. There is no Claude
  Desktop to control and no code pretending otherwise.

## Commits

| commit | what |
|---|---|
| `e0304d7` | the bracket: `AppControl::running`, the owned-state predicate, `stop_desktop`/`relaunch_desktop`, the report sentence, and eight tests |
| `ca96ce4` | a Desktop login awaiting the credential consent still closes the app |

## Known Stubs

None.

## Self-Check: PASSED

Every file named above exists, both commits are in the branch's history, and
the tree is clean and green (`cargo test` 1865/0, clippy clean, fmt clean,
`make test` green) after the negative controls were reverted.
