---
phase: 05-pull-and-restore
plan: 06
subsystem: sync/restore
tags: [restore, report, dry-run, gates, consent, terminal-escapes, line-budget]
status: complete
requires:
  - "sync::restore::{Disposition, ItemPlan, RestorePlan, RestoreOutcome, RestoreOptions, BackupRecord} (5-01, frozen)"
  - "sync::report::human_bytes (Phase 2) — the crate's one byte formatter"
  - "display::sanitize_untrusted_field (pre-existing) — the crate's one terminal-escape stripper"
  - "config::SyncCategory::{ALL, label} — the canonical display order"
provides:
  - "sync::restore::report::render_plan — signature unchanged from 5-01's freeze"
  - "sync::restore::report::render_outcome — signature unchanged from 5-01's freeze"
  - "sync::restore::report::confirm_apply — NEW, D6's one gate"
  - "sync::restore::report::confirm_credentials — NEW, D2's separate consent"
  - "sync::restore::report::{MAX_ITEM_LINES_PER_CATEGORY, MAX_ATTENTION_ITEMS, APPLY_COMMAND}"
affects:
  - "5-07 (sync/cli.rs) — wires both gates and must spell the subcommand APPLY_COMMAND names"
tech-stack:
  added: []
  patterns:
    - "one exhaustive classification match over Disposition, so a new variant is a compile error rather than an unrendered case"
    - "the security control lives inside the gate, not in a caller that must remember it"
    - "two line budgets, deliberately not unified: the lines the user must read are not truncated to the same limit as ordinary creates"
    - "a remote string is rendered through the crate's sanitizer with newlines collapsed, so it occupies exactly one line and cannot forge a row"
key-files:
  created: []
  modified:
    - src/sync/restore/report.rs
decisions:
  - "confirm_credentials takes &RestoreOptions rather than the plan's opts-free signature, so force_credentials is checked inside the gate and no caller can forget it"
  - "Overwrite joins SkipLocalNewer and NeedsCredentialConfirm in the leading attention block — it destroys a newer local file and is the loudest line in the report"
  - "the line ceiling is 250, not 200, because the attention and refusal blocks got a 4-line-per-item layout rather than a cramped 2-line one"
  - "SectionBuilder::push_metric was checked and does not fit — it is a ratatui projection for vendor usage snapshots, a different domain"
metrics:
  duration: ~45 min
  completed: 2026-08-19
---

# Phase 5 Plan 06: The restore report and its two gates — Summary

`render_plan` puts what needs a decision **above** the table instead of below
it, names the different flag that resolves each kind, shows every refused path
with its reason, and bounds a 5,000-item bundle to under 250 lines without
truncating the half a user actually has to read.

---

## SIGNATURE CHANGE — read this first

`render_plan` and `render_outcome` are **unchanged** from 5-01's freeze. One of
the two new gates differs from what 5-06-PLAN.md specified:

```rust
// 5-06-PLAN.md said:
pub fn confirm_credentials(items: &[&ItemPlan], out: &mut dyn Write, input: &mut dyn BufRead) -> Result<bool>;

// what shipped — `opts` added:
pub fn confirm_credentials(
    items: &[&ItemPlan],
    opts: &RestoreOptions,
    out: &mut dyn Write,
    input: &mut dyn BufRead,
) -> Result<bool>;
```

Rationale in **Deviations** below. Everything 5-07 wires against:

```rust
// src/sync/restore/report.rs

pub const MAX_ITEM_LINES_PER_CATEGORY: usize = 10;   // ordinary create/update/identical
pub const MAX_ATTENTION_ITEMS: usize = 20;           // locally-newer + refusals, ~4 lines each
pub const APPLY_COMMAND: &str = "ai-usagebar sync pull --apply";

pub fn render_plan(plan: &RestorePlan) -> String;
pub fn render_outcome(outcome: &RestoreOutcome) -> String;

pub fn confirm_apply(
    plan: &RestorePlan,
    opts: &RestoreOptions,
    out: &mut dyn Write,
    input: &mut dyn BufRead,
) -> Result<bool>;

pub fn confirm_credentials(
    items: &[&ItemPlan],
    opts: &RestoreOptions,
    out: &mut dyn Write,
    input: &mut dyn BufRead,
) -> Result<bool>;
```

### Three things 5-07 must honour

1. **`APPLY_COMMAND` is a contract, not a hint.** The footer of every dry run
   prints `ai-usagebar sync pull --apply`. If the clap subcommand is spelled
   anything else, the report names a command that does not exist — which is
   this milestone's single most repeated defect. Wire `sync pull` with
   `--apply`, or change the const and this summary together.
2. **`confirm_apply` renders the plan itself.** Do not `print!(render_plan(..))`
   and then call the gate; it writes the whole report to `out` first, then asks.
   Calling both prints it twice.
3. **Both gates read from the injected reader.** `sync/cli.rs` already reads the
   **sync passphrase** off `stdin` (`local_keyfile` → `passphrase::read_line`).
   If both the passphrase and the gate answer come from the same non-TTY stdin
   they will interleave, and whichever reads second gets the other's line. Decide
   deliberately: read the passphrase from `--password-file` / an already-held
   `LocalKeyfile` when the run is interactive, or accept that a piped run must
   supply `--yes` and `--force-credentials` rather than answers. `IsTerminal` is
   read at the call site, exactly as `sync/cli.rs` line 410 and 567 already do.

---

## What was built

### `render_plan` — the order is the requirement

Top to bottom, and the order is not cosmetic:

```
DRY RUN — snapshot 3 of github:1, taken 2023-11-14T22:13:20+00:00
  1 to create, 1 to update, 1 already identical
  3 needing your decision, 2 refused
  would fetch 4.0 KiB in 2 pack(s) and write 3 item(s)

  >> YOUR LOCAL COPY IS NEWER — 3 item(s). Read these before answering.
     config/item-3.json
       SKIPPED, needs your decision — the local file is newer
       local 2023-11-15T00:43:20+00:00 · snapshot 2023-11-14T22:13:20+00:00
       pass --force to restore it
     config/item-4.json
       WILL BE OVERWRITTEN — the local file is newer and will be replaced
       local 2023-11-15T00:43:20+00:00 · snapshot 2023-11-14T22:13:20+00:00
     config/item-5.json
       NEEDS YOUR CONFIRMATION — a credential, and the local one is newer
       local 2023-11-15T00:43:20+00:00 · snapshot 2023-11-14T22:13:20+00:00
       pass --force-credentials to restore it

  >> NOT RESTORED — 2 item(s) in the snapshot were refused:
     config/item-6.json
       excluded by policy — not written to this machine
     config/item-7.json
       REFUSED — the path does not resolve inside the sync roots
       reason: it contains a `..` component

                create   update   identical  needs you   refused    to write
  config             1        1           1          3         2     3.0 KiB
  total              1        1           1          3         2     3.0 KiB

  config:
    create     config/item-0.json  (1.0 KiB)
    update     config/item-1.json  (1.0 KiB)
    identical  config/item-2.json

  Nothing has been written. This is a dry run.
  2 of the 3 item(s) to write replace a file that exists here now.
  Their current contents are archived to a tar.gz in the backups
  directory before the first write. That archive's path, and the one
  command that undoes the whole restore, are printed when the run ends.
  To apply it, run:  ai-usagebar sync pull --apply
```

- **A plain skip and a decision are not the same word.** `SkipLocalNewer` reads
  `SKIPPED, needs your decision`; `NeedsCredentialConfirm` reads `NEEDS YOUR
  CONFIRMATION`; and each carries the flag that resolves it — `--force` for the
  first, `--force-credentials` for the second. Two different flags, both named.
- **A refused path is shown with its name and its reason.** Silently dropping it
  is how a user concludes a restore was complete when it was not.
- **Byte counts go through `sync::report::human_bytes`**, categories through
  `SyncCategory::ALL`. No second formatter, no second order table.
- **Zero writable items** renders `already up to date — this machine matches
  the snapshot. That is a success, not a failure.` A separate arm covers the
  case where nothing would be written *because* items need a decision first —
  that is not "up to date" and does not say so.
- **The archive is named before it happens.** `backup::take` is the user's undo
  and is worthless if they do not know it exists. The footer says how many of
  the writes replace an existing file, that those are archived first, and that
  the archive path plus the rollback command print at the end. When every write
  is a create it says there is nothing to archive rather than promising an
  archive that `backup::take` would correctly return `None` for.

### `render_outcome`

Every overwritten path by name, **never truncated** — SYNC-06 is satisfied by a
list, and a count is a list wearing the wrong clothes. Then the archive path and
`BackupRecord::rollback_command()`. On a partial failure the header names the
manifest path it stopped at, and the rollback command prints a **second time as
the last line**, because the bottom of the output is where a user looks after a
failure. An applied run with overwrites but no `BackupRecord` prints a `WARNING:
… there is no undo command for this run` rather than an absent line.

### The two gates

`confirm_apply` writes the whole report, then one question. `assume_yes`
short-circuits to `true` **after** the report prints and without asking. Only
`y` / `yes` (trimmed, case-insensitive) passes; a bare newline, a `no`, an
unrecognised word, and EOF are all refusals. EOF prints `Pass --yes to apply
without being asked` — a run with nothing on stdin never blocks and never
assumes consent (T-5-55).

`confirm_credentials` reads `opts.force_credentials` and **never**
`opts.assume_yes`. It is not a `[y/N]`: it requires the word `overwrite`. Its
wording names the actual failure mode —

> Restoring these writes the snapshot's older token back over the one this
> machine is using now. If that token has since been rotated, the live one is
> gone and everything authenticated with it stops working until you log in
> again. `--yes` does not answer this question.

### What never reaches the terminal

Paths, byte counts, timestamps and counts. Nothing else is *representable* —
no field of `ItemPlan`, `RestorePlan`, `RestoreOutcome` or `BackupRecord`
carries plaintext, a token, or a URL, so there is no signed-URL query string to
leak (T-5-56). Every remote-chosen string (`manifest_path`, a `RejectedPath`
reason, `repo_id`) goes through `display::sanitize_untrusted_field` with
newlines additionally collapsed to spaces, so a hostile path cannot inject ESC
sequences, bidi overrides, or extra report lines — and therefore cannot slip an
entry past a line budget (T-5-50, T-5-53). The test asserts a hostile string
occupies exactly the same number of lines a benign one does.

### The budgets

| const | value | applies to | lines per item |
|---|---|---|---|
| `MAX_ITEM_LINES_PER_CATEGORY` | 10 | ordinary create / update / identical detail | 1 |
| `MAX_ATTENTION_ITEMS` | 20 | locally-newer block **and** refusals block | up to 4 |

They are **deliberately not unified**, and a doc comment says so: collapsing a
rejected path into "and N more" is exactly how a tampered bundle hides an entry.
Only ordinary creates and updates are cheap enough to truncate hard. A 5,000-item
mixed plan renders under 250 lines, and every line is ≤ 80 columns — both
asserted.

### The exhaustive match

One `fn facing(&Disposition) -> Facing` carries the kind, the user-facing verb,
the resolving flag, and whether the item replaces an existing local file. One
`fn mtimes(&Disposition)` carries the timestamp pair. Both are exhaustive with
no wildcard, so a new `Disposition` is a compile error rather than a case that
renders as nothing. `grep -v '^\s*//' … | grep -c '_ =>'` is **0**, and a test
asserts it on the file's own `include_str!` text (needle built at run time so
the assertion is not the thing it searches for).

---

## Deviations from Plan

### [Rule 2 — missing critical functionality] `confirm_credentials` takes `&RestoreOptions`

The plan's signature omitted `opts`, which structurally guarantees `assume_yes`
cannot answer the gate — but it also leaves `force_credentials` to be checked by
a caller. That caller is 5-07, in a different worktree, and **a security control
a caller has to remember to apply is a control that eventually is not applied**.
This milestone's repeated defect has twice been a missing security control, so
the check moved inside the gate:

```rust
if opts.force_credentials { return Ok(true); }
// opts.assume_yes is DELIBERATELY not read here (T-5-52).
```

The T-5-52 guarantee is preserved by a test that slices `confirm_credentials`'
own source out of `include_str!("report.rs")` and asserts the body does not
contain the string `assume_yes` — plus three behavioural tests that
`assume_yes: true` with `""`, `"\n"` and `"y\n"` all leave the gate refusing.

### [Rule 2] `Overwrite` joins the leading attention block

The plan named the block as "every `SkipLocalNewer` and `NeedsCredentialConfirm`
item". `Overwrite` also carries `local_mtime`/`remote_mtime` and means *your
newer local file will be destroyed* — it is the single most dangerous line the
report can print, and burying it in the ordinary per-category detail (where it
would also have been subject to the 10-line budget) would have been the exact
T-5-53 hole the block exists to close. It renders in the attention block with
`WILL BE OVERWRITTEN — the local file is newer and will be replaced`, and no
flag line, because no flag makes it *not* happen.

`Disposition::writes()` — the frozen predicate — is used for the "would write N
items" count rather than `create + update`, precisely because `Overwrite` writes
and is neither.

### The line ceiling is 250, not the 200 first drafted

The attention and refusal entries were first laid out at two lines per item,
which produced 150-column lines. Wrapping them to a 4-line layout (path / what
happens / both timestamps / the flag) keeps every line ≤ 80 columns at the cost
of a taller block. Legibility of the lines a user *must* read beat a rounder
ceiling. Both bounds are asserted on the same 5,000-item fixture.

### `SectionBuilder::push_metric` was checked and does not fit

CLAUDE.md's rule sends report metrics through `SectionBuilder::push_metric` so
reset metadata travels with its row. That seam lives in `src/tui/panels.rs` and
is a **ratatui `Section` projection for per-vendor usage snapshots** — a
different domain with no bearing on a file restore plan. The rule's actual
intent (one formatter, one order table, metadata travelling with its row) is
honoured through the correct seams: `sync::report::human_bytes` for every byte
count, `SyncCategory::ALL` for order, and both mtimes rendered on the same row
as the item they describe. No second renderer was invented.

### Not done here

`sync pull` on the CLI remains 5-07's, per 5-01's note. Nothing in this module
is reachable from a user until 5-07 wires it.

---

## Known Stubs

None. `report.rs` is complete; the 5-01 stub row for it is closed.

---

## Threat Flags

None. No new network endpoint, auth path, file access pattern, or schema
change — this module reads no file, opens no socket, and its only side effect is
writing to an injected `dyn Write`.

Threat register coverage, all mitigated and asserted:

| ID | Mitigation | Test |
|---|---|---|
| T-5-50 | remote strings through `sanitize_untrusted_field`, newlines collapsed | `a_terminal_escape_in_a_remote_string_never_reaches_the_terminal` |
| T-5-51 | every overwritten path named, never truncated | `an_outcome_names_what_it_overwrote_rather_than_counting_it` |
| T-5-52 | `confirm_credentials` never reads `assume_yes` | `assume_yes_alone_leaves_the_credential_gate_refusing` |
| T-5-53 | attention + refusal blocks lead, with their own larger budget | `locally_newer_items_and_refusals_lead_the_report`, `a_five_thousand_item_plan_stays_under_the_line_ceiling` |
| T-5-54 | per-category budget with "and N more" | `a_five_thousand_item_plan_stays_under_the_line_ceiling` |
| T-5-55 | EOF is a refusal that names the flag; never blocks | `an_empty_stdin_refuses_and_names_the_flag` |
| T-5-56 | nothing here can carry plaintext or a URL | structural; `byte_counts_render_through_the_crates_own_formatting` |
| T-5-SC | no new crates | `git diff Cargo.toml Cargo.lock` empty |

---

## Verification

```
cargo test                                 1410 lib passed, 0 failed  (baseline 1388, +22)
                                           1451 total passed, 0 failed (baseline 1429, +22)
cargo test --lib sync::restore::report     24 passed, 0 failed
HOME= cargo test --lib sync::restore::report  24 passed, 0 failed
cargo clippy --all-targets -- -D warnings  clean
cargo fmt --check                          clean
grep -v '^\s*//' report.rs | grep -c '_ =>'   0
git diff --stat Cargo.toml Cargo.lock      empty — no new crates
grep -cE 'env::var|std::env|dirs::|home_dir|temp_dir|Utc::now|reqwest' report.rs   0
```

Hermetic: no test reads or writes a real `$HOME`/`$XDG` path, opens a socket, or
reads the wall clock. Every timestamp is a `DateTime::from_timestamp` const, and
the two source-shape tests use `include_str!`, which resolves at compile time.

`cargo machete` is not installed on this machine; `Cargo.toml` is byte-identical,
so no dependency could have become unused. `make test`'s frontend contract suites
were not run — no frontend file was touched.

---

## Self-Check: PASSED

- `src/sync/restore/report.rs` — present, modified, the only source file changed.
- Commit `70c6e69` — in `git log` on `gsd/5-06`.
