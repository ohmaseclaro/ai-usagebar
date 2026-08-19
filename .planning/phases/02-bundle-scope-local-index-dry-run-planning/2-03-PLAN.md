---
phase: 2-bundle-scope-local-index-dry-run-planning
plan: 03
type: execute
wave: 2
depends_on: ["2-01"]
files_modified:
  - src/sync/index.rs
autonomous: true
requirements: [SYNC-01, SYNC-02]
user_setup: []

must_haves:
  truths:
    - "A file whose (path, size, mtime_ns, inode) all match the stored row is reported unchanged without its body being read."
    - "Any one of size, mtime_ns or inode differing reports the file as changed."
    - "A missing index file reports every file as changed and rebuilds itself — a full re-scan, never a wrong answer."
    - "A corrupt index file does the same: it is replaced, and the caller still gets a correct changed-everything answer."
    - "The index file is mode 0600 on unix from the moment it exists."
    - "An entry not seen for the eviction horizon is removed, so a deleted file's row does not live forever."
  artifacts:
    - src/sync/index.rs
  key_links:
    - "`Index::at(&Path)` is the only constructor tests use; `default_path()` is the production-only resolver."
    - "The change-detection tuple is exactly D5's `(path, size, mtime_ns, inode)`, consumed by plan 2-05's planner."
---

<objective>
Build the local SQLite index at `~/.cache/ai-usagebar/sync/index.sqlite3` per D5: the file /
chunk / meta schema, the `(path, size, mtime_ns, inode)` change-detection lookup that
short-circuits without re-hashing, `seen_gen` eviction, mode 0600, and the degradation
behaviour that makes a missing or corrupt index a slow sync rather than a wrong one.

Purpose: this is what makes SYNC-02's near-zero no-op real. It is also the only file in this
phase that persists account UUIDs, which is why it lives under `~/.cache` — never itself
synced — at mode 0600.
Output: `src/sync/index.rs` complete, ready for plan 2-05's planner to consume.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/2-bundle-scope-local-index-dry-run-planning/2-CONTEXT.md
@.planning/phases/2-bundle-scope-local-index-dry-run-planning/2-01-SUMMARY.md
@.planning/research/chunking-storage.md
@CLAUDE.md
@src/cursor/db.rs
</context>

<tasks>

<task type="auto" tdd="true">
  <name>Task 1: schema, migration, and the change-detection lookup</name>
  <files>src/sync/index.rs</files>
  <read_first>src/cursor/db.rs lines 47-83 (the rusqlite open pattern and its error taxonomy) and lines 220-232 (`seed_db` — the seeded-temp-db test idiom); .planning/research/chunking-storage.md lines 231-262 (the schema this is based on, and the cache-not-source-of-truth rule).</read_first>
  <behavior>
    - Opening a path that does not exist creates the schema and reports schema version 1.
    - Opening an existing index of the current version leaves it untouched and reads back a row written by a previous open.
    - `lookup` on a path with no row returns none.
    - `lookup` on a matching (size, mtime_ns, inode) returns the stored chunk ids and sealed-chunk count.
    - `lookup` returns none when size differs, when mtime_ns differs, and when inode differs — three separate cases.
    - `record` then `lookup` round-trips a chunk-id list of length 0, 1 and many, preserving order.
    - Bumping the generation and evicting past the horizon removes an untouched row and keeps a touched one.
  </behavior>
  <action>
Extend the skeleton from plan 2-01 to the full D5 schema. Keep `Index::at(&Path)` as the
sole test seam and `default_path()` as the production-only resolver — no test calls the
latter, matching `Cache::at` / `creds::read_from`.

Tables, per the research schema, with D5's key set:

`file(path TEXT PRIMARY KEY, size INTEGER NOT NULL, mtime_ns INTEGER NOT NULL,
inode INTEGER NOT NULL, sealed_chunks INTEGER NOT NULL, chunk_ids BLOB NOT NULL,
seen_gen INTEGER NOT NULL)` — `chunk_ids` is the concatenation of 32-byte ids in file order,
so a length that is not a multiple of 32 is a corrupt row and reads as no match rather than
as a truncated list.

`chunk(id BLOB PRIMARY KEY, pack BLOB NOT NULL, offset INTEGER NOT NULL, clen INTEGER NOT NULL,
plen INTEGER NOT NULL, seen_gen INTEGER NOT NULL)`.

`meta(k TEXT PRIMARY KEY, v BLOB)` — already present from plan 2-01; add the `schema_version`
and `generation` keys alongside the existing `last_sync`.

D5 names the change-detection tuple as `(path, size, mtime_ns, inode)`. The research argued
for borg's `ctime` as a fourth field because ctime cannot be forged from user space; D5
locks the tuple without it. Implement D5. Note the divergence in one comment so a future
reader knows it was a decision, not an omission.

API:
- `pub fn lookup(&self, entry: &FileEntry) -> Option<FileRecord>` — returns Some only when
  all four of D5's fields match. This is the short-circuit: a match means the caller never
  opens the file. Callers must be able to rely on that, so `lookup` itself performs no I/O
  beyond the SQLite read.
- `pub fn record(&self, entry: &FileEntry, sealed_chunks: u64, chunk_ids: &[[u8; 32]]) -> Result<()>`
- `pub fn touch(&self, path: &Path) -> Result<()>` stamping `seen_gen` on a row confirmed
  unchanged.
- `pub fn generation(&self) -> u64` / `pub fn bump_generation(&self) -> Result<u64>`.
- `pub fn evict_unseen(&self, keep_generations: u64) -> Result<usize>` deleting file and
  chunk rows whose `seen_gen` is older than the horizon, returning the count removed.
- `pub fn set_last_sync(&self, at: DateTime<Utc>) -> Result<()>` beside the existing reader.

Use a transaction for `record` batches so a killed process leaves the index either updated
or not, never half-updated — and because the index is a hint, either outcome is correct.
Add no crate: `rusqlite` with `bundled` is already a direct dependency.
  </action>
  <verify>
    <automated>cargo test --lib sync::index</automated>
  </verify>
  <done>Every bullet in `&lt;behavior&gt;` has a passing test using a `TempDir` path and `Index::at`. The three mismatch cases (size, mtime_ns, inode) are three separate assertions, not one combined case.</done>
</task>

<task type="auto" tdd="true">
  <name>Task 2: the index is a hint — missing and corrupt both degrade to a full re-scan</name>
  <files>src/sync/index.rs</files>
  <behavior>
    - `Index::at` on a path holding arbitrary non-SQLite bytes succeeds, and every subsequent `lookup` returns none.
    - `Index::at` on a valid SQLite file with an unknown future `schema_version` also succeeds and returns none from every lookup, rather than reading rows it cannot interpret.
    - `Index::at` on a path whose parent directory does not exist creates the directory and the file.
    - After a corrupt index is replaced, `record` and `lookup` work normally.
    - The replacement file is mode 0600 on unix, same as a fresh one.
  </behavior>
  <action>
This is the invariant the whole design rests on: the index is a cache, exactly like the
project's vendor caches, so it is always safe to delete and a damaged one must never produce
a wrong answer. Wrong here means reporting a changed file as unchanged, which would silently
omit it from the sync.

In `Index::at`, wrap the open-and-migrate path: if opening fails, or the integrity check
fails, or `schema_version` reads as anything other than the current version, discard the file
and create a fresh empty index at the same path. Discard means remove and recreate — do not
attempt repair, and do not fall back to an in-memory database, because a caller that then
records into memory would lose the index every run without saying so.

Set the 0600 mode on the recreated file exactly as on a fresh one; the recreate path is the
one an attacker or a crash actually reaches, so it must not be the path that leaves the file
world-readable.

Report the discard through a field on `Index` — `pub fn was_rebuilt(&self) -> bool` — so
plan 2-07's dry-run can tell the user why this run is slow instead of leaving them guessing.
Log the path only; the index's contents are account UUIDs.
  </action>
  <verify>
    <automated>cargo test --lib sync::index</automated>
  </verify>
  <done>A test writes garbage bytes to the index path, opens it, and asserts `was_rebuilt()` is true, every `lookup` returns none, and a subsequent `record`/`lookup` round-trips. A second test does the same for a future `schema_version`. On unix both assert mode 0600.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| local index file → planner | A file any local process could have modified determines which files this tool decides not to read. |
| collector → index file | Account UUIDs embedded in paths cross into a persisted file. |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|-----------|----------|-----------|----------|-------------|-----------------|
| T-2-11 | Tampering | `Index::at` | critical | mitigate | A corrupt or future-versioned index is discarded and rebuilt, so a damaged hint yields a full re-scan. The failure mode is a slow sync, never an omitted file. Asserted by tests for both the garbage-bytes and future-version cases. |
| T-2-12 | Information disclosure | index file mode | high | mitigate | Mode 0600 set at creation and on the rebuild path alike; the index holds full paths containing account UUIDs. |
| T-2-13 | Information disclosure | index location | high | mitigate | The index lives under `~/.cache`, not `~/.config`, precisely so it is outside every collected category and can never be synced to another machine. |
| T-2-14 | Tampering | `chunk_ids` blob | medium | mitigate | A `chunk_ids` blob whose length is not a multiple of 32 is treated as no match, so a truncated row forces a re-chunk rather than yielding a short chunk list. |
| T-2-15 | Denial of service | unbounded index growth | low | mitigate | `seen_gen` eviction removes rows for files that no longer exist, following borg's cache-age mechanic. |
| T-2-SC | Tampering | npm/pip/cargo installs | high | accept | This plan adds no dependency — `rusqlite` with `bundled` is already present for Cursor's `state.vscdb`. `cargo machete` runs in the phase-end gate. |
</threat_model>

<verification>
`cargo test --lib sync::index` is green. Every test constructs its index with `Index::at`
over a `TempDir`; none calls `default_path()` or touches the real cache directory.
</verification>

<success_criteria>
The index answers unchanged/changed on D5's four-field tuple without reading a file body, and
a missing or corrupt index degrades to a full re-scan with the failure surfaced through
`was_rebuilt()`.
</success_criteria>

<output>
Create `.planning/phases/2-bundle-scope-local-index-dry-run-planning/2-03-SUMMARY.md` when done.
Record the exact `Index` public API, since plan 2-05 builds directly against it.
</output>
