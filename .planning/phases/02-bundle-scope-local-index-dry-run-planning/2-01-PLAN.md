---
phase: 2-bundle-scope-local-index-dry-run-planning
plan: 01
type: execute
wave: 1
depends_on: []
files_modified:
  - src/lib.rs
  - src/config.rs
  - config.example.toml
  - src/sync/mod.rs
  - src/sync/scope.rs
  - src/sync/transcripts.rs
  - src/sync/index.rs
  - src/sync/plan.rs
  - src/sync/report.rs
  - src/sync/cli.rs
  - src/widget/cli.rs
  - src/bin/ai-usagebar.rs
autonomous: true
requirements: [SCOPE-01, SCOPE-02, SCOPE-05, UX-02]
user_setup: []

must_haves:
  truths:
    - "`ai-usagebar sync status` runs against injected temp roots and prints one line per category with file count and raw bytes."
    - "Transcripts appears in that listing as off, and last-sync reads never."
    - "A symlink pointing outside a scanned root contributes zero files to any category."
    - "None of the D2 hard-exclusion names appears in a scan result."
    - "An existing config.toml with no `[sync]` section still loads and yields the D6 default category set."
  artifacts:
    - src/sync/mod.rs
    - src/sync/scope.rs
    - src/sync/index.rs
    - src/sync/report.rs
    - src/sync/cli.rs
  key_links:
    - "`SyncRoots::at` is the only way tests reach a filesystem root; `SyncRoots::resolve` is the thin production wrapper."
    - "`scope::walk` is the single walker every category funnels through, so `is_excluded` cannot be bypassed by a later category."
    - "`Config.sync` is a plain field on `Config`, so `[sync]` in a real config.toml is not rejected by the section-level `deny_unknown_fields`."
---

<objective>
Stand up the whole Phase 2 spine as one thin working vertical slice: the `[sync]` config
section, the injected-roots seam, the single bounded symlink-safe walker with the D2
exclusion predicate, a skeleton local index, and `ai-usagebar sync status` — wired end to
end and proven on a seeded temp tree, for the `config` category only.

Purpose: every later plan in this phase adds one horizontal layer to a proven path. The
walker, the exclusion predicate and the roots seam are the load-bearing pieces; getting them
wrong quietly ships a credential to the wrong machine, so they are proven first on the
agent's best context.
Output: `src/sync/` exists with all six module files declared, `sync status` runs, and every
later plan owns exactly one of those files.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/2-bundle-scope-local-index-dry-run-planning/2-CONTEXT.md
@CLAUDE.md
@src/context/mod.rs
@src/claude_desktop/mod.rs
@src/cache.rs
</context>

<tasks>

<task type="tracer">
  <name>Task 1: `[sync]` config section, the `SyncRoots` seam, and the module skeleton</name>
  <files>src/config.rs, src/lib.rs, config.example.toml, src/sync/mod.rs, src/sync/scope.rs, src/sync/transcripts.rs, src/sync/index.rs, src/sync/plan.rs, src/sync/report.rs, src/sync/cli.rs</files>
  <read_first>src/config.rs lines 36-57 (the `Config` struct and its section-level `deny_unknown_fields` comment), lines 819-862 (`load_from` / `expand_paths`), lines 1009-1044 (`default_path` / `resolved_path`); src/claude_desktop/mod.rs lines 65-141 (`Paths::at` / `Paths::resolve` — the seam idiom to copy).</read_first>
  <action>
Add `pub sync: SyncConfig` to `Config` in src/config.rs. `Config` carries
`deny_unknown_fields` at the section level, so without this field a user's `[sync]` section
is a hard parse failure — the field must land in the same commit as the documented section.

`SyncConfig` is `#[derive(Debug, Clone, Deserialize, Serialize)]` with `#[serde(default)]`
and an explicit `Default` impl (not `derive(Default)`, because the defaults are non-empty).
Per D6 and D3: `categories: Vec<SyncCategory>` defaulting to Config, Credentials, Routines,
ChatIndex — transcripts deliberately absent, which is SCOPE-02; `transcript_days: u32`
defaulting to 30; `transcript_max_bytes: u64` defaulting to `2 * 1024 * 1024 * 1024`. Do not
apply `deny_unknown_fields` inside the section, matching the existing rule that unknown keys
are denied per-section only. Add a helper `SyncConfig::includes(&self, cat: SyncCategory) -> bool`.

`SyncCategory` is a `#[serde(rename_all = "snake_case")]` enum with the five D1 variants
Config, Credentials, Routines, ChatIndex, Transcripts, plus `ALL: [SyncCategory; 5]` in
canonical D1 order and a `label()` returning the snake_case string for display. Derive
Copy, Clone, Debug, PartialEq, Eq, Deserialize, Serialize.

Add no new path knobs to config: `SyncRoots::resolve` derives every root from what already
exists, so `expand_paths()` needs no change.

Create `src/sync/mod.rs` declaring `pub mod cli; pub mod index; pub mod plan; pub mod report;
pub mod scope; pub mod transcripts;` and register `pub mod sync;` in src/lib.rs in
alphabetical position. Create the five sibling files now with only their module doc comment
so the crate compiles; Task 2 and Task 3 fill scope/index/report/cli, and later plans own
transcripts.rs and plan.rs.

`src/sync/mod.rs` also holds the roots seam, modelled directly on
`claude_desktop::Paths::at` / `::resolve`:

`pub struct SyncRoots { pub config_file: PathBuf, pub config_dir: PathBuf,
pub desktop_data_dir: PathBuf, pub desktop_profiles_dir: PathBuf, pub claude_home: PathBuf }`
— `config_file` is the effective config.toml, `config_dir` its parent (where `accounts/*/`
lives), `desktop_data_dir` is Claude Desktop's data dir (parent of `claude-code-sessions`),
`desktop_profiles_dir` is `~/.claude-acc/profiles`, `claude_home` is `~/.claude` (parent of
`scheduled-tasks/` and `projects/`).

`SyncRoots::at(...)` takes all five explicitly and is what every test uses.
`SyncRoots::resolve(&Config) -> Result<SyncRoots>` is the thin production wrapper: it reads
`config::resolved_path()`, `claude_desktop::Paths::resolve(&config.anthropic)` for
`data_dir`/`profiles_dir`, and `cache::home_dir()?.join(".claude")`. No test calls `resolve`.

Add the `[sync]` section to config.example.toml with the three keys at their defaults and a
comment saying transcripts is off by default and that this section is itself synced, so a
second machine inherits the selection.
  </action>
  <verify>
    <automated>cargo test --lib sync</automated>
  </verify>
  <done>`Config::load_from` on a fixture containing `[sync]` round-trips the three keys; on a fixture with no `[sync]` section it yields the four D6 default categories and 30 / 2 GiB. `SyncRoots::at` constructs from five TempDir paths with no call into `home_dir`.</done>
  <reversibility rating="costly">The `[sync]` TOML key names are a user-facing surface and `Config` denies unknown sections, so renaming a key after ship breaks existing config files. Names come straight from D6 and are not to be improvised.</reversibility>
</task>

<task type="auto" tdd="true">
  <name>Task 2: the bounded symlink-safe walker, the D2 exclusion predicate, and the `config` category</name>
  <files>src/sync/scope.rs, src/sync/index.rs</files>
  <read_first>src/context/mod.rs lines 142-208 (`discover` — the bounded walker with the symlink guard to mirror) and lines 590-608 (`discovered_symlinks_are_not_followed`, the test shape to mirror); src/cursor/db.rs lines 47-83 and 220-232 (rusqlite open + the seeded-temp-db test idiom).</read_first>
  <behavior>
    - A file directly under a scanned root is collected with its size, mtime_ns and inode.
    - A symlink whose target sits outside the scanned root contributes nothing.
    - A directory symlink is not descended into.
    - Each D2 name is rejected: `bridge-state.json`, `ant-device-registry.json`, `.stale`, `.last_error`, `.fetch.lock`.
    - Each D2 directory component is rejected: `backups`, `prelogin-backup`, `hidden`, `local-agent-mode-sessions`.
    - Each D2 suffix/prefix is rejected: `*.lock`, `*.tmp`, `*-journal`, and the `.tmp.` prefix `cache::atomic_write` uses for its in-flight tempfiles.
    - Walking a tree wider than the entry cap stops and reports that it was capped rather than running unbounded.
    - `collect(Category::Config, ...)` on a seeded root returns config.toml plus any `accounts/*/.credentials.json`, and nothing else.
    - A category absent from `SyncConfig::categories` returns an empty scan without touching the filesystem.
  </behavior>
  <action>
In src/sync/scope.rs define:

`pub struct FileEntry { pub path: PathBuf, pub size: u64, pub mtime_ns: i128, pub inode: u64 }`
and `pub struct CategoryScan { pub category: SyncCategory, pub files: Vec<FileEntry>,
pub bytes: u64, pub excluded_files: usize, pub excluded_bytes: u64, pub walk_capped: bool,
pub skipped: usize }`. `CategoryScan::empty(cat)` for a disabled or absent category.

The two excluded-* fields stay zero for every category except transcripts, whose D3 bounds
leave a remainder the user needs told about. They are declared here, not by plan 2-04, so
that plan owns exactly one file and never contends with plan 2-02 over this one.

Stat extraction goes through one private helper so the platform split lives in a single
place: on unix use `std::os::unix::fs::MetadataExt` for `mtime_nanos`-precision and `ino()`;
on other platforms report inode 0 and derive mtime_ns from `SystemTime`. D5 keys change
detection on `(path, size, mtime_ns, inode)`, so this helper is the producer of three
quarters of that tuple.

`pub fn is_excluded(path: &Path) -> bool` implements D2 in full against three private
consts — an excluded-basename set, an excluded-path-component set, and the suffix/prefix
rules listed in the behavior block above. These are not a size optimisation: each entry is
wrong to carry to another machine, so the predicate lives in the walker and every category
inherits it rather than each collector re-deciding.

`fn walk(root: &Path, out: &mut CategoryScan)` is the single walker. Port the structure of
`context::discover`: an explicit stack, `fs::read_dir` failures isolated to the affected
entry, a `MAX_WALK_ENTRIES` cap that sets `walk_capped`, and `file_type.is_symlink()`
returning early — never followed, for files or directories alike. Apply `is_excluded` to
every entry before it is pushed or collected. A missing root is not an error: an account
that has never run the Desktop app simply has no credentials tree, so a missing root yields
an empty scan.

`pub fn collect(cat: SyncCategory, roots: &SyncRoots, cfg: &SyncConfig, now: DateTime<Utc>) -> CategoryScan`
returns `CategoryScan::empty` when `!cfg.includes(cat)`, and otherwise matches on the
category. Wire all five arms now so no later plan edits this match: `Config` is implemented
here (walk `roots.config_dir.join("accounts")` and add `roots.config_file` directly, per
D1); `Transcripts` delegates to `crate::sync::transcripts::collect_bounded(roots, cfg, now)`;
`Credentials`, `Routines` and `ChatIndex` return `CategoryScan::empty` with a one-line
comment naming plan 2-02 as their owner. `transcripts::collect_bounded(roots, cfg, now)` is
added in transcripts.rs with exactly that signature, returning an empty scan for now; plan
2-04 owns its body.

`collect` takes `now` because the transcripts arm's D3 bounds are time-dependent, and the
project's rule is that time-dependent logic takes `now: DateTime<Utc>` as an argument so no
test reads the wall clock. The other four arms ignore it. Declaring it here means plan 2-04
never has to edit this file.

In src/sync/index.rs add only what `sync status` needs, leaving the schema to plan 2-03:
`pub fn default_path() -> Result<PathBuf>` resolving `~/.cache/ai-usagebar/sync/index.sqlite3`
via the same `directories`-based cache resolver `cache.rs` already uses, and
`pub struct Index` with `pub fn at(path: &Path) -> Result<Index>` which creates the parent
directory, opens the rusqlite connection, creates a `meta(k TEXT PRIMARY KEY, v BLOB)` table
if absent, and on unix sets the file to mode 0600 immediately after creation — the index
holds account UUIDs in its paths, so the mode is set before anything is written, not after.
Add `pub fn last_sync(&self) -> Option<DateTime<Utc>>` reading the `last_sync` meta key.
Reuse the existing `rusqlite` dependency; add no crate.
  </action>
  <verify>
    <automated>cargo test --lib sync::scope sync::index</automated>
  </verify>
  <done>Every bullet in `<behavior>` has a passing test that injects its root from a `TempDir`. `Index::at` on a fresh TempDir path creates the file and, on unix, `metadata().mode() &amp; 0o777 == 0o600`.</done>
</task>

<task type="auto" tdd="true">
  <name>Task 3: `ai-usagebar sync status`, end to end</name>
  <files>src/sync/report.rs, src/sync/cli.rs, src/widget/cli.rs, src/bin/ai-usagebar.rs</files>
  <read_first>src/widget/cli.rs lines 134-168 (the `Command` enum and the `Settings` subcommand shape to copy); src/bin/ai-usagebar.rs lines 8-16 (how a non-widget subcommand is dispatched to an `i32`-returning `run` before the tokio runtime is built).</read_first>
  <behavior>
    - A status report built from a seeded temp tree renders one line per category, in D1 order, each with a file count and a raw byte size.
    - The transcripts line renders as off when the category is absent from the configured set.
    - With no `last_sync` meta key the report renders last-sync as never.
    - Rendering is a pure function of the report struct — no filesystem access, so it is snapshot-testable.
  </behavior>
  <action>
src/sync/report.rs holds the pure model and renderer, following the project's existing split
where `report.rs` builds a model that frontends render:
`pub struct CategoryLine { pub category: SyncCategory, pub enabled: bool, pub files: usize,
pub bytes: u64 }`, `pub struct StatusReport { pub lines: Vec<CategoryLine>,
pub last_sync: Option<DateTime<Utc>>, pub index_path: PathBuf }`,
`pub fn build_status(roots: &SyncRoots, cfg: &SyncConfig, index: Option<&Index>, now: DateTime<Utc>) -> StatusReport`
and `pub fn render_status(&StatusReport) -> String`. `now` is threaded straight through to
`scope::collect`. Add a private `human_bytes(u64) -> String` here unless `crate::format`
already exposes one — check before writing it.

A disabled category renders its size column as off rather than as a zero, because zero and
not-selected are different facts and the user is choosing between them. Plan 2-07 adds the
would-upload column to this same renderer; leave room for it but do not invent it here.

src/sync/cli.rs holds `pub fn run(action: &SyncAction) -> i32`, matching the signature shape
of `account::run` and `tui::settings::run_cli`. For `SyncAction::Status` it loads `Config`,
resolves `SyncRoots`, opens the index at `index::default_path()` — a failure to open is
reported and the status still renders with last-sync unknown, since the index is a hint —
prints `render_status`, and returns 0. Any error resolving roots prints an actionable message
naming the path and returns non-zero. Never print a file's contents; paths and byte counts
only.

In src/widget/cli.rs add `Sync { #[command(subcommand)] action: SyncAction }` to `Command`
with a doc comment, and `pub enum SyncAction { Status }` carrying its own doc comment. In
src/bin/ai-usagebar.rs dispatch it alongside `Account` and `Settings`, before the tokio
runtime is constructed — this phase makes no network call and needs no runtime.
  </action>
  <verify>
    <automated>cargo test --lib sync</automated>
  </verify>
  <done>A test seeds a TempDir with a config.toml and one `accounts/work/.credentials.json`, builds `SyncRoots::at` over it, and asserts the rendered status contains all five D1 category labels, the config line's file count of 2, an off marker on the transcripts line, and a never marker for last-sync. `cargo test --lib sync` is green and no test in the module constructs a real home path.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| user filesystem → collector | Arbitrary files, symlinks and names authored outside this tool cross into a set destined for another machine. |
| collector → local index | Account UUIDs and full paths cross into a persisted SQLite file. |
| collector → stdout | File names cross into a terminal that may be logged or shared. |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|-----------|----------|-----------|----------|-------------|-----------------|
| T-2-01 | Information disclosure | `scope::walk` | critical | mitigate | `file_type.is_symlink()` returns early for files and directories alike, so a link planted in a scanned tree cannot pull arbitrary host files into the bundle set. Asserted by a test that links to a file in a second TempDir. |
| T-2-02 | Information disclosure | `scope::is_excluded` | high | mitigate | D2's hard exclusions live in the one shared walker, not per collector, so a category added later cannot forget them. One test per exclusion class. |
| T-2-03 | Information disclosure | `index::Index::at` | high | mitigate | Mode 0600 is set at creation, before any write — the index's paths carry account UUIDs. |
| T-2-04 | Information disclosure | `sync::cli::run` output | medium | mitigate | Status output carries paths and byte counts only; no file body is read for display. |
| T-2-05 | Denial of service | `scope::walk` | medium | mitigate | `MAX_WALK_ENTRIES` caps traversal and surfaces `walk_capped` rather than silently truncating, mirroring `context::discover`. |
| T-2-06 | Tampering | `Config` parse | medium | mitigate | `sync` is a real field on `Config`, so a typo'd sibling section is still rejected by the existing section-level `deny_unknown_fields` instead of silently defaulting. |
| T-2-SC | Tampering | npm/pip/cargo installs | high | accept | This plan adds no dependency — `rusqlite` is already vendored and bundled. `cargo machete` runs in the phase-end gate. |
</threat_model>

<verification>
`cargo test --lib sync` is green. Every test in `src/sync/` obtains its roots from
`SyncRoots::at` over a `TempDir`; none calls `SyncRoots::resolve`, `home_dir`, or
`index::default_path`.
</verification>

<success_criteria>
`ai-usagebar sync status` runs and lists all five categories with counts and bytes,
transcripts off, last-sync never. `src/sync/` contains six declared module files, four of
them thin, each owned by exactly one later plan in this phase.
</success_criteria>

<output>
Create `.planning/phases/2-bundle-scope-local-index-dry-run-planning/2-01-SUMMARY.md` when done.
Record in it the exact public signatures of `SyncRoots`, `FileEntry`, `CategoryScan`,
`Index::at` and `collect`, since three parallel plans build against them.
</output>
