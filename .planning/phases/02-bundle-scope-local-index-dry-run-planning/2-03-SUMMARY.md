---
phase: 02-bundle-scope-local-index-dry-run-planning
plan: 03
subsystem: infra
tags: [sync, rusqlite, change-detection, cache, hermetic-tests]

requires:
  - phase: 02-bundle-scope-local-index-dry-run-planning
    plan: 01
    provides: "`Index::{at, path, last_sync}`, `index::default_path`, `scope::FileEntry`"
provides:
  - "`Index::lookup(&FileEntry) -> Option<FileRecord>` — D5's `(path, size, mtime_ns, inode)` short-circuit; a hit means the file is never opened"
  - "`Index::record(&FileEntry, sealed_chunks, &[[u8; 32]])` / `Index::touch(&Path)`"
  - "`Index::{generation, bump_generation, evict_unseen}` — borg's cache-age mechanic"
  - "`Index::set_last_sync(DateTime<Utc>)` beside the existing reader"
  - "`Index::was_rebuilt()` — the discard-and-rebuild signal plan 2-07's dry-run reports"
  - "`FileRecord { sealed_chunks: u64, chunk_ids: Vec<[u8; 32]> }`"
  - "`index::SCHEMA_VERSION` and the `file` / `chunk` / `meta` schema"
affects: [2-05-planner, 2-07-dry-run]

tech-stack:
  added: []
  patterns:
    - "Every read path fails towards a miss: no row, an out-of-range field, a malformed blob, or any SQL error all read as `changed`, so the index can be slow but never wrong"
    - "Corrupt state is discarded whole, never repaired — a repaired row could be a stale row"
    - "Mode 0600 set on the rebuild path with the same helper as the fresh path"

key-files:
  created: []
  modified:
    - src/sync/index.rs

key-decisions:
  - "`schema_version` and `generation` live in `meta` as the plan specified, not in SQLite's `user_version` pragma. `user_version` would have been marginally simpler but the plan pins the meta keys and 2-07 may read them."
  - "A version mismatch is a *discard*, not a `check_version`-style ceiling. The index is a cache: there is nothing to migrate and nothing to lose, so both a past and a future version are thrown away. This is deliberately the opposite of the bundle format's read-anything-at-or-below rule."
  - "`discard` removes `-journal`, `-wal` and `-shm` alongside the file. SQLite would replay a stale journal over the fresh database and reinstate the corruption we just deleted."
  - "`open_checked` ends with a prepare-only probe of every `file` column. A hand-made database carrying our table names with different columns survives `CREATE TABLE IF NOT EXISTS`; without the probe it would fail at the first `lookup`, mid-sync, where the answer can no longer be `discard`."
  - "`lookup` bounds `length(chunk_ids)` inside the SQL (`MAX_CHUNK_IDS_BYTES = 32 MiB`). 2-CONTEXT carried forward the rule that an id list read before its container authenticates needs its own bound; the local index is unauthenticated by construction."
  - "`evict_unseen` computes its horizon in signed arithmetic. On a fresh index `generation - keep_generations` is negative, and an unsigned saturating zero would have evicted the rows just written — the test pins this."
  - "`i64` conversion failures (a `size`/`mtime_ns`/`inode`/`sealed_chunks` that does not fit, a non-UTF-8 path) make `record` a silent no-op and `lookup` a miss, rather than an error. The effect is a re-chunk; a failed sync would be the worse answer."

patterns-established:
  - "The hint invariant is a test, not a comment: garbage bytes, a future schema version, a wrong-shaped table, a truncated `chunk_ids` blob and an outright deleted file each have a test asserting the answer is `changed`, not a wrong `unchanged`"
  - "Mode 0600 is asserted on the rebuild path as well as the fresh one — the rebuild path is the one a crash or an attacker actually reaches"

requirements-completed: [SYNC-01, SYNC-02]

coverage:
  - id: D5
    description: "16 tests in `sync::index`, every one over a `TempDir` via `Index::at`; none calls `default_path()` or touches the real cache directory"

completed: 2026-08-19
status: complete
---

# Phase 2 / Plan 03: Local change-detection index Summary

**`src/sync/index.rs` now answers unchanged/changed on D5's `(path, size, mtime_ns, inode)` tuple without opening the file, and a missing, corrupt, future-versioned or wrong-shaped index is discarded and rebuilt so the failure mode is a slow sync rather than a silently omitted file.**

## Performance

- **Duration:** ~20 min
- **Tasks:** 2 of 2
- **Files modified:** 1

## Task Commits

1. **Task 1 + Task 2 (single file, single unit)** — `985b091` (feat)

Both tasks edit the same file and the degradation behaviour of task 2 lives inside `Index::at`, which task 1 also rewrites; splitting them would have produced a commit that does not compile on its own.

## THE `Index` PUBLIC API — what plan 2-05 builds against

```rust
pub const SCHEMA_VERSION: i64 = 1;

/// What a hit gives back: enough to reuse the file's chunks without opening it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRecord {
    pub sealed_chunks: u64,        // count of full CHUNK_SIZE chunks
    pub chunk_ids: Vec<[u8; 32]>,  // in file order
}

pub fn default_path() -> Result<PathBuf>;   // production only; no test calls it

pub struct Index { /* conn, path, rebuilt */ }

impl Index {
    /// TEST SEAM and the production constructor alike. Creates a missing parent
    /// directory, creates the file mode 0600, and discards+rebuilds anything it
    /// cannot read at SCHEMA_VERSION.
    pub fn at(path: &Path) -> Result<Self>;

    pub fn path(&self) -> &Path;
    pub fn was_rebuilt(&self) -> bool;

    /// Some(_) only when all four of D5's fields match. Performs no I/O beyond
    /// the SQLite read — the caller may rely on never opening the file.
    pub fn lookup(&self, entry: &FileEntry) -> Option<FileRecord>;

    pub fn record(
        &self,
        entry: &FileEntry,
        sealed_chunks: u64,
        chunk_ids: &[[u8; 32]],
    ) -> Result<()>;

    /// Stamp the current generation on a row confirmed unchanged.
    pub fn touch(&self, path: &Path) -> Result<()>;

    pub fn generation(&self) -> u64;              // 0 before the first bump
    pub fn bump_generation(&self) -> Result<u64>; // returns the new generation

    /// Deletes file and chunk rows with `seen_gen <= generation - keep_generations`,
    /// returning how many went. On a fresh index (generation 0) this removes nothing.
    pub fn evict_unseen(&self, keep_generations: u64) -> Result<usize>;

    pub fn last_sync(&self) -> Option<DateTime<Utc>>;          // from 2-01
    pub fn set_last_sync(&self, at: DateTime<Utc>) -> Result<()>;
}
```

### Usage shape the planner is expected to follow

```rust
let gen = index.bump_generation()?;          // once per sync run
for entry in &scan.files {
    match index.lookup(entry) {
        Some(rec) => { index.touch(&entry.path)?; /* reuse rec.chunk_ids */ }
        None      => { /* open, chunk, then */ index.record(entry, sealed, &ids)?; }
    }
}
index.evict_unseen(KEEP)?;                   // once, at the end
index.set_last_sync(now)?;
let _ = gen;
```

`record` stamps `seen_gen` from `generation()` itself, so the planner does not pass it.

## Schema

```sql
CREATE TABLE IF NOT EXISTS file (
  path          TEXT PRIMARY KEY,
  size          INTEGER NOT NULL,
  mtime_ns      INTEGER NOT NULL,
  inode         INTEGER NOT NULL,
  sealed_chunks INTEGER NOT NULL,
  chunk_ids     BLOB    NOT NULL,   -- concatenated 32-byte ids, in order
  seen_gen      INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS chunk (
  id        BLOB PRIMARY KEY,
  pack      BLOB    NOT NULL,
  "offset"  INTEGER NOT NULL,       -- quoted: OFFSET is a SQLite keyword
  clen      INTEGER NOT NULL,
  plen      INTEGER NOT NULL,
  seen_gen  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v BLOB);
-- meta keys: schema_version, generation, last_sync
```

The `chunk` table is created and evicted here but has no writer yet — plan 2-05/2-06 owns the pack bookkeeping that fills it.

## The hint invariant, as tests

| Damage | Test | Asserted answer |
|---|---|---|
| index deleted | `deleting_the_index_reports_every_file_as_changed` | every lookup `None`, then `record`/`lookup` round-trips |
| non-SQLite bytes | `garbage_bytes_are_discarded_and_the_index_rebuilds_itself` | `was_rebuilt()`, lookups `None`, mode 0600, round-trips |
| `schema_version` = 2 | `a_future_schema_version_is_discarded_rather_than_read` | `was_rebuilt()`, prior rows unreadable, version reset to 1, mode 0600 |
| `file` table, wrong columns | `our_table_names_with_the_wrong_columns_are_discarded_too` | `was_rebuilt()`, then normal operation |
| `chunk_ids` truncated mid-id | `a_chunk_ids_blob_that_is_not_a_whole_number_of_ids_reads_as_no_match` | `None` — a re-chunk, never a short list |

The three mismatch cases are three separate tests (`lookup_misses_when_size_differs`, `…mtime_ns…`, `…inode…`), as the plan required.

## Deviations from Plan

1. **No explicit transaction in `record`.** The plan asked for one "so a killed process leaves the index either updated or not, never half-updated". `record` is a single `INSERT OR REPLACE`, and SQLite already commits a single statement whole or not at all — the property holds without the ceremony, and `rusqlite`'s `Transaction` wants `&mut Connection` while `record` takes `&self`. The reasoning is recorded as a comment at the statement. If a future caller batches many `record`s into one unit of work, `Connection::unchecked_transaction` is the seam to add there.
2. **A schema-version mismatch is not routed through `sync::check_version`.** That helper implements "read anything at or below the ceiling", which is right for a bundle a *different machine* wrote and wrong for a local cache: there is no old index worth reading. Both directions discard.
3. **Two extra hardening checks the plan did not name**, both feeding the discard path it did: the SQLite `integrity_check` pragma, and the prepare-only column probe that catches our table names carrying foreign columns.

## Issues Encountered

- `FileEntry.mtime_ns` is `i128` (2-01's choice) but SQLite integers are `i64`. Handled by `key_of`, which converts all three numeric fields and returns `None` if any does not fit — a miss, which is the safe direction. Real nanosecond timestamps fit `i64` until 2262, so this never fires in practice.

## Security notes carried forward

- T-2-11 (corrupt index → wrong answer), T-2-12 (file mode), T-2-14 (truncated `chunk_ids`) and T-2-15 (unbounded growth) are each mitigated and each have a named test. T-2-13 (location) is structural: `default_path` resolves under `~/.cache` and no collector category reaches it.
- T-2-SC holds: no dependency added. `rusqlite`/`bundled` was already direct.
- The rebuild `eprintln!` prints the index path and the SQLite reason only. Row contents — full paths carrying account UUIDs — are never logged.

## Verification

- `cargo test --lib sync::index` — 16 passed.
- `cargo test --lib sync::` — 135 passed (no regression in the sibling modules).
- `cargo clippy --all-targets -- -D warnings` — clean.
- `cargo fmt --check` — clean.
- No test calls `default_path()`; every one builds its path from a `TempDir`.

## Next Phase Readiness

Plan 2-05's planner can consume the API above unchanged. The `chunk` table is empty and waiting for whoever owns pack bookkeeping; `evict_unseen` already ages it.
