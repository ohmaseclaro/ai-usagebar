---
phase: 02-bundle-scope-local-index-dry-run-planning
plan: 05
subsystem: infra
tags: [sync, change-detection, chunking, append-fast-path, dry-run, hermetic-tests]

requires:
  - phase: 02-bundle-scope-local-index-dry-run-planning
    plan: 01
    provides: "`SyncRoots::at`, `scope::collect`, `scope::FileEntry`, `CategoryScan`"
  - phase: 02-bundle-scope-local-index-dry-run-planning
    plan: 03
    provides: "`Index::{lookup, record, touch, generation, bump_generation, was_rebuilt}`, `FileRecord`"
  - phase: 02-bundle-scope-local-index-dry-run-planning
    plan: 04
    provides: "`transcripts::collect_bounded` filling the Transcripts arm of `scope::collect`"
  - phase: 01-encrypted-bundle-core
    provides: "`sync::CHUNK_SIZE` and `chunk::sealed_chunk_count` — the chunking contract this builds against"
provides:
  - "`plan::build(roots, cfg, index, now, chunk_id) -> Result<SyncPlan>` — the whole of SYNC-01/02/03"
  - "`SyncPlan` — the object 2-07 renders and Phase 4 uploads"
  - "`FilePlan` / `CategoryPlan`"
  - "`plan::CHUNK_BYTES` — a re-export of Phase 1's `CHUNK_SIZE`, not a second literal"
  - "`Index::cached(&Path) -> Option<FileRecord>` — the stale-row read the append check verifies against"
affects: [2-07-dry-run, phase-4-upload]

tech-stack:
  added: []
  patterns:
    - "The chunk-id function is a generic closure parameter, so the planner tests against Phase 1's contract with a toy hasher and 2-07 supplies the real keyed BLAKE3 at one call site"
    - "Appendness is verified by re-hashing the last sealed chunk, never inferred from a size increase"
    - "`files_opened` is incremented at the module's single `File::open`, so the no-op claim is a counter and cannot drift from reality"

key-files:
  created: []
  modified:
    - src/sync/plan.rs
    - src/sync/index.rs

key-decisions:
  - "`CHUNK_BYTES` is `CHUNK_SIZE as u64`, not the plan's duplicated literal. The plan allowed one wave of deliberate duplication because Phase 1 might not have merged; it has, in this worktree, so the drift risk was removed now instead of deferred to 2-07."
  - "`build` bumps the generation *before* the touch/record loop, not after. `record` and `touch` both stamp `generation()` themselves, so bumping afterwards would stamp every row with the previous run's number and make `evict_unseen` age rows a generation early. This matches 2-03's own documented usage shape."
  - "Each category runs two passes: every `index.lookup` first, then the changed files. A changed file that shares a chunk with an unchanged one is therefore never counted as new because of scan order. The passes do not span categories — a chunk shared between `config` and `transcripts` is a fiction."
  - "A chunk is *new* iff it was computed from bytes this run and not already seen. Ids that came out of the index — a lookup hit, or a verified append prefix — are known by construction. A file that failed its append check contributes none of its cached ids to the known set, so its re-chunk plans the same list a fresh run would; that is what makes the truncation test's from-scratch comparison meaningful."
  - "`build` does not call `evict_unseen` or `set_last_sync`. Nothing has been uploaded when a plan is built, and a dry-run that stamped `last_sync` would lie. Both belong to whoever actually pushes."
  - "`sealed_chunks` is derived from the bytes actually read, not from `entry.size`. A file that grew between the scan and the read still records an internally consistent row (T-2-24), and next run's D5 mismatch re-plans it."

patterns-established:
  - "The measurement, not the inference: a private `Counters` tracks bytes actually read from disk, so \"the append re-reads one verification chunk plus the new region\" is asserted as an exact byte count rather than deduced from the plan's contents"
  - "Fixtures sized for the AUR `check()`: 5 MiB exercises the same arithmetic as 50 MB, and every installer pays the difference"

requirements-completed: [SYNC-01, SYNC-02, SYNC-03]

coverage:
  - id: SYNC-02
    description: "A second plan over an untouched tree opens exactly 0 files and lists 0 new chunks"
    verification:
      - kind: unit
        ref: "src/sync/plan.rs#a_second_plan_over_an_untouched_tree_opens_nothing_and_uploads_nothing"
        status: pass
    human_judgment: false
  - id: SYNC-03
    description: "A 200 KiB append to a 5 MiB fixture plans 2 new chunks / 307,200 bytes and reads 569,344 of 5,447,680 bytes"
    verification:
      - kind: unit
        ref: "src/sync/plan.rs#appending_plans_one_new_sealed_chunk_and_one_new_tail"
        status: pass
      - kind: unit
        ref: "src/sync/plan.rs#the_append_reads_one_verification_chunk_plus_the_new_region"
        status: pass
    human_judgment: false
  - id: T-2-21
    description: "Truncation, an in-place overwrite at unchanged length, and a rewrite that grew each fall back to a full re-chunk that equals a from-scratch plan"
    verification:
      - kind: unit
        ref: "src/sync/plan.rs#truncating_falls_back_to_a_full_rechunk"
        status: pass
      - kind: unit
        ref: "src/sync/plan.rs#an_in_place_overwrite_fails_the_append_check_and_is_rechunked"
        status: pass
      - kind: unit
        ref: "src/sync/plan.rs#a_file_that_grew_but_whose_last_sealed_chunk_changed_is_fully_rechunked"
        status: pass
    human_judgment: false

duration: 40min
completed: 2026-08-19
status: complete
---

# Phase 2 / Plan 05: Change detection and the plan builder Summary

**`plan::build` produces the object a push uploads: unchanged files are never
opened, an append re-reads one 256 KiB verification chunk instead of the file,
and every way that verification can fail — truncation, an in-place overwrite, a
rewrite that happened to grow — falls back to a full re-chunk that equals what a
fresh run would have planned.**

## Task Commits

1. **`Index::cached`** — `471a43f` (feat)
2. **Tasks 1 + 2: change detection, the append fast path, the plan builder** — `8fbd12b` (feat)

Both plan tasks edit one file and task 2's fast path lives inside the function
task 1 defines; splitting them would have produced a commit that does not
compile. The index accessor is a separate file and is committed on its own.

## THE PUBLIC SURFACE — what 2-07 renders and Phase 4 uploads

```rust
/// Phase 1's fixed chunk size, as the u64 the offset arithmetic wants.
/// A re-export of `sync::CHUNK_SIZE`, not a second literal.
pub const CHUNK_BYTES: u64 = CHUNK_SIZE as u64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePlan {
    pub path: PathBuf,
    pub chunk_ids: Vec<[u8; 32]>,      // every chunk, in order, reused and fresh alike
    pub sealed_chunks: u64,            // full CHUNK_BYTES chunks; the remainder is the tail
    pub new_chunk_ids: Vec<[u8; 32]>,  // the subset this run is the first to see
    pub new_bytes: u64,                // plaintext bytes of new_chunk_ids
    pub reused: bool,                  // true iff the file was never opened
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryPlan {
    pub category: SyncCategory,
    pub files: usize,
    pub raw_bytes: u64,
    pub new_bytes: u64,
    pub excluded_files: usize,   // bound-dropped; transcripts only
    pub excluded_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncPlan {
    pub categories: Vec<CategoryPlan>,       // all five, in SyncCategory::ALL order
    pub new_chunk_ids: Vec<[u8; 32]>,        // deduplicated across files
    pub total_raw_bytes: u64,
    pub total_new_bytes: u64,
    pub files_opened: usize,                 // 0 on a true no-op — SYNC-02's evidence
    pub append_check_miss_bytes: u64,        // what failed append checks cost
    pub index_rebuilt: bool,                 // from Index::was_rebuilt()
    pub file_plans: Vec<FilePlan>,           // per-file detail, in scan order
}

impl SyncPlan {
    /// Nothing to upload.
    pub fn is_empty(&self) -> bool;          // new_chunk_ids.is_empty()
}

pub fn build<F: Fn(&[u8]) -> [u8; 32]>(
    roots: &SyncRoots,
    cfg: &SyncConfig,
    index: &Index,
    now: DateTime<Utc>,
    chunk_id: F,
) -> Result<SyncPlan>;
```

**`build` mutates the index** — it bumps the generation once, touches every
unchanged row and re-records every changed one. It writes nothing else anywhere
and makes no network call. It does **not** call `evict_unseen` or
`set_last_sync`; a plan is not a push.

**2-07's one call site** supplies Phase 1's real chunk-id function:

```rust
let plan = plan::build(&roots, &cfg.sync, &index, Utc::now(), |bytes| {
    *blake3::keyed_hash(name_key, bytes).as_bytes()
})?;
```

The `chunk_id` closure hashes **plaintext**, never a compressed or encrypted
frame. This module never compresses and never encrypts.

## How the three requirements are actually produced

| Requirement | The branch that produces it | The test that proves it |
|---|---|---|
| SYNC-01 — only changed data uploads | `index.lookup` → `Some` ends the file's work | `a_rewritten_file_is_rechunked`, `a_file_rewritten_in_place_at_the_same_size_is_caught_by_mtime` |
| SYNC-02 — a no-op uploads nothing | the same branch; `files_opened == 0` | `a_second_plan_over_an_untouched_tree_opens_nothing_and_uploads_nothing` |
| SYNC-03 — an append uploads the appended bytes | `verified_prefix` returning the cached prefix | `appending_plans_one_new_sealed_chunk_and_one_new_tail` |

## The append check, and why it is a check

The claim an append fast path rests on is that appending displaces no byte's
offset below the old length, so every fully contained 256 KiB chunk keeps its
hash. That is true of appends — and it is **not** a fact the filesystem tells
you. A rewrite that happens to grow the file is indistinguishable from an append
by `(size, mtime_ns, inode)` alone.

So `verified_prefix` re-reads chunk `sealed_chunks - 1` and re-hashes it before
reusing anything. Measured on the 5 MiB fixture (5,345,280 bytes, 20 sealed
chunks plus a 100 KiB tail, +200 KiB appended):

| | Bytes |
|---|---|
| verification read | 262,144 |
| new region (old sealed boundary → EOF) | 307,200 |
| **total read** | **569,344** of 5,447,680 |
| new chunks | 2 (one newly sealed + one tail) |
| **new bytes planned** | **307,200** |

Every failure direction is tested and every one falls back to a full re-chunk
whose id list equals a from-scratch plan's:

| Damage | `append_check_miss_bytes` | Bytes read |
|---|---|---|
| genuine append | 0 | 262,144 + new region |
| truncation | 0 — a shrink skips the probe, there is nothing to verify against | the truncated file |
| in-place overwrite, unchanged length | 262,144 | probe + whole file |
| rewrite that grew | 262,144 | probe + whole file |
| cached `sealed_chunks == 0` | 0 — no probe | the file |
| row claiming more sealed chunks than it stores ids for | 0 — no probe | the file |

`append_check_miss_bytes` is the field that says, in the field, whether the
no-CDC assumption held. If it is ever non-trivial the chunker changes — which is
why Phase 1 records `"chunker": "fixed-256k"` in the snapshot.

## Deviations from Plan

**1. [Correctness, blocking] `Index::cached` added to `src/sync/index.rs`**
- **Found during:** Task 2.
- **Issue:** The plan lists `src/sync/plan.rs` as this plan's only file, but the
  append fast path needs the *stale* row — `sealed_chunks` and `chunk_ids` for a
  file whose D5 tuple has changed. `Index::lookup` returns `Some` only on a full
  four-field match, and a grown file is by definition a miss. Phase 2 shipped no
  path-only read, so the fast path was unimplementable as specified.
- **Fix:** `Index::cached(&Path) -> Option<FileRecord>`, sharing a new private
  `read_row` with `lookup` so the 32 MiB bound, the whole-number-of-ids check and
  the fail-towards-a-miss rule cannot drift between the two readers. Its doc
  comment says plainly that its `Some` is a hypothesis the caller must verify.
- **File-ownership check:** neither 2-06 (`tests/live.rs`,
  `docs/sync-calibration.md`, `docs/sync-format.md`) nor 2-07 (`report.rs`,
  `cli.rs`, `plan.rs`, `widget/cli.rs`) touches `index.rs`, so no worktree
  conflict.
- **Committed in:** `471a43f`

**2. [Simplification] `CHUNK_BYTES` re-exports Phase 1's constant now, not in 2-07**
- **Issue:** The plan specified a duplicated `256 * 1024` literal as deliberate
  drift risk carried "for exactly one wave", because Phase 1 might not have
  merged. It has — `sync::CHUNK_SIZE` and `chunk::sealed_chunk_count` are both
  present in this worktree.
- **Fix:** `pub const CHUNK_BYTES: u64 = CHUNK_SIZE as u64;`, and the sealed
  count comes from `chunk::sealed_chunk_count` rather than a second `/`.
- **Impact:** 2-07's "replace the literal with a re-export" step is already done.

**3. [Scope, additive] `SyncPlan.file_plans`**
- **Issue:** The plan defines `FilePlan` but no field on `SyncPlan` carries one,
  which would have left it unreachable dead surface — and Phase 4 needs the
  per-file chunk list to build a manifest. The plan's own task-2 `<done>` also
  requires a test to compare "the resulting chunk-id list" against a from-scratch
  plan, which needs per-file access.
- **Fix:** one field, `pub file_plans: Vec<FilePlan>`, in scan order.
  `categories` carries the same bytes already aggregated, so 2-07 need not read it.

**Total deviations:** 3 — one unblocking, one removing a drift risk a wave early,
one making a specified type reachable.

## Security notes carried forward

- **T-2-20** (a wrong unchanged verdict) — detection is entirely 2-03's
  `lookup`, which uses all four D5 fields and fails towards `changed`.
  `index_rebuilt` is carried onto the plan for 2-07 to surface.
- **T-2-21** (append fast path) — mitigated by the verification re-hash; both
  failure directions (grow-and-rewrite, shrink) have their own test, and the
  cost of a failed check is reported rather than hidden.
- **T-2-22** (information disclosure) — the only error constructed here is
  `AppError::Io { path, source }` via one `io_at` helper. No file body and no
  chunk id reaches an error or a log line; nothing in this module prints.
- **T-2-23** (memory on large files) — `chunk_from` reads into one reused
  `CHUNK_BYTES` buffer and never holds two chunks.
- **T-2-24** (file changing mid-plan) — accepted as planned. `sealed_chunks` is
  derived from the bytes actually read so the recorded row stays internally
  consistent, and the next run's D5 mismatch re-plans the file.
- **T-2-SC** — no dependency added.
- **Phase 1 NEW-3 (the deferred AAD object-type separator) is still not
  triggered:** this plan seals nothing. It computes ids through an injected
  closure and never calls `chunk::seal_chunk`.
- **No unbounded id list is read before its container authenticates:**
  `Index::cached` reuses `lookup`'s existing `MAX_CHUNK_IDS_BYTES` bound, and
  `verified_prefix` refuses a row claiming more sealed chunks than it stores ids
  for rather than indexing past the end.

## Verification

- `cargo test --lib sync::plan` — **15 passed**.
- `cargo test --lib sync::` — **175 passed**, no regression in `index` or the
  sibling modules after the `read_row` refactor.
- `cargo clippy --all-targets -- -D warnings` — clean.
- `cargo fmt --check` — clean.
- Every test builds its roots from a `TempDir` via `SyncRoots::at` and its index
  via `Index::at`; none calls `default_path()`, `SyncRoots::resolve` or
  `Utc::now()` (`now` is a fixed timestamp), and none depends on Phase 1's keys —
  the chunk id is a toy FNV-shaped closure.
- Fixture cost: one 5.2 MiB write per append test. `tests/live.rs` untouched
  (owned by 2-06).

## Next Phase Readiness

- **2-07** — call `plan::build` with Phase 1's keyed BLAKE3; `CHUNK_BYTES` is
  already the re-export, and `SyncPlan` carries every D4 column
  (`files`, `raw_bytes`, `new_bytes`, plus `excluded_*` which 2-CONTEXT says to
  render for transcripts only). `index_rebuilt` is there to surface when a
  rebuilt index makes a dry-run report everything as new.
- **Phase 4** — `new_chunk_ids` is what to upload; `file_plans` is what to build
  a manifest from.
- **Not delivered here:** the `chunk` table still has no writer, so
  "already uploaded" is known only from the `file` table's rows. Whoever owns
  pack bookkeeping should populate `chunk` and have the planner consult it, which
  would let a chunk shared with a file that failed its append check be recognised
  as already-present instead of re-uploaded.

---
*Phase: 02-bundle-scope-local-index-dry-run-planning*
*Completed: 2026-08-19*
