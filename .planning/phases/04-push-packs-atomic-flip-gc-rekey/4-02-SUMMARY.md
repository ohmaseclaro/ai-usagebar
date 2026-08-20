---
phase: 04-push-packs-atomic-flip-gc-rekey
plan: 02
subsystem: transport
tags: [packing, repo-06, chunk-table, index-object, manifest, snapshot-root, pack-max, hermetic-tests]

requires:
  - phase: 04-push-packs-atomic-flip-gc-rekey
    plan: 01
    provides: "`PushCtx` (eleven fields, incl. `kdf` and `previous`), `PushBundle`, `BuiltPack`, `RemoteIndexEntry`, `Pointer`, `SnapshotRecord`, `B64`"
  - phase: 02-bundle-scope-local-index-dry-run-planning
    plan: 05
    provides: "`SyncPlan`, `FilePlan`"
  - phase: 02-bundle-scope-local-index-dry-run-planning
    plan: 03
    provides: "the `chunk` table in `SCHEMA`, which had no writer until now"
  - phase: 01-encrypted-bundle-core
    provides: "`PackWriter`, `should_seal`, `PACK_MAX`, `Manifest`, `IndexObject`, `Root`, `Keys`, `content_address`, `seal_chunk`"
provides:
  - "`packer::build` — the only producer of `PushBundle`"
  - "`Index::{record_chunks, known_chunks, forget_chunks}` — the `chunk` table's first writer"
  - "`Index::chunk_locations` and `index::ChunkLocation` — **added**, see the signature notice below"
affects: [4-03, 4-05, 4-07, phase-5-restore]

tech-stack:
  added: []
  patterns:
    - "Reuse is a two-key question: the local chunk table answers *where*, the arriving pointer answers *whether that pack ever landed*. Neither alone is evidence."
    - "`referenced_packs` is derived from the index object rather than accumulated beside it, so it is total by construction instead of by discipline."
    - "A size guard is built at the constant the code actually compares against, never at the advisory one."
    - "An expensive-crypto property test drives the private seam with cloned blobs, following `pack.rs`'s own precedent, so an AUR `check()` does not pay 20 s of debug-mode ChaCha."

key-files:
  created: []
  modified:
    - src/sync/index.rs
    - src/sync/push/packer.rs

requirements-completed: [REPO-06]

duration: 2h
completed: 2026-08-19
status: complete
---

# Phase 4 / Plan 02: `SyncPlan` to `PushBundle`

**385 chunks become three 48 MiB objects, and a chunk a published snapshot
already holds is neither re-read, re-sealed, nor re-uploaded.** The local
`chunk` table gained the writer it has been missing since 2-03, and
`referenced_packs` is now derived from the index object rather than
accumulated, which makes "the snapshot names every pack it needs" a
structural property rather than a rule someone has to keep.

## Task commits

1. `c1e4bbe` — the chunk table's accessors, as failing tests
2. `c094d61` — the chunk table's first writer
3. `a542471` — `SyncPlan` to `PushBundle`: packs, manifest, index object, root

## SIGNATURES — one addition, nothing changed

**Nothing frozen moved.** `packer::build`, `packer::manifest_path`, and all
three `Index` accessors the plan named carry exactly the signatures it froze:

```rust
// src/sync/push/packer.rs — unchanged from 4-01's freeze
pub fn build(ctx: &PushCtx<'_>, plan: &SyncPlan) -> Result<PushBundle>;
pub fn manifest_path(roots: &SyncRoots, path: &Path) -> Result<String>;

// src/sync/index.rs — as planned
pub fn record_chunks(&self, rows: &[(ChunkId, ChunkId, u64, u32, u32)]) -> Result<usize>;
pub fn known_chunks(&self, ids: &[ChunkId]) -> HashSet<ChunkId>;
pub fn forget_chunks(&self, packs: &[ChunkId]) -> Result<usize>;
```

**One accessor was added, and 4-05 and Phase 5 should know it exists:**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkLocation {
    pub pack: ChunkId,
    pub offset: u64,
    pub clen: u32,
    pub plen: u32,
}

pub fn chunk_locations(&self, ids: &[ChunkId]) -> HashMap<ChunkId, ChunkLocation>;
```

Why it had to exist: the plan gives `known_chunks` a `HashSet` return because
"the caller's question is membership". That is true of the fill loop and false
of everything else in `build`. The **index object** is the chunk-id → pack
location map for the snapshot, so a reused chunk needs a full `IndexEntry` —
pack, offset, clen, plen — or a restore of an incremental snapshot cannot find
the unchanged data at all. `referenced_packs` needs the pack id for the same
reason. Membership alone cannot produce either. `chunk_locations` is the single
query; `known_chunks` is one line on top of it and keeps the frozen surface
honest for callers that only need the yes/no.

`prune` (4-05) wants `forget_chunks(&[pack_ids])` after the deletes land.

## The object build order — Phase 5 walks this in reverse

1. **Data chunks** — every chunk the manifest will name that the snapshot
   cannot already locate, sealed and packed.
2. **Manifest** — built from `plan.file_plans`, sealed (`Vec<Blob>`; it spans as
   many chunks as it needs), packed, **and the writer flushed**. The flush is
   load-bearing: an entry's pack id does not exist until the header is sealed.
3. **Index object** — reused entries from the chunk table, plus every entry
   packed in steps 1 and 2. Building it before step 2's flush is how the root's
   `manifest_chunks` end up naming ids the index object does not describe, which
   is a bundle that passes every test here and that no restore can read.
4. **Index object's own chunks** → `PushBundle.index_chunks`, the plaintext
   bootstrap in the pointer. Nothing describes itself.
5. **Snapshot root** — `Root::new(counter, now, repo_id, manifest_chunks, kdf)`,
   with `kdf` taken from `ctx.kdf` as instructed and never re-read off disk.

Steps 2 and 4 each end in a flush, so a push produces **at least two packs**:
one carrying data and the manifest, one carrying the index object.

## Measured numbers

| | |
|---|---|
| Sealed full `CHUNK_SIZE` chunk | **262,207 bytes** ciphertext |
| Blobs a pack holds at `PACK_MAX` | **191** (`PACK_MAX / 262,207`) |
| Fill-loop fixture | **385 blobs**, 100,949,695 bytes (96.3 MiB) of ciphertext |
| Packs produced | **3** — `385.div_ceil(191)`, derived from the constant, not written down |
| Worst-case pack header, built at `PACK_MAX` | 191 entries, **23,669 bytes** JSON |
| Single-chunk ceiling | 262,144 bytes — **11× slack** |

**The worst-case header was built at `PACK_MAX`, not at `PACK_TARGET`.** The
test says so in its own doc comment, along with the upgrade path if it ever
fails (a format-2 multi-chunk header through the same `seal_all`/`reassemble`
pair the manifest uses) and the fact that raising `PACK_MAX` is what would
break it.

**Neither pack constant was touched.** `PACK_TARGET` still reads 32 MiB,
`PACK_MAX` still reads 48 MiB, and `src/sync/pack.rs` is byte-identical to the
branch point. **No new kind of object was sealed under `chunk_key`** — only
packs, manifests, index objects and roots, the four the format already defines,
so Phase 1's deferred AAD object-type separator stays untriggered.

## Deviations from the plan

### 1. [Rule 2 — missing critical correctness] Reuse asks the pointer, not only the chunk table

The plan's rule is "skip a chunk when `known_chunks` already has it". That is
unsafe on its own, and the failure it produces is D2's worst outcome.

The chunk table records what this machine **packed**, which is not what
*landed*. A push that packs and then fails at upload — a dropped connection, a
403, anything — leaves rows pointing at packs no remote ever saw. The next push
would skip those chunks, put their packs into `referenced_packs`, and publish a
pointer naming assets that do not exist: an unrestorable backup, reached with
nobody doing anything wrong. Nothing downstream catches it, because
`upload::run` only verifies packs it uploaded.

So `reusable()` intersects the chunk table with the packs named by a snapshot
the **arriving pointer** already carries. That is the remote's own evidence that
the pack landed *and* has not since been pruned. On a first push the set is
empty and everything is sealed, which is correct.
`a_chunk_whose_pack_no_published_snapshot_names_is_packed_again` pins it.

### 2. [Rule 2] The index object carries reused entries, and `referenced_packs` is derived from it

The plan's step 2 says the index object covers "the data chunks **and** the
manifest's chunks". Read narrowly as *this run's* entries, an incremental
snapshot's index object would not describe the unchanged data, and a restore
could not resolve it. It therefore carries an entry per chunk the snapshot
names, reused ones resolved through `chunk_locations`.

`referenced_packs` is then the unique pack set over `index_object.entries` plus
`index_chunks` — **derived, not accumulated**. A reused pack cannot be forgotten,
because forgetting it would mean the index object could not describe a chunk the
manifest names.

### 3. [Rule 1 — bug in the inherited tracer] Entries were read before the writer was flushed

4-01's `build` read `packs.entries` while a writer was still open. Entries only
gain a pack id at `finish`, so on the tracer's own single-pack path the index
object would have been built with **zero entries** and `index_chunks` would have
been **empty** — a pointer with no bootstrap. Fixed by the two explicit flushes
in the build order above.

### 4. [Rule 2] Files are streamed, and a fully-locatable file is not opened

T-4-16 asks for "one reused chunk-sized buffer". The inherited code did
`std::fs::read` on every file in the plan, holding every credential in the
bundle in memory at once and re-reading 115 MB on a push that changed one small
file. `pack_file` now streams `CHUNK_SIZE` blocks through a single
`Zeroizing` buffer, and a file whose every chunk the snapshot can already locate
is never opened — SYNC-02 honoured rather than claimed.

Consequence: `true_len` and `mode` come from one `fs::metadata` call instead of
from the bytes read. The skip decision is made on the id of the block actually
read, not on the plan's list by position, so if a file changed under us the id
we skip on is the id the blob would have had.

### 5. [Contract clarification] "Zero new packs on an unchanged tree" is two packs, not zero

The must-have reads "re-running `build` over an unchanged tree … produces zero
new packs". Unreachable by construction: every snapshot seals a fresh manifest
and a fresh index object, and those have to be packed. What is asserted instead,
and is the property that actually matters, is that **no data chunk is re-sealed
or re-packed** — read back through `pack::read_header` rather than from the
builder's own bookkeeping — while the pack holding it is still named. A second
push over an unchanged tree produces exactly two small packs.

### 6. [Not implementable in scope] The anchor's high-water mark on a first push

The plan asks for "one above the local anchor's high-water mark on a first
push". `anchor.rs` has `read_from(&Path)` and no path resolver anywhere in the
crate — nothing has ever written an anchor, and its own module docs say the
resolver "belongs to whichever phase owns the config directory", keyed to the
*remote* rather than to a `repo_id`. Inventing that path inside `packer.rs`
would pre-empt a decision Phase 5 owns, and `anchor.rs` is not this plan's file.

A first push therefore starts at **counter 1**, which is provably identical to
reading an anchor that is never written. When Phase 5 adds the resolver,
`next_counter`'s `None` arm is where it plugs in.

The *published* case did get the full treatment: the counter is one above the
highest counter found by opening the pointer's sealed roots, not one above the
pointer's length. Position in `snapshots` is remote-controlled; the counter
inside a root is not.

### 7. [Test economics] The fill-loop test clones blobs instead of sealing 96 MiB

Measured in this worktree, debug-mode sealing runs at ~5 MB/s (50 MiB took
9.9 s; ChaCha20-Poly1305 unoptimised is the bottleneck, with zstd second). The
plan's ~160 MiB fixture would have added **~32 s** to a suite that runs in 2.6 s,
on installers' machines during the AUR `check()`.

`pack.rs`'s own `PACK_MAX` test already set the precedent — "cloned rather than
resealed: the sizes are what this test is about, and 191 zstd passes over
256 KiB are not". The fill loop is driven directly with 385 clones of one
genuinely sealed chunk, which costs ~2 s and asserts everything the property
needs: the pack count derived from `PACK_MAX`, every pack at or under
`PACK_MAX`, every pack self-addressing through `content_address`, every header
readable, and every entry naming a pack that exists. The end-to-end `build`
tests use small fixtures and cover the object graph, ordering, reuse, and
`referenced_packs`.

Suite time went 2.60 s → 4.53 s.

## Not done here, and why

- **The keyfile is still never uploaded.** 4-01's deviation 9 assigned it to
  4-03, which owns `upload.rs` and is adding
  `ensure_keyfile(ctx, release_id, permit) -> Result<()>`. **No competing
  keyfile path exists in `packer.rs`** — the string `keyfile` does not appear in
  it, and `ctx.keyfile_asset` is untouched.
- **`plan.new_chunk_ids` is not consulted.** The authoritative set is the union
  of `file_plans[].chunk_ids`: a snapshot must name a pack for *every* chunk it
  references, and filtering by the plan's new ids is exactly how
  `referenced_packs` ends up missing the packs holding all the unchanged data.
- **`evict_unseen` was not extended** and `SCHEMA` / `SCHEMA_VERSION` were not
  touched. `evict_unseen` already ages `chunk` rows on the same clock as `file`
  rows; a separate lifetime rule has nothing asking for it.

## Security properties, and how each is enforced

| Property | Enforcement |
|---|---|
| T-4-12 — a poisoned `chunk` table | `chunk_locations` fails towards not-known on a SQL error, a missing table, a blob that is not 32 bytes, and a negative `offset`/`clen`/`plen` — rejected rather than cast, because `as u64` turns −1 into a plausible offset into somebody's pack. Five damage shapes are asserted. A row cannot cause a chunk to be *omitted*: `referenced_packs` is derived from the manifest's own id list. |
| T-4-13 — `referenced_packs` missing a reused pack | Derived from the index object, so a missing pack would mean an unresolvable chunk. Asserted on a second push over an unchanged tree and on the append-check-failure case. Deviation 1 closes the harder half: a pack that never landed is not reusable at all. |
| T-4-14 — an oversized pack header | The worst case is built at `PACK_MAX`: 191 entries, 23,669 bytes against a 262,144-byte ceiling, with every field at its widest. Raising `PACK_MAX` moves the test's own expectation, so it cannot silently reintroduce the failure 1-09 removed. |
| T-4-15 — plaintext or a chunk id in an error or a log | Every error carries a path and an io source, through `AppError::io_at`. Nothing in either file prints; `index.rs`'s one `eprintln!` is the pre-existing rebuild notice, path and reason only. |
| T-4-16 — plaintext held longer than needed | One reused `Zeroizing<Vec<u8>>` of `CHUNK_SIZE`, streamed; no whole-file allocation, and a fully-locatable file is never opened. Asserted from the other side too: the temp-dir prefix appears in no pack and not in the root. |
| T-4-17 — a new object kind under `chunk_key` | Only packs, manifests, index objects and roots are sealed. Nothing else calls `seal_chunk`. |
| T-4-18 — a row against a pack that never materialised | `record_chunks` runs once, after the final flush, over `packs.entries` — every one of which carries a real content address. |
| T-4-SC — dependency surface | Zero new crates. `Cargo.toml` and `Cargo.lock` are byte-identical to the branch point. |

Additionally: a chunk table this build cannot read is now discarded at **open**
(a prepare-only probe beside the existing one for `file`), where the answer can
still be "throw the index away". Its reads degraded on their own, but
`record_chunks` is a write in the middle of a push, where the answer can no
longer be "discard".

## Verification

```
cargo test --lib sync::                     333 passed, 0 failed
cargo test --lib                           1315 passed, 0 failed, 0 ignored   (baseline 1300)
cargo clippy --all-targets -- -D warnings   clean
cargo fmt --check                           clean
Cargo.toml / Cargo.lock                     unchanged
git diff --stat 688beb3..HEAD               src/sync/index.rs, src/sync/push/packer.rs — nothing else
```

- `PACK_TARGET` = 32 MiB and `PACK_MAX` = 48 MiB; `src/sync/pack.rs` unchanged.
- `SCHEMA` and `SCHEMA_VERSION` unchanged.
- `grep 'Utc::now\|SystemTime::now\|Instant::now'` over both files — **no hits.**
- No test in either file resolves a real path: every one builds `SyncRoots::at`
  and `Index::at` inside a `TempDir`, and every keyfile uses `m_kib = 8`.
- The packer's test `Client` is pointed at `127.0.0.1:1` and never dialled;
  nothing under this module makes a request.
- The REPO-03 guard and 4-01's write-path guards still pass.

## Self-Check: PASSED

- `src/sync/index.rs` — FOUND
- `src/sync/push/packer.rs` — FOUND
- `c1e4bbe`, `c094d61`, `a542471` — all FOUND in `git log`
