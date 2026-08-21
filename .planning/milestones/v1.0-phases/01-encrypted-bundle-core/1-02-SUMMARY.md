---
phase: 01-encrypted-bundle-core
plan: 02
subsystem: chunk
tags: [chunking, zstd, framing, padding, dedup, plaintext-addressing]

# Dependency graph
requires: [1-01]
provides:
  - "src/sync/chunk.rs — the frame layout, the zstd stage, seal/open of a single chunk, and the identity recheck"
  - "Fixed 256 KiB offset-aligned split plus an explicit tail, and whole-buffer seal_all/reassemble"
  - "sealed_chunk_count — the full-chunk count Phase 2's append fast path needs without the data"
affects: [1-03, 1-04, 1-07, 1-08, phase-2]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Ids address raw plaintext; ciphertext may legitimately diverge across zstd versions for one id"
    - "Power-of-two padding applied *after* compression, so a tail's sealed size reveals only a bucket"
    - "Length fields inside a decrypted frame are range-checked before any slice or allocation"
    - "Bounded decompression: zstd::bulk::decompress into an exactly-true_len buffer, not decode_all"

key-files:
  created: []
  modified:
    - src/sync/chunk.rs

key-decisions:
  - "The chunk id is keys.chunk_id(raw_plaintext), computed before framing/compression — hashing the frame would tie every id to the zstd version and re-id every chunk in every bundle on a crate bump"
  - "The chunk_id(plaintext) == id recheck lives in open_chunk, after unframe; crypto::Keys::open deliberately does not do it"
  - "Padding target is next_power_of_two(8 + comp_len).min(CHUNK_SIZE).max(8 + comp_len) — the .max is load-bearing, not defensive: incompressible data pushes the frame past CHUNK_SIZE and a bare cap would truncate it"
  - "unframe decompresses via zstd::bulk::decompress with true_len as the capacity rather than zstd::stream::decode_all, so the T-02-01 bound is enforced before the allocation instead of after"
  - "Chunk ordering is the manifest's responsibility, not chunk.rs's — a chunk carries no position, so reassemble catches wrong *contents* but cannot catch a wholly reordered chunk list"

patterns-established:
  - "Every buffer holding plaintext or compressed plaintext is Zeroizing, including the frame accumulator inside reassemble, so an aborted reassembly wipes what it had collected"
  - "Deterministic xorshift test fixtures — incompressible bytes with no random source and no clock"

requirements-completed: [CRYPTO-01, CRYPTO-05]

coverage:
  - id: D1
    description: "A multi-hundred-KiB fixture round-trips byte-exactly through split, frame, zstd, seal, open, unframe, reassemble"
    requirement: CRYPTO-05
    verification:
      - kind: unit
        ref: "cargo test --lib sync::chunk — a_seven_hundred_kib_fixture_reassembles_byte_exactly, a_full_chunk_round_trips_byte_exactly, a_hundred_bytes_round_trip_byte_exactly, an_empty_chunk_round_trips_to_an_empty_chunk"
        status: pass
    human_judgment: false
  - id: D2
    description: "Chunk ids are a keyed function of raw plaintext, asserted directly and independent of the compression stage"
    requirement: CRYPTO-01
    verification:
      - kind: unit
        ref: "cargo test --lib sync::chunk — the_id_is_a_function_of_the_raw_plaintext_and_of_nothing_downstream"
        status: pass
    human_judgment: false
  - id: D3
    description: "An append leaves every previously sealed chunk id unchanged, asserted over the id lists"
    requirement: CRYPTO-05
    verification:
      - kind: unit
        ref: "cargo test --lib sync::chunk — appending_leaves_every_previously_sealed_chunk_id_unchanged, a_full_multiple_splits_without_a_tail_and_one_more_byte_adds_one"
        status: pass
    human_judgment: false
  - id: D4
    description: "A swapped or transposed chunk aborts reassembly with zero bytes returned"
    requirement: CRYPTO-05
    verification:
      - kind: unit
        ref: "cargo test --lib sync::chunk — a_swapped_ciphertext_aborts_reassembly_with_no_bytes_returned, transposed_ciphertexts_abort_reassembly_rather_than_reordering_it, a_plaintext_that_does_not_hash_to_the_supplied_id_is_refused"
        status: pass
    human_judgment: false
  - id: D5
    description: "A crafted frame errors instead of panicking or over-allocating (T-02-01)"
    requirement: CRYPTO-05
    verification:
      - kind: unit
        ref: "cargo test --lib sync::chunk — a_frame_declaring_more_compressed_bytes_than_it_carries_errors"
        status: pass
    human_judgment: false
  - id: D6
    description: "A tail's sealed size reveals only a power of two, not its exact length (T-02-02)"
    requirement: CRYPTO-05
    verification:
      - kind: unit
        ref: "cargo test --lib sync::chunk — a_tail_seals_to_a_power_of_two_and_not_to_its_own_length, compressible_input_seals_smaller_than_incompressible_input_of_one_length"
        status: pass
    human_judgment: false
  - id: D7
    description: "Sealing is deterministic within a build, so re-syncing unchanged data creates no new objects"
    requirement: CRYPTO-01
    verification:
      - kind: unit
        ref: "cargo test --lib sync::chunk — two_seals_of_one_input_are_byte_identical"
        status: pass
    human_judgment: false

# Metrics
duration: 20min
completed: 2026-08-19
status: complete
---

# Phase 1 Plan 02: The Chunker Summary

**Fixed 256 KiB offset-aligned chunks plus an explicit tail, each addressed by a keyed hash of its
raw plaintext, framed with its true length, compressed with zstd, padded to a power of two to hide
its exact size, and sealed through `crypto.rs`.**

## Performance

- **Duration:** ~20 min
- **Tasks:** 2/2
- **Files modified:** 1 (`src/sync/chunk.rs`, stub → 465 lines)
- **Test suite:** 15 new tests, 0.34 s wall clock; 33 across `sync::` total

## Accomplishments

- **The id addresses the raw plaintext, and it is proven, not merely intended.**
  `the_id_is_a_function_of_the_raw_plaintext_and_of_nothing_downstream` asserts both directions:
  `blob.id == keys.chunk_id(data)` *and* `blob.id != keys.chunk_id(&frame(data))`. The second
  assertion is the one that goes red if someone ever "optimises" the id onto the compressed frame.
  The reason lives in the module doc under its own heading.
- **The identity recheck landed where 1-01 said it would.** `open_chunk` opens through `Keys::open`,
  unframes, and only then compares `chunk_id(&plaintext)` to the supplied id. The test seals *my*
  frame under *their* id, asserts the raw AEAD open succeeds, and then asserts `open_chunk` refuses
  it — so the recheck is demonstrably doing work the tag does not.
- **Appends preserve ids, asserted over the real lists.** 700 KiB → 3 chunks; +200 KiB → 4 chunks
  with `old[..2] == new[..2]` and `old[2] != new[2]`. That is the no-CDC decision holding.
- **Tail sizes are bucketed, not exact.** Two tails 100 bytes apart seal to *identical* ciphertext
  lengths, and that length minus the Poly1305 tag is a power of two.

## Frame layout (canonical — 1-07 pins a vector against this, 1-08 documents it)

The bytes handed to `Keys::seal`:

| Offset | Size | Field |
|--------|------|-------|
| 0 | 4 | `true_len`, u32 **little-endian** — the uncompressed byte count |
| 4 | 4 | `comp_len`, u32 **little-endian** — the exact length of the zstd frame that follows |
| 8 | `comp_len` | the zstd **level-3** frame |
| 8 + `comp_len` | rest | zero padding |

Total frame length is:

```
let body   = 8 + comp_len;
let target = body.next_power_of_two().min(CHUNK_SIZE).max(body);
```

The `.max(body)` is load-bearing rather than defensive. zstd grows incompressible input slightly, so
a full 256 KiB chunk of noise yields `body > CHUNK_SIZE`; capping alone would truncate the frame.
The rule in words: *pad up to the next power of two, but never past `CHUNK_SIZE`, and never below the
frame you actually have.*

Sealed size is therefore `target + 16` (the Poly1305 tag).

## Public surface of `src/sync/chunk.rs`

```rust
pub fn frame(data: &[u8]) -> Result<Zeroizing<Vec<u8>>>;
pub fn unframe(frame: &[u8]) -> Result<Zeroizing<Vec<u8>>>;

#[derive(Debug)]
pub struct Blob {
    pub id: ChunkId,
    pub ciphertext: Vec<u8>,
    pub true_len: u32,
}

pub fn seal_chunk(keys: &Keys, data: &[u8]) -> Result<Blob>;
pub fn open_chunk(keys: &Keys, id: &ChunkId, ciphertext: &[u8]) -> Result<Zeroizing<Vec<u8>>>;

pub fn split(data: &[u8]) -> impl Iterator<Item = &[u8]>;
pub fn seal_all(keys: &Keys, data: &[u8]) -> Result<Vec<Blob>>;
pub fn reassemble(keys: &Keys, chunks: &[(ChunkId, Vec<u8>)]) -> Result<Zeroizing<Vec<u8>>>;
pub fn sealed_chunk_count(len: u64) -> u64;
```

Private constants: `ZSTD_LEVEL: i32 = 3`, `HEADER_LEN: usize = 8`.

`Blob` derives `Debug` and may keep deriving it — an id is an address, and ciphertext is by
definition safe to print. Nothing in this module imports `argon2` or `chacha20poly1305`; 1-01's
containment gate still passes.

## Task Commits

1. **Tasks 1 + 2: the chunker** — `c7da7f7` (feat)

Both tasks touch the same single file and were written in one pass; splitting the commit
retroactively would have produced an artificial intermediate state rather than a meaningful one.

## Decisions Made

- **`unframe` decompresses through `zstd::bulk::decompress(frame, true_len)`, not
  `zstd::stream::decode_all`.** The plan named `decode_all`, but the plan's own T-02-01 requires the
  bound to be checked *before* the allocation — and `decode_all` grows its output buffer without a
  ceiling, so a crafted frame is bounded only after the fact. `bulk::decompress` allocates exactly
  `true_len` (itself checked against `CHUNK_SIZE` first) and errors if the frame expands past it. The
  encode side uses `zstd::stream::encode_all` at level 3 exactly as specified. Same on-disk bytes,
  strictly tighter bound.
- **`checked_add` for `HEADER_LEN + comp_len`.** `comp_len` is a full `u32` and `usize` is 32 bits on
  some targets, where a plain `+` would wrap and turn a bounds check into a pass.
- **`true_len > CHUNK_SIZE` is refused up front.** It is what makes the decompression bound a
  constant rather than an attacker-chosen number.
- **The 8-byte header is not itself padded to a power of two.** Padding covers the whole frame
  (`8 + comp_len` rounded up), so the header's size is already inside the bucket; a separately
  aligned header would only shift every offset without hiding anything more.
- **`split` returns `impl Iterator<Item = &[u8]>` with no explicit `+ '_`.** Edition 2024 captures
  in-scope lifetimes in RPIT by default, so the annotation the plan sketched is redundant; the
  signature is otherwise exactly as planned. `data.chunks(CHUNK_SIZE)` *is* the implementation —
  offset-aligned full chunks plus a shorter tail is precisely `slice::chunks`' contract.

## Deviations from Plan

1. **`zstd::bulk::decompress` in place of `decode_all`** — see Decisions above. This is a
   tightening of the plan's own threat mitigation, not a change to the format.
2. **"Two chunks' entries transposed" is tested as transposed *ciphertexts*, not transposed
   entries.** Swapping two whole `(id, ciphertext)` pairs cannot error at this layer and never
   could: each pair still decrypts correctly and hashes to its own id, so the only observable is a
   reordered buffer. A chunk carries no position — **ordering is the manifest's invariant (1-04),
   not the chunker's**. The test therefore swaps the ciphertexts *between* two entries, leaving the
   ids in place, which is the strongest thing this layer can detect: both entries fail, and the
   assertion is on the error rather than on divergent output. `reassemble`'s doc comment states the
   boundary explicitly so 1-04 knows it inherits ordering integrity.
3. **One commit rather than two task commits** — same file, one pass.

## Issues Encountered

None. First compile, first test run, both green.

## Verification

All commands run in the worktree at `.claude/worktrees/1-02`.

| Check | Result |
|---|---|
| `cargo test --lib sync::chunk` | 15 passed, 0 failed, 0.34 s |
| `cargo test --lib sync::` | 33 passed, 0 failed — 1-01's containment gate still green |
| `env -u HOME -u XDG_CACHE_HOME cargo test --lib sync::chunk` | 15 passed — hermetic |
| `cargo clippy --all-targets -- -D warnings` | clean |
| `cargo fmt -- --check` | clean |

No test in this module opens a file, reads an environment variable, or touches the clock. Every test
that derives keys uses the cheap KDF parameters `{ m_kib: 8, t: 1, p: 1 }`. Incompressible fixtures
come from a deterministic in-test xorshift, not a random source.

`zstd` now has a real consumer, so the `cargo machete` flag noted in 1-01's summary is resolved
(`cargo-machete` is not installed on this machine; the release gate will confirm).

## Known Stubs

None in this file. `src/sync/{pack,model,passphrase,anchor}.rs` remain doc-only stubs owned by
1-03 … 1-05, untouched by this plan.

## Threat Flags

None beyond the plan's `<threat_model>`. T-02-01 and T-02-02 are mitigated and tested; T-02-03 is
mitigated for chunk *substitution* here, with whole-list *reordering* explicitly delegated to the
manifest layer (see Deviation 2) — 1-04 should treat ordered-chunk-list integrity as its own
requirement rather than assuming it inherited it.

## User Setup Required

None — pure and offline.

## Next Phase Readiness

**Ready.** 1-03 (`pack.rs`) has `Blob { id, ciphertext, true_len }` to pack and `content_address` to
name the pack with; 1-04 (`model.rs`) has `seal_all`/`reassemble` and the ordering boundary named
above; 1-07 has the exact frame layout to pin a vector against; 1-08 has the layout table to copy
into `docs/sync-format.md`.

## Self-Check: PASSED

`src/sync/chunk.rs` is 465 lines and exists on disk; commit `c7da7f7` is in git; every public item
listed above resolves in the compiled crate.
