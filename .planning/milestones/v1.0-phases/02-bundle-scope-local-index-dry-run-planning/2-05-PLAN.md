---
phase: 2-bundle-scope-local-index-dry-run-planning
plan: 05
type: execute
wave: 3
depends_on: ["2-02", "2-03", "2-04"]
files_modified:
  - src/sync/plan.rs
autonomous: true
requirements: [SYNC-01, SYNC-02, SYNC-03]
user_setup: []

must_haves:
  truths:
    - "Planning an unchanged tree twice returns an empty second plan and opens zero file bodies, proven by a counter rather than by timing."
    - "Appending to a large fixture yields a plan of roughly the appended bytes plus one tail chunk, not the whole file."
    - "Truncating that same fixture falls back to a full re-chunk instead of producing a wrong plan."
    - "A file whose content changed in place at the same size and mtime is caught, because the D5 tuple includes mtime_ns and inode, and the append check re-verifies the last sealed chunk."
    - "The plan carries per-category file counts, raw bytes, and new-chunk bytes."
    - "The count of bytes re-read because the append check failed is reported, so the no-CDC decision is measurable in the field."
  artifacts:
    - src/sync/plan.rs
  key_links:
    - "The chunk-id function is a closure parameter, so this plan compiles and tests against Phase 1's contract without Phase 1's code; plan 2-07 supplies the real one at the single call site."
    - "`index.lookup` returning Some is what makes a no-op sync cost a stat sweep; the planner must not open the file on that path."
---

<objective>
Build the change-detection and plan builder: D5's `(path, size, mtime_ns, inode)`
short-circuit, the append fast path that re-reads only the last sealed chunk, and the plan
object carrying new chunk ids plus per-category file/byte totals.

Purpose: this is the object Phase 4 will upload and the number D4's dry-run shows. It is
also where SYNC-01, SYNC-02 and SYNC-03 are actually produced — Phase 4 only transmits the
result.
Output: `src/sync/plan.rs`, ready for plan 2-07 to render and Phase 4 to upload.

**Phase 1 coupling:** this plan builds against Phase 1's *contract* only — chunk id is a
keyed BLAKE3 of the plaintext, chunks are fixed 256 KiB with an explicit tail, zstd runs
before encryption. It needs none of Phase 1's code, because the chunk-id function arrives as
a closure parameter. Plan 2-07 supplies the real one.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/02-bundle-scope-local-index-dry-run-planning/2-CONTEXT.md
@.planning/phases/01-encrypted-bundle-core/1-CONTEXT.md
@.planning/phases/02-bundle-scope-local-index-dry-run-planning/2-01-SUMMARY.md
@.planning/phases/02-bundle-scope-local-index-dry-run-planning/2-03-SUMMARY.md
@.planning/research/chunking-storage.md
@CLAUDE.md
</context>

<tasks>

<task type="auto" tdd="true">
  <name>Task 1: change detection with the zero-read no-op path</name>
  <files>src/sync/plan.rs</files>
  <read_first>.planning/research/chunking-storage.md lines 206-230 (the borg rule and the append fast path pseudocode); .planning/phases/02-bundle-scope-local-index-dry-run-planning/2-03-SUMMARY.md (the exact `Index` API this builds on).</read_first>
  <behavior>
    - First plan over a seeded tree: every file is new, `files_opened` equals the file count, and the plan's chunk set is non-empty.
    - Second plan over the same untouched tree: the plan is empty, `files_opened` is exactly 0, and no chunk is listed as new.
    - A file rewritten with different content and a new mtime is re-chunked.
    - A file whose row is missing from the index is re-chunked even though nothing about it changed on disk.
    - A file under 256 KiB becomes exactly one chunk, matching Phase 1's whole-file-blob rule for small files.
    - A file of exactly 256 KiB becomes one sealed chunk and an empty tail, not two chunks.
  </behavior>
  <action>
Define in src/sync/plan.rs:

```
pub const CHUNK_BYTES: u64 = 256 * 1024;
```
matching Phase 1's fixed chunk size. This constant appearing in two modules is deliberate
duplication for exactly one wave: once Phase 1 has merged, plan 2-07 replaces it with a
re-export of Phase 1's own constant so the two can never drift.

`pub struct FilePlan { pub path: PathBuf, pub chunk_ids: Vec<[u8; 32]>, pub sealed_chunks: u64,
pub new_chunk_ids: Vec<[u8; 32]>, pub new_bytes: u64, pub reused: bool }`

`pub struct CategoryPlan { pub category: SyncCategory, pub files: usize, pub raw_bytes: u64,
pub new_bytes: u64, pub excluded_files: usize, pub excluded_bytes: u64 }`

`pub struct SyncPlan { pub categories: Vec<CategoryPlan>, pub new_chunk_ids: Vec<[u8; 32]>,
pub total_raw_bytes: u64, pub total_new_bytes: u64, pub files_opened: usize,
pub append_check_miss_bytes: u64, pub index_rebuilt: bool }` with
`pub fn is_empty(&self) -> bool` meaning nothing to upload.

The entry point is generic over the chunk-id function rather than taking a trait object or a
fn pointer, because Phase 1's `chunk_id` is `blake3::keyed_hash(name_key, plaintext)` and the
key has to be captured:

```
pub fn build<F: Fn(&[u8]) -> [u8; 32]>(
    roots: &SyncRoots, cfg: &SyncConfig, index: &Index,
    now: DateTime<Utc>, chunk_id: F,
) -> Result<SyncPlan>
```

For each category in `SyncCategory::ALL`, call `scope::collect`, then for each `FileEntry`:
call `index.lookup(entry)`. **A `Some` result ends the work for that file** — reuse the stored
chunk ids, mark `reused`, call `index.touch`, and do not open the file. That single branch is
SYNC-02; everything else in this module is the slow path.

`files_opened` increments in exactly one place: immediately before the first read of a file's
body. Make that the only `File::open` in the module so the counter cannot drift from reality.
The roadmap requires the no-op property be asserted by a read counter, not by timing, and this
field is that counter.

Chunking follows Phase 1's contract: offset-aligned 256 KiB chunks plus an explicit tail.
`sealed_chunks = size / CHUNK_BYTES`; the remainder is the unsealed tail, which is a chunk in
its own right unless it is zero-length. Ids come from the injected `chunk_id` over the
plaintext, never over compressed or encrypted bytes — this module never compresses and never
encrypts. `new_chunk_ids` is the subset not already present in the index's `chunk` table,
deduplicated across files, since two identical chunks upload once.

Read file bodies in `CHUNK_BYTES` buffers; never load a 50 MB transcript into memory.

After planning, `index.record` each changed file and `index.bump_generation`. Carry
`index.was_rebuilt()` into `index_rebuilt`.
  </action>
  <verify>
    <automated>cargo test --lib sync::plan</automated>
  </verify>
  <done>Every bullet in `&lt;behavior&gt;` has a passing test using `SyncRoots::at` over a `TempDir`, `Index::at` over a second `TempDir`, and a toy chunk-id closure. The unchanged-tree test asserts `files_opened == 0` exactly.</done>
</task>

<task type="auto" tdd="true">
  <name>Task 2: the append fast path and its miss counter</name>
  <files>src/sync/plan.rs</files>
  <read_first>.planning/research/chunking-storage.md lines 386-422 (the worked append example and the numbers this must reproduce).</read_first>
  <behavior>
    - Appending 200 KiB to a 5 MiB fixture yields one newly-sealed chunk plus one new tail, and a new-bytes total under 400 KiB.
    - That same append opens the file once and re-reads exactly one 256 KiB verification chunk plus the appended region.
    - Truncating the fixture triggers a full re-chunk, and the resulting plan is correct rather than a short chunk list.
    - Overwriting the middle of the fixture in place at unchanged length triggers the append check, which fails, which triggers a full re-chunk — and `append_check_miss_bytes` records the re-read.
    - A file that grew but whose last sealed chunk no longer matches falls back to a full re-chunk.
    - A file whose cached `sealed_chunks` is 0 skips the verification read entirely and chunks from offset 0.
  </behavior>
  <action>
Implement the fast path from the research, in the changed-file branch only:

```
if new_size >= cached.sealed_chunks * CHUNK_BYTES && cached.sealed_chunks > 0:
    re-read chunk[cached.sealed_chunks - 1]        // one 256 KiB read
    if chunk_id(that) == cached.chunk_ids[sealed_chunks - 1]:
        genuine append — reuse ids [0 .. sealed_chunks) and chunk from
        sealed_chunks * CHUNK_BYTES onward
    else:
        full re-chunk, and add the 256 KiB to append_check_miss_bytes
else:
    full re-chunk        // truncation, or nothing cached to verify against
```

This verification read is the whole reason fixed-size chunking is safe here without
content-defined chunking. Appends displace no bytes, so every sealed chunk before the old
end-of-file is provably unchanged — but that is a property of appends, not of every write, so
it is *checked* rather than assumed. `append_check_miss_bytes` is the field that says in the
field whether that assumption held; if it is ever non-trivial, the chunker changes, which is
why Phase 1 records `"chunker": "fixed-256k"` in the snapshot.

Use a 5 MiB fixture, not the roadmap's 50 MB one. The arithmetic is identical — 20 sealed
chunks instead of 190 — and the AUR `check()` runs this test during `makepkg` on a user's
machine, so a 50 MB write per test run is a cost paid by every installer. Put the fixture size
behind a private const and say so in a comment.

Track bytes actually read from disk in a private counter so the "re-reads exactly one
verification chunk plus the appended region" assertion is a real measurement rather than an
inference from the plan's contents.
  </action>
  <verify>
    <automated>cargo test --lib sync::plan</automated>
  </verify>
  <done>Every bullet in `&lt;behavior&gt;` has a passing test. The append test asserts both the chunk count and the byte total; the truncation test asserts the resulting chunk-id list matches a from-scratch plan over the same truncated file.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| local index → planner | A hint file decides which files this tool declines to read, and therefore which files it declines to sync. |
| user filesystem → planner | Files may change under the planner mid-run. |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|-----------|----------|-----------|----------|-------------|-----------------|
| T-2-20 | Tampering | `index.lookup` short-circuit | critical | mitigate | A wrong unchanged verdict silently omits a file from every future sync. Detection uses all four D5 fields; a missing, corrupt or future-version index reports everything changed via plan 2-03's rebuild path, and `index_rebuilt` is surfaced. |
| T-2-21 | Tampering | append fast path | high | mitigate | Appendness is verified by re-hashing the last sealed chunk, never assumed from a size increase. A mismatch or a shrink both fall back to a full re-chunk. Both directions are tested. |
| T-2-22 | Information disclosure | error and log output | high | mitigate | Errors name paths and byte counts only. No file body, and no chunk id, reaches a log line or an error message. |
| T-2-23 | Denial of service | memory use on large files | medium | mitigate | Bodies are read in 256 KiB buffers; a 50 MB transcript is never resident. |
| T-2-24 | Tampering | file changing mid-plan | low | accept | A file written during the scan may be planned in a torn state. The next sync's D5 tuple differs and re-plans it, and Phase 1's manifest binds chunk ids so a torn read cannot corrupt an existing snapshot. |
| T-2-SC | Tampering | npm/pip/cargo installs | high | accept | This plan adds no dependency. `cargo machete` runs in the phase-end gate. |
</threat_model>

<verification>
`cargo test --lib sync::plan` is green. Every test injects its roots and its index from
`TempDir`s and passes a toy chunk-id closure; no test constructs a real home path, and no test
depends on Phase 1's code.
</verification>

<success_criteria>
Re-planning an unchanged tree returns an empty plan with `files_opened == 0`. Appending
200 KiB to the fixture plans under 400 KiB of new bytes. Truncating it falls back to a full
re-chunk. `append_check_miss_bytes` reports the cost of every fallback.
</success_criteria>

<output>
Create `.planning/phases/02-bundle-scope-local-index-dry-run-planning/2-05-SUMMARY.md` when done.
Record `SyncPlan`'s exact fields and `build`'s signature — plan 2-07 renders the first and
calls the second, and Phase 4 uploads the result.
</output>
