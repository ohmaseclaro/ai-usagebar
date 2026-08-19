---
phase: 02-bundle-scope-local-index-dry-run-planning
plan: 01
subsystem: infra
tags: [sync, filesystem-scan, rusqlite, clap, serde, symlink-safety]

requires:
  - phase: 01-encrypted-bundle-format
    provides: "src/sync/ module tree, the sealed-object format, and the AAD/id-list caveats carried into this phase"
provides:
  - "`[sync]` config section — `SyncConfig` (categories / transcript_days / transcript_max_bytes) and `SyncCategory` (5 D1 variants + ALL + label)"
  - "`SyncRoots::at` / `SyncRoots::resolve` — the injected-roots seam every collector scans through"
  - "`scope::walk` — the single bounded, symlink-refusing walker, plus `scope::is_excluded` implementing D2 in full"
  - "`scope::collect(cat, roots, cfg, now)` with all five category arms already wired"
  - "`scope::CategoryScan` including the `excluded_files` / `excluded_bytes` fields plan 2-04 fills"
  - "`transcripts::collect_bounded(roots, cfg, now)` — signature fixed, body owned by plan 2-04"
  - "`index::Index::at` / `path` / `last_sync` + `index::default_path`, mode-0600 at creation"
  - "`ai-usagebar sync status` end to end"
affects: [2-02-collectors, 2-03-index-schema, 2-04-transcripts, 2-07-dry-run]

tech-stack:
  added: []
  patterns:
    - "Injected-roots seam (`SyncRoots::at`) mirroring `claude_desktop::Paths::at`"
    - "One shared walker owning the exclusion predicate, so no collector can bypass D2"
    - "`now: DateTime<Utc>` threaded through `collect` so no test reads the wall clock"

key-files:
  created:
    - src/sync/scope.rs
    - src/sync/index.rs
    - src/sync/report.rs
    - src/sync/cli.rs
    - src/sync/transcripts.rs
    - src/sync/plan.rs
  modified:
    - src/config.rs
    - src/sync/mod.rs
    - src/cache.rs
    - src/widget/cli.rs
    - src/bin/ai-usagebar.rs
    - config.example.toml

key-decisions:
  - "`CategoryScan.category` is a plain `SyncCategory`, not `Option<SyncCategory>`: `CategoryScan::empty(cat)` spells every field out rather than deriving `Default`, so downstream plans get the declared surface."
  - "`StatusReport.index_path` is taken from `Index::path()` rather than re-resolved via `index::default_path()`, so `build_status` never touches `$HOME` and stays hermetic. Added `Index::path()` for it."
  - "`MAX_WALK_ENTRIES = 200_000` (vs `context`'s 10_000) — the measured worst case here is 4110 transcripts + ~1300 session indexes, and a cap that trips on real data would report `capped` on every run."
  - "`is_excluded` on a non-UTF-8 filename returns true. It is a security predicate: refuse what cannot be checked."
  - "`cache::xdg_cache_dir` widened from private to `pub(crate)` so `index::default_path` reuses the one cache-path resolver instead of adding a second."

patterns-established:
  - "Exclusion lives in the walker, never in a collector — a category added later inherits D2 rather than remembering it"
  - "A missing root is an empty scan, not an error — an account that never ran the Desktop app simply has no tree"
  - "A disabled category renders as `off`, never as `0` — zero and not-selected are different facts the user is choosing between"

requirements-completed: [SCOPE-01, SCOPE-02, SCOPE-05, UX-02]

coverage:
  - id: D1
    description: "`ai-usagebar sync status` runs against injected temp roots and prints one line per category with file count and raw bytes"
    requirement: UX-02
    verification:
      - kind: unit
        ref: "src/sync/report.rs#a_seeded_tree_renders_every_category_in_d1_order_with_counts_and_bytes"
        status: pass
      - kind: manual_procedural
        ref: "cargo run --bin ai-usagebar -- sync status (exit 0, five category lines)"
        status: pass
    human_judgment: false
  - id: D2
    description: "Transcripts appears in the listing as off, and last-sync reads never"
    requirement: SCOPE-02
    verification:
      - kind: unit
        ref: "src/sync/report.rs#a_category_absent_from_the_configured_set_renders_off_not_zero"
        status: pass
      - kind: unit
        ref: "src/sync/report.rs#no_last_sync_renders_as_never"
        status: pass
    human_judgment: false
  - id: D3
    description: "A symlink pointing outside a scanned root contributes zero files to any category (T-2-01)"
    requirement: SCOPE-01
    verification:
      - kind: unit
        ref: "src/sync/scope.rs#a_symlink_to_a_file_outside_the_root_contributes_nothing"
        status: pass
      - kind: unit
        ref: "src/sync/scope.rs#a_directory_symlink_is_not_descended_into"
        status: pass
      - kind: unit
        ref: "src/sync/scope.rs#an_explicitly_named_path_that_is_a_symlink_is_refused_too"
        status: pass
    human_judgment: false
  - id: D4
    description: "None of the D2 hard-exclusion names, directories, suffixes or the atomic-write tempfile prefix appears in a scan result (T-2-02)"
    requirement: SCOPE-01
    verification:
      - kind: unit
        ref: "src/sync/scope.rs#every_d2_hard_excluded_name_is_rejected"
        status: pass
      - kind: unit
        ref: "src/sync/scope.rs#every_d2_excluded_directory_component_is_rejected"
        status: pass
      - kind: unit
        ref: "src/sync/scope.rs#every_d2_suffix_and_the_atomic_write_tempfile_prefix_are_rejected"
        status: pass
    human_judgment: false
  - id: D5
    description: "An existing config.toml with no `[sync]` section still loads and yields the D6 default category set (T-2-06)"
    requirement: SCOPE-05
    verification:
      - kind: unit
        ref: "src/config.rs#config_without_a_sync_section_still_loads_and_gets_the_defaults"
        status: pass
      - kind: unit
        ref: "src/config.rs#sync_section_round_trips_all_three_keys"
        status: pass
      - kind: unit
        ref: "src/config.rs#sync_defaults_are_the_four_d6_categories_with_transcripts_off"
        status: pass
    human_judgment: false
  - id: D6
    description: "The local index is created mode 0600 before anything is written to it (T-2-03)"
    verification:
      - kind: unit
        ref: "src/sync/index.rs#the_index_file_is_created_mode_0600"
        status: pass
      - kind: unit
        ref: "src/sync/index.rs#opening_a_fresh_index_creates_the_file_and_reports_no_last_sync"
        status: pass
    human_judgment: false
  - id: D7
    description: "A walk wider than its entry cap stops and reports `walk_capped` rather than running unbounded (T-2-05)"
    verification:
      - kind: unit
        ref: "src/sync/scope.rs#a_tree_wider_than_the_cap_stops_and_says_it_was_capped"
        status: pass
      - kind: unit
        ref: "src/sync/report.rs#a_capped_walk_is_reported_rather_than_silently_under_counting"
        status: pass
    human_judgment: false

duration: 25min
completed: 2026-08-19
status: complete
---

# Phase 2 / Plan 01: Bundle-scope tracer Summary

**`ai-usagebar sync status` runs end to end over injected temp roots — `[sync]` config section, the `SyncRoots` seam, one bounded symlink-refusing walker owning D2 in full, a mode-0600 rusqlite index, and a pure report renderer — with the `config` category live and the other four arms pre-wired for the three plans that follow.**

## Performance

- **Duration:** ~25 min
- **Started:** 2026-08-19T16:35:00-03:00
- **Completed:** 2026-08-19T16:50:00-03:00
- **Tasks:** 3 of 3
- **Files modified:** 12 (6 created, 6 modified)

## Accomplishments

- The load-bearing security pieces are proven first, as the plan intended: `scope::walk` refuses symlinks for files and directories alike, `scope::is_excluded` implements every D2 rule inside the walker so no later collector can forget one, and both have a test per class.
- `ai-usagebar sync status` is a working vertical slice — it loads `Config`, resolves `SyncRoots`, scans all five categories in D1 order, and prints counts, bytes, `off` for transcripts and `never` for last-sync.
- All six `src/sync/` scope modules exist and every cross-plan signature is fixed, so 2-02, 2-03 and 2-04 each own exactly one file with no shared edits.

## Task Commits

1. **Task 1: `[sync]` config section, the `SyncRoots` seam, and the module skeleton** — `76fd9d6` (feat)
2. **Task 2: the bounded symlink-safe walker, the D2 exclusion predicate, and the `config` category** — `3300dee` (feat)
3. **Task 3: `ai-usagebar sync status`, end to end** — `dfd89e3` (feat)

## THE SHARED SURFACE — what the three parallel plans build against

Three plans run against this concurrently. **These signatures are fixed; changing one is a
worktree merge conflict.**

### `src/config.rs`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncCategory { Config, Credentials, Routines, ChatIndex, Transcripts }

impl SyncCategory {
    pub const ALL: [SyncCategory; 5];   // canonical D1 order
    pub fn label(self) -> &'static str; // "config" | "credentials" | "routines"
                                        // | "chat_index" | "transcripts"
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]                        // explicit Default impl, not derived
pub struct SyncConfig {
    pub categories: Vec<SyncCategory>,   // default: Config, Credentials, Routines, ChatIndex
    pub transcript_days: u32,            // default 30
    pub transcript_max_bytes: u64,       // default 2 * 1024 * 1024 * 1024
}

impl SyncConfig { pub fn includes(&self, cat: SyncCategory) -> bool; }

// and on Config itself, a plain field so `[sync]` survives deny_unknown_fields:
pub struct Config { /* … */ pub sync: SyncConfig, /* … */ }
```

### `src/sync/mod.rs`

```rust
#[derive(Debug, Clone)]
pub struct SyncRoots {
    pub config_file: PathBuf,           // the effective config.toml
    pub config_dir: PathBuf,            // its parent — where accounts/*/ lives
    pub desktop_data_dir: PathBuf,      // parent of claude-code-sessions
    pub desktop_profiles_dir: PathBuf,  // ~/.claude-acc/profiles
    pub claude_home: PathBuf,           // ~/.claude — scheduled-tasks/, projects/
}

impl SyncRoots {
    /// TEST SEAM. Argument order is exactly the field order above.
    pub fn at(
        config_file: PathBuf,
        config_dir: PathBuf,
        desktop_data_dir: PathBuf,
        desktop_profiles_dir: PathBuf,
        claude_home: PathBuf,
    ) -> Self;

    /// Production only. No test calls this.
    pub fn resolve(config: &Config) -> Result<SyncRoots>;
}
```

### `src/sync/scope.rs`

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub path: PathBuf,
    pub size: u64,
    pub mtime_ns: i128,
    pub inode: u64,        // 0 on non-unix — a sentinel meaning "no opinion"
}

#[derive(Debug, Clone)]
pub struct CategoryScan {
    pub category: SyncCategory,
    pub files: Vec<FileEntry>,
    pub bytes: u64,
    pub excluded_files: usize,   // bound-dropped, NOT D2-excluded. Transcripts only.
    pub excluded_bytes: u64,     // idem — declared here so plan 2-04 owns one file.
    pub walk_capped: bool,
    pub skipped: usize,
}

impl CategoryScan { pub fn empty(category: SyncCategory) -> Self; }

pub fn is_excluded(path: &Path) -> bool;

pub(crate) fn walk(root: &Path, out: &mut CategoryScan);       // recursive, bounded
pub(crate) fn push_path(path: &Path, out: &mut CategoryScan);  // one named file

pub fn collect(
    cat: SyncCategory,
    roots: &SyncRoots,
    cfg: &SyncConfig,
    now: DateTime<Utc>,
) -> CategoryScan;
```

- `collect` returns `CategoryScan::empty(cat)` when `!cfg.includes(cat)` — **without touching the filesystem**.
- All five match arms are already wired. **Plan 2-02 fills the bodies of the
  `Credentials | Routines | ChatIndex` arm** (currently one empty arm with an ownership
  comment — split it into three arms there). Plan 2-04 does **not** edit this file:
  its arm already delegates to `transcripts::collect_bounded`.
- Accumulate into a `CategoryScan` via `walk` / `push_path`; do not re-stat or re-sum
  `bytes` by hand, both helpers maintain it.

### `src/sync/transcripts.rs` — owned by plan 2-04

```rust
pub fn collect_bounded(
    roots: &SyncRoots,
    cfg: &SyncConfig,
    now: DateTime<Utc>,
) -> CategoryScan;   // currently returns CategoryScan::empty(Transcripts)
```

### `src/sync/index.rs` — schema owned by plan 2-03

```rust
pub fn default_path() -> Result<PathBuf>;  // ~/.cache/ai-usagebar/sync/index.sqlite3

pub struct Index { /* conn: rusqlite::Connection, path: PathBuf */ }

impl Index {
    pub fn at(path: &Path) -> Result<Index>;      // creates parent dir; file created
                                                  // mode 0600 BEFORE the connection
                                                  // opens; `meta(k TEXT PRIMARY KEY,
                                                  // v BLOB)` created if absent
    pub fn path(&self) -> &Path;
    pub fn last_sync(&self) -> Option<DateTime<Utc>>;  // meta['last_sync'], RFC3339
}
```

`meta` is deliberately minimal — plan 2-03 adds the file table alongside it, keyed on D5's
`(path, size, mtime_ns, inode)`. Keep the 0600-before-write property when you add a
second table; do not move file creation after `Connection::open`.

### `src/sync/report.rs` — plan 2-07 adds the would-upload column here

```rust
pub struct CategoryLine {
    pub category: SyncCategory,
    pub enabled: bool,
    pub files: usize,
    pub bytes: u64,
    pub capped: bool,
}

pub struct StatusReport {
    pub lines: Vec<CategoryLine>,
    pub last_sync: Option<DateTime<Utc>>,
    pub index_path: PathBuf,   // empty when the index could not be opened
}

pub fn build_status(
    roots: &SyncRoots, cfg: &SyncConfig, index: Option<&Index>, now: DateTime<Utc>,
) -> StatusReport;
pub fn render_status(report: &StatusReport) -> String;   // pure
```

## Files Created/Modified

- `src/config.rs` — `SyncCategory`, `SyncConfig`, `Config.sync`, 4 tests
- `src/sync/mod.rs` — six new `pub mod` declarations, `SyncRoots`
- `src/sync/scope.rs` — walker, D2 predicate, stat helper, `collect`, 11 tests
- `src/sync/index.rs` — `default_path`, `Index::{at,path,last_sync}`, 4 tests
- `src/sync/report.rs` — `CategoryLine`, `StatusReport`, `build_status`, `render_status`, `human_bytes`, 6 tests
- `src/sync/cli.rs` — `run(&SyncAction) -> i32`
- `src/sync/transcripts.rs` — `collect_bounded` signature (plan 2-04)
- `src/sync/plan.rs` — module doc only (plan 2-07)
- `src/cache.rs` — `xdg_cache_dir` private → `pub(crate)`
- `src/widget/cli.rs` — `Command::Sync`, `SyncAction::Status`
- `src/bin/ai-usagebar.rs` — dispatch before the tokio runtime
- `config.example.toml` — documented `[sync]` section

## Decisions Made

- **`CategoryScan.category` is `SyncCategory`, not `Option<SyncCategory>`.** A first pass
  derived `Default` to get `..Self::default()` in `empty()`, which forced the field to an
  `Option`. Reverted: three parallel plans consume this struct, and giving them an `Option`
  they must unwrap to save four lines in one constructor is a bad trade.
- **`StatusReport.index_path` comes from `Index::path()`, not from `index::default_path()`.**
  The plan's verification forbids any test path reaching `default_path`; a `build_status`
  that resolved the path for display would have violated that even though it never read the
  file. Added `Index::path()`. `None` index → empty path → the renderer prints
  `index: unavailable`.
- **`MAX_WALK_ENTRIES = 200_000`, not `context`'s 10_000.** The structure is ported as
  specified, but the measured payload here is 4110 transcripts plus ~1300 session indexes;
  a 10k cap would report `walk_capped` on every real run and make the signal meaningless.
- **`is_excluded` returns true for a non-UTF-8 filename.** It is a security predicate, so
  the fail direction is "refuse what cannot be checked".
- **`is_excluded` checks excluded directory names across the whole path, not just the
  basename.** The walker never descends into one anyway, so this is belt-and-braces for the
  paths added directly by `push_path` — over-excluding is the correct side to err on.
- **The `config` category filters the `accounts/` walk down to `.credentials.json`.** D1
  says `accounts/*/.credentials.json`; an unfiltered walk would have swept a whole
  CLAUDE_CONFIG_DIR account tree (history, todos, projects) into the bundle.
- **`cache::xdg_cache_dir` widened to `pub(crate)`** rather than adding a second
  `directories`-based resolver in `index.rs`.

## Deviations from Plan

Two, both narrow and both in service of a stated constraint:

**1. [Correctness] `Index::path()` added, `report::index_path_hint()` removed**
- **Found during:** Task 3
- **Issue:** The plan's `StatusReport` carries `index_path`, but `build_status`'s signature
  gives it no way to learn that path except by calling `index::default_path()` — which the
  plan's own `<verification>` block forbids any test-reachable path from doing.
- **Fix:** `Index` stores its path; `build_status` reads it off the opened index.
- **Verification:** every `src/sync/` test's roots come from a `TempDir`, and no test module
  under `src/sync/` mentions `SyncRoots::resolve`, `home_dir()`, `default_path()` or
  `Config::load()`.
- **Committed in:** `dfd89e3`

**2. [Scope, additive] `CategoryLine.capped` and `push_path`**
- **Found during:** Tasks 2–3
- **Issue:** `CategoryScan::walk_capped` had no route to the user — T-2-05 requires the cap
  be surfaced, not silent. Separately, `roots.config_file` is a named file with no directory
  to walk, and inlining a stat for it would have put a second, unguarded collection path
  beside the walker.
- **Fix:** one extra `CategoryLine` field, rendered as `(capped)`; `push_path` applies
  `is_excluded` and the same symlink refusal to a single named file.
- **Verification:** `a_capped_walk_is_reported_rather_than_silently_under_counting`,
  `an_explicitly_named_path_that_is_a_symlink_is_refused_too`.
- **Committed in:** `3300dee`, `dfd89e3`

**Total deviations:** 2 auto-fixed. **Impact:** no scope creep — one closes a hermeticity
hole the plan's own verification block opens, the other surfaces a threat-model mitigation
that had no output path.

## Issues Encountered

None. `src/sync/` already existed from Phase 1, so the six modules were added to the
existing `mod.rs` rather than creating it.

## Security notes carried forward

- **No new object is sealed under `chunk_key` in this plan.** Nothing here touches
  `chunk::seal_chunk`; the local SQLite index is plaintext-on-disk at mode 0600, not a
  sealed object. **The deferred AAD object-type separator (Phase 1 NEW-3) is therefore
  still not triggered.** The first plan in this phase that seals a new object kind must land
  the separator first.
- **No unbounded id list is read before its container authenticates.** The only new
  bounded-read surface is `scope::walk`, which is capped by `MAX_WALK_ENTRIES` and reports
  `walk_capped`.
- `sync status` prints paths and byte counts only; no file body is ever read for display.

## User Setup Required

None.

## Next Phase Readiness

- **2-02 (collectors)** — fill the `Credentials | Routines | ChatIndex` arm of
  `scope::collect`; `walk` / `push_path` / `is_excluded` are ready and `SyncRoots` already
  carries `desktop_profiles_dir`, `desktop_data_dir` and `claude_home`.
- **2-03 (index)** — add the file table beside `meta` in `src/sync/index.rs`; keep the
  create-0600-before-open ordering.
- **2-04 (transcripts)** — fill `transcripts::collect_bounded` only; `scope::collect`'s
  Transcripts arm and `CategoryScan`'s `excluded_*` fields are already in place, so that
  file is the plan's sole edit.
- **2-07 (dry-run)** — extend `report.rs` with the would-upload column; `render_status` is
  pure and its shape is fixed.
- **Not yet delivered by this plan:** four of the five categories report zero, and the
  CAL-2 / CAL-4 calibrations remain open for later plans.

---
*Phase: 02-bundle-scope-local-index-dry-run-planning*
*Completed: 2026-08-19*
