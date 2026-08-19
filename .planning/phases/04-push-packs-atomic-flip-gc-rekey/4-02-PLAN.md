---
phase: 04-push-packs-atomic-flip-gc-rekey
plan: 02
type: execute
wave: 2
depends_on: ["4-01"]
files_modified:
  - src/sync/push/packer.rs
  - src/sync/index.rs
autonomous: true
requirements: [REPO-06]
must_haves:
  truths:
    - "A `SyncPlan` of ~5,000 new chunks becomes a handful of packs, never one object per chunk — the property REPO-06 names, asserted as a pack count."
    - "A chunk already recorded in the local `chunk` table is not re-sealed and not re-packed; the table has a writer for the first time, closing 2-05's recorded gap."
    - "A chunk shared with a file that failed its append check is recognised as already-present instead of being re-uploaded."
    - "`PACK_TARGET` is unchanged at 32 MiB, and a test proves the pack header of a worst-case full pack still fits one sealed chunk — the ceiling Phase 1's gap-closure deliberately did not reach."
    - "The manifest, the index object, and the snapshot root are produced by Phase 1's own `seal`/`new` entry points; nothing here re-implements a format."
    - "`referenced_packs` names every pack the snapshot needs, reused ones included, so prune can be computed from the pointer with no download and no key."
    - "Nothing in this file seals a new kind of object under `chunk_key`."
  artifacts:
    - src/sync/push/packer.rs — `build`, the pack-fill loop, and the manifest/index/root assembly
    - "`Index::{record_chunks, known_chunks, forget_chunks}` in src/sync/index.rs — the `chunk` table's first writer"
  key_links:
    - "`packer::build` is the only producer of `PushBundle`; upload, pointer, and prune all consume what it names"
    - "The `chunk` table is what makes `already uploaded` survive a lost local plan; without it the answer comes only from the `file` table"
    - "`should_seal` and `PACK_TARGET` come from `pack.rs` and are read, never redefined — a second literal is how the header ceiling gets re-broken"
---

<objective>
Turn Phase 2's `SyncPlan` into the bytes a push puts on the wire: sealed chunks packed into a
handful of 32 MiB objects, a manifest, an index object, and a snapshot root — and give the local
`chunk` table its first writer, so "already uploaded" stops being a guess derived from the `file`
table alone.

Implements **REPO-06** (a small number of large objects) and closes the gap
`2-05-SUMMARY.md` recorded under "Not delivered here".

Purpose: this is the object that makes the 80/min content-creation limit irrelevant.
Output: `packer::build`, and three chunk-table accessors on `Index`.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/04-push-packs-atomic-flip-gc-rekey/4-CONTEXT.md
@.planning/phases/04-push-packs-atomic-flip-gc-rekey/4-01-SUMMARY.md
@.planning/phases/02-bundle-scope-local-index-dry-run-planning/2-05-SUMMARY.md
@docs/sync-format.md
@CLAUDE.md
@src/sync/pack.rs
@src/sync/chunk.rs
@src/sync/model.rs
@src/sync/index.rs
@src/sync/plan.rs
</context>

<tasks>

<task type="auto" tdd="true">
  <name>Task 1: The `chunk` table's first writer</name>
  <files>src/sync/index.rs</files>
  <behavior>
    - `record_chunks` inserts a batch of `(id, pack, offset, clen, plen)` rows stamped with the current generation, and re-recording an id updates its location rather than duplicating it.
    - `known_chunks` given a slice of ids returns the subset already present, in one query rather than one per id.
    - `forget_chunks` removes the ids naming a pack that has been deleted, so a pruned pack leaves no row pointing at it.
    - An index whose `chunk` table is missing or malformed degrades to "nothing known" and re-plans, exactly as `lookup` degrades — never to an error mid-push.
    - A row whose `clen` or `offset` is negative, or whose `id`/`pack` blob is not 32 bytes, is treated as absent rather than materialised.
  </behavior>
  <action>
The `chunk` table has existed since plan 2-03 and has never had a writer; `2-05-SUMMARY.md`
records the consequence, which is that a chunk shared with a file that failed its append check
gets re-uploaded because the only evidence of "already present" lives in the `file` table.
Close that.

Add three methods to `Index`, alongside the existing `lookup` / `cached` / `record` / `touch`:

`pub fn record_chunks(&self, rows: &[(ChunkId, ChunkId, u64, u32, u32)]) -> Result<usize>` —
`(chunk id, pack id, offset, clen, plen)`, inserted with `INSERT OR REPLACE` inside **one**
transaction and stamped with `self.generation()` in the same statement, matching how `record` and
`touch` already stamp themselves. Batch, not row-at-a-time: a first push records thousands of
rows and a transaction per row is the difference between a second and a minute.

`pub fn known_chunks(&self, ids: &[ChunkId]) -> HashSet<ChunkId>` — one query with a bounded
`IN` list, chunked into batches under SQLite's variable limit. It returns a set rather than a
`Vec` because the caller's question is membership. Follow the module's existing rule and **fail
towards not-known**: a query error, a malformed row, or a blob of the wrong length yields
absence, which costs a re-upload of already-present bytes and never a wrong "already there".
Reuse the same validation shape `read_row` already applies, and reject a negative `offset` or
`clen` rather than casting it.

`pub fn forget_chunks(&self, packs: &[ChunkId]) -> Result<usize>` — delete every row whose `pack`
is in the list. Prune calls it after a pack is actually gone from the remote, so the local index
never claims a chunk lives somewhere it does not. Deleting rows is safe by construction: the
worst outcome is a re-upload.

Do not touch `SCHEMA` or `SCHEMA_VERSION`. The table is already there; only the accessors are
new. Do not extend `evict_unseen` to the chunk table in this plan — a chunk row that outlives its
pack is corrected by `forget_chunks`, and an eviction policy for chunk rows is a second lifetime
rule with nothing asking for it yet.

Tests build their index through `Index::at` against a `TempDir`, as every existing test in this
file does.
  </action>
  <verify>
    <automated>cargo test --lib sync::index</automated>
  </verify>
  <done>`cargo test --lib sync::index` is green, including the pre-existing tests. The three accessors exist, batch their SQL, and degrade to "nothing known" on every malformed input rather than erroring. `SCHEMA_VERSION` is unchanged.</done>
</task>

<task type="auto" tdd="true">
  <name>Task 2: `SyncPlan` to `PushBundle` — packing, manifest, index object, root</name>
  <files>src/sync/push/packer.rs</files>
  <behavior>
    - A plan whose new chunks total ~160 MiB of plaintext produces 5 or 6 packs, not thousands of objects — asserted as a pack count against `PACK_TARGET`.
    - Every produced pack is under `PACK_MAX`, and every pack's `id` equals `content_address` of its own bytes.
    - Re-running `build` over an unchanged tree with a populated `chunk` table produces zero new packs and a `PushBundle` whose `referenced_packs` still names every pack the snapshot needs.
    - A chunk present in the `chunk` table is neither re-sealed nor re-packed, even when the file that owns it failed its append check.
    - The manifest reassembles through `Manifest::open`, and the root opens through `Root::open` under the caller's own `repo_id`.
    - `counter` is exactly one above the previous snapshot's, and one above the local anchor's high-water mark on a first push.
    - A worst-case pack — `PACK_TARGET` filled entirely with the smallest blobs the format admits — produces a header that still seals as a single chunk.
  </behavior>
  <action>
Fill `packer::build(ctx: &PushCtx<'_>, plan: &SyncPlan) -> Result<PushBundle>`, whose signature
plan 4-01 froze.

**The fill loop.** For each new chunk id in `plan.new_chunk_ids`, skip it when
`ctx.index.known_chunks` already has it — that is task 1 earning its keep, and it is what makes a
chunk shared with an append-check failure free rather than re-uploaded. For the rest, read the
bytes from the file the plan names, seal through `chunk::seal_chunk`, and push into a
`PackWriter`. Before each push, ask `pack::should_seal(writer.len_bytes(), blob.ciphertext.len())`
and, when it says so, `finish` the writer into a `BuiltPack` and start a new one. Use
`pack::PACK_TARGET` and `pack::should_seal` as they are. **Do not introduce a second size
literal and do not raise `PACK_TARGET`** — `docs/sync-format.md` §7 records that CAL-1 was not
run, so 32 MiB is the standing answer, and the entry-count ceiling below is a function of it.

**The header ceiling, which is the one thing in this plan that is easy to get quietly wrong.**
The pack header is still a single sealed chunk: `pack.rs` seals it through `chunk::seal_chunk`,
which Phase 1's gap-closure 1-09 deliberately did not reach when it made manifests and index
objects multi-chunk. At 32 MiB of 256 KiB chunks a pack holds about 128 entries against a limit
in the thousands, so today it is slack rather than a limit. Write the test that keeps it that
way: construct a pack filled to `PACK_TARGET` with the **smallest** blobs the format admits,
serialize its header, and assert the header's JSON is comfortably inside one `CHUNK_SIZE` frame.
Name in the test's own doc comment that the upgrade path, if this ever fails, is a format-2
multi-chunk header through the same `chunk::seal_all` / `reassemble` pair the manifest now uses —
and that raising the pack target is what would break it.

**Chunk bookkeeping.** As each pack is finished, record every entry it holds through
`Index::record_chunks`, mapping the `PackEntry`'s `id`, `offset`, `clen`, and `true_len` against
the pack's own content address. Record after `finish`, not before: the pack's id does not exist
until its header is sealed, and a row written against a pack id that never materialised is a
pointer to nothing.

**The three objects.** Build `model::Manifest` from `plan.file_plans` — path, mode, `true_len`,
and the ordered chunk id list, all of which `FilePlan` already carries — and `seal` it. Build
`model::IndexObject` from every entry across every pack this snapshot references, with
`supersedes` naming the index-object chunk ids the previous snapshot used, and `seal` it. Both
`seal` calls return `Vec<Blob>` because both objects span as many chunks as they need; pack those
blobs like any others. Then build `model::Root::new` with the manifest's chunk ids in order, the
caller's `repo_id`, the keyfile's `KdfParams`, and `ctx.now`, and `seal` it.

The index object's own chunks are the bootstrap: collect their `(id, pack, offset, clen,
true_len)` into `RemoteIndexEntry` values for `PushBundle.index_chunks`. Without them a reader
holding the pointer can resolve nothing, so assert in a test that every id in `index_chunks`
appears in some pack this bundle either produces or already references.

**`counter`.** One above the previous snapshot's, read by opening the newest root the pointer
carries; one above the local anchor's high-water mark on a first push. Do **not** advance the
anchor here — Phase 1's rule is that the anchor advances only after a snapshot verifies, and this
code is producing one, not verifying it.

**`referenced_packs`.** Every pack the snapshot needs: the ones built here, plus the pack of every
reused chunk, resolved from the `chunk` table. Reused ones are not optional. A snapshot that
names only its new packs would let prune delete the packs holding all of its unchanged data —
the unrestorable-backup outcome D2 exists to prevent.

**Sizes are exact here, and that matters.** Phase 2 projects a chunk's cost as
`frame(window).len() + 40` per window because padding makes a ratio about 40% wrong in the user's
favour. This module holds the finished packs, so every byte count it reports is
`pack.bytes.len()`, measured. Progress and the outcome totals use these numbers and never
re-project.

Nothing in this file seals a new **kind** of object: packs, manifests, index objects, and roots
are the four kinds the format already defines, so Phase 1's deferred AAD object-type separator
stays untriggered. If the implementation finds itself calling `seal_chunk` on something that is
none of those four, stop and raise it rather than proceeding.

Fixtures are sized for the AUR `check()` — the pack-count test needs enough plaintext to cross
`PACK_TARGET` a few times, so generate it rather than writing it to disk, and keep the on-disk
fixture small. Every test injects its roots through `SyncRoots::at` and its index through
`Index::at`, with cheap KDF parameters.
  </action>
  <verify>
    <automated>cargo test --lib sync::push::packer</automated>
  </verify>
  <done>`cargo test --lib sync::push::packer` is green. A plan crossing `PACK_TARGET` several times produces that many packs and no more. Every pack is under `PACK_MAX` and self-addressing. A worst-case header still seals as one chunk, proven by a test that would fail if `PACK_TARGET` were raised. `referenced_packs` includes reused packs. The manifest and root round-trip through Phase 1's own `open` entry points. `PACK_TARGET` and `PACK_MAX` are unchanged.</done>
  <reversibility rating="costly">`PushBundle`'s contents are what upload, pointer, and prune all consume. Raising `PACK_TARGET` here would be one-way: it changes every future pack boundary and re-opens the single-chunk header ceiling that 1-09 did not reach.</reversibility>
  <precondition>Plan 4-01 is merged: `PushCtx`, `PushBundle`, `BuiltPack`, and `RemoteIndexEntry` exist with the signatures `4-01-SUMMARY.md` records. Task 1 of this plan is merged before task 2 runs — `build` calls `known_chunks` and `record_chunks`.</precondition>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|---|---|
| local files → sealed chunks | Plaintext credentials cross into ciphertext here; nothing downstream may ever see the plaintext form |
| local SQLite index → planning decisions | The index is unauthenticated local state any process on the machine can write |
| plan → pack boundaries | Sizes and offsets computed here become the offsets a reader slices a pack at |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|---|---|---|---|---|---|
| T-4-12 | Tampering | a poisoned `chunk` table claiming a chunk is present | high | mitigate | `known_chunks` fails towards not-known on every malformed input, so the worst a poisoned row buys is a re-upload; a row cannot cause a chunk to be *omitted* from `referenced_packs`, which is derived from the plan's own id list |
| T-4-13 | Tampering | `referenced_packs` missing a reused pack | critical | mitigate | Reused chunks' packs are resolved and included, and a test asserts a second push over an unchanged tree still names every pack; omitting them is the exact input that makes prune delete live data |
| T-4-14 | Denial of service | an oversized pack header | high | mitigate | The worst-case-header test pins the single-chunk ceiling to the current `PACK_TARGET`, so raising the target cannot silently reintroduce the failure 1-09 removed |
| T-4-15 | Information disclosure | plaintext or a chunk id in an error or a log line | high | mitigate | Errors carry a path and an io source only, following 2-05's single `io_at` helper; nothing in this module prints |
| T-4-16 | Information disclosure | plaintext held longer than needed | medium | mitigate | One reused chunk-sized buffer, as 2-05's `chunk_from` already does; sealed output replaces plaintext rather than accumulating beside it |
| T-4-17 | Tampering | a new object kind sealed under `chunk_key` | high | mitigate | Only the format's four existing kinds are sealed; the deferred AAD object-type separator stays untriggered, and the executor is instructed to stop rather than add a fifth |
| T-4-18 | Repudiation | a chunk row written against a pack that never materialised | medium | mitigate | Rows are recorded after `PackWriter::finish`, when the pack's content address exists |
| T-4-SC | Tampering | dependency surface | low | accept | Zero new crates. `rusqlite`, `serde_json`, and `blake3` are declared today; `Cargo.toml` is not in `files_modified` |
</threat_model>

<verification>
- `cargo test --lib sync::push::packer` and `cargo test --lib sync::index` are green.
- `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` are clean.
- `src/sync/pack.rs` is unchanged; `PACK_TARGET` and `PACK_MAX` still read 32 MiB and 48 MiB.
- `SCHEMA` and `SCHEMA_VERSION` in `src/sync/index.rs` are unchanged.
- Every test injects its roots and its index; none reads a real `$HOME` or uses production KDF
  parameters.
</verification>

<success_criteria>
A `SyncPlan` becomes a `PushBundle` of a handful of packs plus a manifest, an index object, and a
snapshot root — with reused chunks recognised from the local `chunk` table rather than
re-uploaded, every pack the snapshot needs named, and the single-chunk pack-header ceiling pinned
by a test to the pack target that justifies it.
</success_criteria>

<output>
Create `.planning/phases/04-push-packs-atomic-flip-gc-rekey/4-02-SUMMARY.md` when done.

Record the measured pack count and total bytes for the test fixture, the measured worst-case
header size against the `CHUNK_SIZE` ceiling, and the three `Index` accessor signatures. State
plainly whether `PACK_TARGET` was touched, and whether any new object kind was sealed under
`chunk_key`.
</output>
