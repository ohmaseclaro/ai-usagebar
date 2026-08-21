---
phase: 02-bundle-scope-local-index-dry-run-planning
plan: 04
subsystem: infra
tags: [sync, transcripts, bounds, filesystem-scan, chrono]

requires:
  - phase: 02-bundle-scope-local-index-dry-run-planning
    provides: "`scope::walk`, `scope::CategoryScan` (incl. `excluded_files`/`excluded_bytes`), `SyncRoots::at`, and the pre-wired `collect` Transcripts arm — all from plan 2-01"
provides:
  - "`transcripts::collect_bounded(roots, cfg, now)` — the real body: opt-in, newest-first, both D3 bounds, whole files only"
  - "`CategoryScan.excluded_files` / `excluded_bytes` populated for the transcripts category, so `sync status` and the dry-run can report the remainder left outside the working window"
affects: [2-06-calibration, 2-07-dry-run]

tech-stack:
  added: []
  patterns:
    - "`now: DateTime<Utc>` injected, never `SystemTime::now()` in the collector — the day bound is testable without the wall clock"
    - "Tests set mtimes with stdlib `File::set_times` + `fs::FileTimes` (Rust 1.75+), so no `filetime` dependency was added"
    - "Collection funnels through the one shared walker; this plan adds no second traversal and therefore inherits D2 and the symlink refusal unchanged"

key-files:
  created: []
  modified:
    - src/sync/transcripts.rs

key-decisions:
  - "The byte bound *stops* the selection rather than skipping the oversized file and back-filling smaller older ones. A newest-first working window that back-fills is no longer a window — and on real data the file that stops it is a 50 MiB transcript, so back-filling would have swapped one recent conversation for dozens of older ones."
  - "The age bound and the byte bound are evaluated per entry in one newest-first pass, not as two filters. Because the sort is monotone in mtime, the age check short-circuits naturally and the two bounds compose without a second sort."
  - "`cutoff_ns` uses `TimeDelta::try_days` + `checked_sub_signed` and falls back to `i128::MIN` (no age bound) instead of panicking. `transcript_days` is user config; a silly value must not abort a scan."
  - "`excluded_files`/`excluded_bytes` count only what a *bound* dropped — not D2 exclusions and not non-`.jsonl` files. Those are 'never yours to carry', not 'left behind this run', and conflating them would misreport the archive remainder."
  - "`scope.rs` was not edited, as the plan required. `CategoryScan::push` is private to `scope`, so the collector assigns `files`/`bytes` directly — which is exactly the surface plan 2-01 declared public."

patterns-established:
  - "A bound reports its remainder. Dropping data silently is the failure mode; `excluded_files`/`excluded_bytes` is how the user learns 1.66 GiB stayed home."
  - "Whole-file selection only. There is no offset path in this module and none is to be added — a truncated JSONL restores as a conversation that simply stops, which reads as complete and is worse than an absent file."

requirements-completed: [SCOPE-01, SCOPE-02, SCOPE-03]

coverage:
  - id: D1
    description: "Transcripts are absent from the default category set, so an unconfigured user syncs none of them and no directory read happens (T-2-16)"
    requirement: SCOPE-02
    verification:
      - kind: unit
        ref: "src/sync/transcripts.rs#the_default_config_leaves_transcripts_out_entirely"
        status: pass
    human_judgment: false
  - id: D2
    description: "With transcripts enabled, a .jsonl inside the day window is included and one older than transcript_days is excluded and counted"
    requirement: SCOPE-03
    verification:
      - kind: unit
        ref: "src/sync/transcripts.rs#a_transcript_inside_the_day_window_is_selected"
        status: pass
      - kind: unit
        ref: "src/sync/transcripts.rs#the_age_bound_binds_first_when_the_byte_budget_is_ample"
        status: pass
    human_judgment: false
  - id: D3
    description: "Selection stops once transcript_max_bytes would be exceeded, newest first, and the selected total never exceeds the budget (T-2-17)"
    requirement: SCOPE-03
    verification:
      - kind: unit
        ref: "src/sync/transcripts.rs#the_byte_bound_binds_first_when_every_file_is_inside_the_window"
        status: pass
      - kind: unit
        ref: "src/sync/transcripts.rs#the_budget_stops_the_selection_rather_than_back_filling_smaller_files"
        status: pass
    human_judgment: false
  - id: D4
    description: "Selection is per whole file: a file larger than the entire budget is dropped, never partially taken (T-2-18)"
    requirement: SCOPE-03
    verification:
      - kind: unit
        ref: "src/sync/transcripts.rs#a_file_bigger_than_the_whole_budget_is_dropped_not_truncated"
        status: pass
    human_judgment: false
  - id: D5
    description: "The scan reports how many files and bytes the bounds excluded"
    requirement: SCOPE-03
    verification:
      - kind: unit
        ref: "src/sync/transcripts.rs#the_age_bound_binds_first_when_the_byte_budget_is_ample"
        status: pass
      - kind: unit
        ref: "src/sync/transcripts.rs#a_file_bigger_than_the_whole_budget_is_dropped_not_truncated"
        status: pass
    human_judgment: false
  - id: D6
    description: "Collection reaches the tree only through plan 2-01's walker, so D2 (Cowork) and the symlink guard apply unchanged (T-2-19)"
    requirement: SCOPE-01
    verification:
      - kind: unit
        ref: "src/sync/transcripts.rs#cowork_sessions_are_never_selected_even_with_transcripts_enabled"
        status: pass
      - kind: unit
        ref: "src/sync/transcripts.rs#a_symlinked_transcript_is_refused_by_the_shared_walker"
        status: pass
    human_judgment: false
  - id: D7
    description: "Two consecutive scans of the same tree select the same files; a missing projects root is an empty scan, not an error; a non-.jsonl file is never selected"
    requirement: SCOPE-01
    verification:
      - kind: unit
        ref: "src/sync/transcripts.rs#files_with_the_same_mtime_are_ordered_by_path_so_two_runs_agree"
        status: pass
      - kind: unit
        ref: "src/sync/transcripts.rs#a_missing_projects_root_is_an_empty_scan_not_an_error"
        status: pass
      - kind: unit
        ref: "src/sync/transcripts.rs#a_non_jsonl_file_under_the_projects_tree_is_not_selected"
        status: pass
    human_judgment: false

duration: 20min
completed: 2026-08-19
status: complete
---

# Phase 2 / Plan 04: Bounded opt-in transcripts Summary

**`transcripts::collect_bounded` is real: off by default at zero filesystem cost, and when
enabled it selects `~/.claude/projects/**/*.jsonl` newest-first under both D3 bounds —
`transcript_days` and `transcript_max_bytes`, whichever binds first — per whole file, reporting
everything the bounds left behind.**

## Performance

- **Duration:** ~20 min
- **Tasks:** 1 of 1
- **Files modified:** 1 (`src/sync/transcripts.rs`); `scope.rs` untouched, as the plan required
- **Tests:** 12 new, all passing (`cargo test --lib sync::transcripts`)

## Task Commits

1. **Task 1: the bounded transcripts collector** — `d5ee812` (feat)

## How it works

One pass, in this order:

1. `!cfg.includes(Transcripts)` → `CategoryScan::empty` immediately. The default set omits
   transcripts, so the common case costs not one `read_dir`.
2. `scope::walk(claude_home/projects)` — the shared walker, so D2's exclusions (Cowork's
   `local-agent-mode-sessions` among them) and the symlink refusal come for free, and there is
   no second traversal to keep in sync.
3. Keep `.jsonl` only; sort newest-first by `mtime_ns`, tie-breaking on path so two consecutive
   dry-runs cannot disagree about what the budget covered.
4. Walk that order once. Older than `now - transcript_days` → excluded. Otherwise, the first
   entry that would push the running total past `transcript_max_bytes` **stops** the selection;
   it and everything after it are excluded.
5. `excluded_files` / `excluded_bytes` carry the remainder out.

`now` is the injected argument, never `SystemTime::now()` — every test passes a fixed
`2026-08-19T12:00:00Z` and seeds mtimes explicitly with stdlib `File::set_times`.

## Measured shape of the bounds on this machine — input for 2-06's CAL-4

Real `~/.claude/projects`, metadata only (sizes and mtimes; no file body was read):

| Set | Files | Bytes |
|---|---:|---:|
| All `.jsonl` after D2 exclusions | 4208 | 3.65 GiB |
| Inside the 30-day window | 3729 | 3.41 GiB |
| **Actually selected** (both bounds) | **2073** | **1.989 GiB** |
| Excluded by the bounds | 2135 | ~1.66 GiB |

**The byte bound binds first here, not the day bound.** 30 days alone still admits 3.41 GiB, so
the 2 GiB backstop is what actually shapes the bundle: it reaches back to **2026-07-29**, about
21 of the 30 days, and stops on a **50 MiB** transcript that was the 2074th-newest file.

Two consequences worth carrying into 2-06 and the user-facing numbers:

- Quoting "30 days of transcripts" to this user would be wrong. Quote the selected figure —
  ~2073 files / ~1.99 GiB raw, before zstd — and the excluded remainder alongside it.
- Because the stop lands on one of the 14 files over 50 MB, the selection is sensitive to where
  those large files fall in the ordering: a fresh 50 MiB transcript pushes the cut-off later and
  drops roughly its own size in older files. That is the intended behaviour of a working window,
  but it means the per-run file count will visibly wobble; `sync status` showing the excluded
  remainder is what keeps that legible rather than alarming.
- Cowork (`local-agent-mode-sessions/`) currently contributes **0 files** on this machine, so the
  D2 exclusion costs nothing today — it is still load-bearing for other machines and stays.

## Deviations

None. The plan's `<behavior>` list is covered assertion-for-assertion, plus three tests it did
not ask for: the same-mtime determinism tie-break, the Cowork exclusion routed through
`scope::collect` (which also proves the arm wiring), and an absurd `transcript_days` not
panicking.

## Notes for later plans

- **2-07 (dry-run)** — `excluded_files`/`excluded_bytes` are populated only for transcripts;
  every other category leaves them zero. Rendering them unconditionally would print a
  meaningless `0 excluded` on four lines.
- **`transcript_max_bytes` is a bound on *raw* bytes**, before chunking, compression, or dedup.
  The uploaded size is smaller and is the index's business, not this module's.
