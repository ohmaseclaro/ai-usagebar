---
phase: 01-encrypted-bundle-core
plan: 09
subsystem: model
tags: [manifest, snapshot-root, index-object, multi-chunk, gap-closure, format-versioning, ordering-integrity]

# Dependency graph
requires: [1-02, 1-04]
provides:
  - "Manifest::seal → Vec<Blob> and Manifest::open(&[(ChunkId, Vec<u8>)]) — a manifest of any size seals and reopens"
  - "Root.manifest_chunks: Vec<ChunkId> — the ordered chunk list, inside the root's sealed plaintext"
  - "IndexObject sealed the same way, so it inherits the split rather than being the next caller to hit the ceiling"
  - "MANIFEST_VERSION / ROOT_VERSION = 2, with ceilings raised to match and the at-or-below rule intact"
affects: [1-06, 1-07, 1-08, phase-2, phase-4, phase-5]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Bundle-sized JSON objects seal through chunk::seal_all and open through chunk::reassemble — the 1-02 primitives, not a per-object container"
    - "The size guard was deleted rather than raised: there is no longer a length at which a sealed object is refused for being large"
    - "A test fixture whose per-entry size is itself asserted, so a size test cannot pass vacuously on unrealistically small synthetic data"

key-files:
  created: []
  modified:
    - src/sync/model.rs
    - src/sync/mod.rs

key-decisions:
  - "Fixed at the shared seal_object/open_object helpers rather than in Manifest, because IndexObject routes through the same pair and carries one entry per chunk in the bundle — it had the identical latent 256 KiB ceiling"
  - "Root.manifest_id becomes manifest_chunks: Vec<ChunkId>; this is on-disk format, which is why it lands in Phase 1 before 1-07 pins regression vectors rather than in Phase 2"
  - "MANIFEST_VERSION and ROOT_VERSION go to 2 together with MAX_SUPPORTED_MANIFEST / MAX_SUPPORTED_ROOT; check_version stays at-or-below, never equality (CRYPTO-02)"
  - "No v1-root compatibility deserializer: Phase 1 has not shipped, so no v1 bundle exists on any disk. Writing one would be speculative code guarding a case that cannot occur"
  - "Ordering integrity stays closed by construction. manifest_chunks lives inside the root's sealed plaintext, so a transposition needs the key; the reassembly failure is belt and braces on top of that, not the guarantee"
  - "The many_files fixture now uses realistic project-scoped session paths — the old short synthetic paths serialized at ~150 bytes/entry, so a 1,600-entry fixture stayed under one chunk and would have made the split tests pass vacuously"

patterns-established:
  - "No object in the format has a size at which it stops working; the only thing a length changes is how many chunks it occupies"

requirements-completed: [CRYPTO-01, CRYPTO-05]

coverage:
  - id: D1
    description: "The measured default bundle's manifest (~1,600 entries, past 256 KiB) seals across several chunks and reopens byte-identically"
    requirement: CRYPTO-01
    verification:
      - kind: unit
        ref: "cargo test --lib sync::model — the_default_bundles_manifest_spans_several_chunks_and_round_trips"
        status: pass
    human_judgment: false
  - id: D2
    description: "The default-plus-transcripts manifest (~5,700 entries) also round-trips, across more than two chunks"
    requirement: CRYPTO-01
    verification:
      - kind: unit
        ref: "cargo test --lib sync::model — a_fifty_seven_hundred_file_manifest_round_trips"
        status: pass
    human_judgment: false
  - id: D3
    description: "The split tests are non-vacuous: the fixture's per-entry size is asserted against the measured bundle's ~294 bytes, and a small manifest still seals to exactly one chunk"
    requirement: CRYPTO-01
    verification:
      - kind: unit
        ref: "cargo test --lib sync::model — the_many_files_fixture_matches_the_measured_bundles_bytes_per_entry, a_thousand_file_manifest_still_fits_in_one_chunk_and_round_trips, a_three_file_manifest_seals_and_reopens_identically"
        status: pass
    human_judgment: false
  - id: D4
    description: "Transposing two entries of manifest_chunks after the root is sealed is detected and returns zero entries — the reordered list cannot be served under the original root ciphertext, and a reader that followed one anyway gets an error rather than a manifest (the 1-06 Attack 8 shape)"
    requirement: CRYPTO-05
    verification:
      - kind: unit
        ref: "cargo test --lib sync::model — transposing_manifest_chunks_after_the_root_is_sealed_yields_zero_entries"
        status: pass
    human_judgment: false
  - id: D5
    description: "1-04's ordering-integrity guarantee for a file's own chunk list survives the change: a re-sealed reordered manifest has a different id, its bytes served under the honest id fail, and an in-place edit fails too"
    requirement: CRYPTO-05
    verification:
      - kind: unit
        ref: "cargo test --lib sync::model — transposing_two_chunk_ids_inside_a_sealed_manifest_yields_zero_entries"
        status: pass
    human_judgment: false
  - id: D6
    description: "A truncated chunk list is refused rather than read as a shorter manifest; an empty list is refused too"
    requirement: CRYPTO-05
    verification:
      - kind: unit
        ref: "cargo test --lib sync::model — dropping_the_last_manifest_chunk_is_refused_rather_than_read_short"
        status: pass
    human_judgment: false
  - id: D7
    description: "Version acceptance is still at-or-below a ceiling in both directions: a manifest stamped one version below today's opens, and today's opens on a build whose ceiling is higher"
    requirement: CRYPTO-02
    verification:
      - kind: unit
        ref: "cargo test --lib sync::model — a_manifest_below_the_ceiling_opens_and_the_ceiling_may_be_raised, a_manifest_one_version_above_the_ceiling_is_refused, a_root_below_the_ceiling_opens_and_one_above_it_is_refused"
        status: pass
    human_judgment: false
  - id: D8
    description: "No sealed manifest chunk carries a file path in the clear — checked across every chunk of a multi-chunk manifest, not only the first"
    requirement: CRYPTO-05
    verification:
      - kind: unit
        ref: "cargo test --lib sync::model — no_sealed_manifest_chunk_carries_a_file_path_in_the_clear"
        status: pass
    human_judgment: false
  - id: D9
    description: "A bundle-sized index object spans several chunks and round-trips, so the sibling caller of the shared helpers is fixed too"
    requirement: CRYPTO-01
    verification:
      - kind: unit
        ref: "cargo test --lib sync::model — a_bundle_sized_index_object_spans_several_chunks_and_round_trips"
        status: pass
    human_judgment: false

# Metrics
duration: 35min
completed: 2026-08-19
status: complete
---

# Phase 1 Plan 09: Multi-Chunk Manifest (gap closure) Summary

**The default bundle could not seal at all. `Manifest::seal` produced one chunk and `chunk::frame`
refuses input past `CHUNK_SIZE` on its *plaintext* length; the measured default bundle is 1,558
entries / 448 KiB against a 256 KiB limit. `Root.manifest_id` is now `manifest_chunks:
Vec<ChunkId>`, sealing goes through 1-02's `seal_all` / `reassemble`, and the size guard is gone
rather than raised.**

## Performance

- **Duration:** ~35 min
- **Tasks:** 3/3
- **Files modified:** 2 (`src/sync/model.rs` 696 → 860 lines; `src/sync/mod.rs`, four constants)
- **Commit:** `6aa621e`

## What changed

### The fix landed at the shared helper, not in `Manifest`

`Manifest::seal` and `IndexObject::seal` both routed through one `seal_object`, and `open` through
one `open_object`. The single-chunk assumption and its `json.len() > CHUNK_SIZE` refusal lived in
that pair, so both objects had it. Patching only the manifest would have left `IndexObject` — one
entry per chunk in the whole bundle, so *larger* than the manifest at scale — as the next caller to
hit the same wall, in a later phase, after 1-07 had pinned the format.

```rust
fn seal_object<T: Serialize>(keys: &Keys, value: &T, object: &str) -> Result<Vec<Blob>>
fn open_object<T: DeserializeOwned>(
    keys: &Keys, chunks: &[(ChunkId, Vec<u8>)], ceiling: u32, object: &str,
) -> Result<T>
```

The bodies are `seal_all(keys, &json)` and `reassemble(keys, chunks)?` followed by the unchanged
version probe and deserialization. Nothing manifest-specific was invented: `reassemble` already
performs the per-chunk `chunk_id(plaintext) == id` recheck and already returns no partial buffer on
failure, which is exactly the contract a truncated or reordered chunk list needs.

The guard was **deleted, not raised**. There is no longer a length at which a sealed object is
refused for being large.

### Public API

```rust
impl Manifest {
    pub fn seal(&self, keys: &Keys) -> Result<Vec<Blob>>;                       // was (ChunkId, Vec<u8>)
    pub fn open(keys: &Keys, chunks: &[(ChunkId, Vec<u8>)]) -> Result<Manifest>; // was (&ChunkId, &[u8])
}
impl IndexObject { /* the same two shapes */ }

pub struct Root {
    pub format: u32, pub counter: u64, pub created_at: DateTime<Utc>, pub repo_id: String,
    pub manifest_chunks: Vec<ChunkId>,   // was manifest_id: ChunkId
    pub chunker: String, pub kdf: KdfParams,
}
impl Root {
    pub fn new(counter: u64, now: DateTime<Utc>, repo_id: String,
               manifest_chunks: Vec<ChunkId>, kdf: KdfParams) -> Self;
}
```

`Blob` (`{ id, ciphertext, true_len }`) is 1-02's type, reused rather than re-declared — the packer
already consumes it, and `true_len` travels with each chunk for free.

### Versioning

`MANIFEST_VERSION` and `ROOT_VERSION` are 2, and `MAX_SUPPORTED_MANIFEST` / `MAX_SUPPORTED_ROOT`
moved with them — a write version above its own ceiling would make this build unable to read what it
just wrote. `check_version` is untouched: `found <= ceiling`, never equality. The ceiling test now
asserts **both** directions, so an equality regression fails it either way:

- a manifest stamped `MAX_SUPPORTED_MANIFEST - 1` (a genuine v1 stamp) opens today;
- today's manifest opens under `MAX_SUPPORTED_MANIFEST + 1`, the future build's ceiling.

There is deliberately **no** v1-root compatibility deserializer. Phase 1 has not shipped, so no v1
root exists on any disk; a `#[serde(alias)]` or an untagged enum would be speculative code guarding a
case that cannot occur, and would itself become format surface 1-07 has to pin.

## Ordering integrity (the invariant that must not weaken)

`manifest_chunks` is ordered and lives **inside the root's sealed plaintext**. Transposing two ids
therefore requires re-sealing the root, which requires the key. That is the guarantee; the test
asserts it directly rather than asserting a downstream symptom:

```rust
// Served under the original root ciphertext, the order comes back exactly as written.
assert_eq!(Root::open(&keys, &framed).expect("open").manifest_chunks, ordered);

// And a reader that did follow a reordered list gets an error, not a manifest.
let mut reordered = served(&blobs);
reordered.swap(0, 1);
let err = Manifest::open(&keys, &reordered).expect_err("must refuse");
assert!(err.to_string().contains("manifest is malformed"));
assert!(!err.to_string().contains(".claude/projects"));   // and no path in the error
```

The second leg is belt and braces, and worth being precise about for 1-06: a whole-pair transposition
survives the chunk layer (1-02 Deviation 2 proved it — each pair still decrypts and still hashes to
its own id), so what catches it there is the reassembled buffer failing to parse. That is a
consequence, not the guarantee. The guarantee is the sealed root. 1-04's own ordering test — for the
chunk list *inside* a `FileEntry` — is kept and still asserts all three legs (`evil_id != honest_id`,
tampered bytes under the honest id, in-place bit flip).

`Result` carries no `Manifest`, so "returns zero entries" is structural rather than a length check.

## Deviations from the plan

1. **`IndexObject` was fixed too.** The plan scoped Task 1 to the manifest. Both objects share the
   helper that carried the bug, so the smaller and more correct diff fixes it once at the shared
   function; guarding only the path the gap named would have left the sibling caller broken. Covered
   by `a_bundle_sized_index_object_spans_several_chunks_and_round_trips`.

2. **The `many_files` fixture was rewritten before the size tests could mean anything.** Its old
   synthetic paths (`chat-sessions/2026-08-19/session-000123.jsonl`) serialize at roughly 150
   bytes/entry, against the real bundle's ~294 (448 KiB / 1,558) — so a 1,600-entry fixture came in
   *under* 256 KiB and both new split tests failed their own non-vacuity assertions on the first run.
   The fixture now uses project-scoped session paths at 229 bytes/entry, and
   `the_many_files_fixture_matches_the_measured_bundles_bytes_per_entry` pins bytes-per-entry into
   150..400 so a future edit to the fixture cannot quietly make the split tests vacuous again.

   Measured, at 229 bytes/entry: 1,000 entries → 224 KiB → 1 chunk; 1,600 → 358 KiB → 2 chunks;
   5,700 → 1.25 MiB → 5 chunks.

3. **Two extra tests beyond the plan's three assertions:** the truncated/empty chunk list (D6) and
   the path-leak check extended across *every* chunk rather than only the first (D8) — the old test
   only ever saw one chunk, so multi-chunk sealing would have been unchecked for leaks.

## Test Approach

`cargo test --lib sync::model` — 24 tests, 0.21 s. `cargo test --lib sync::` — 93 tests, 0.89 s.

Hermetic: no `$HOME`, no `$XDG`, no Keychain, no network, no clock (`created_at` is the fixed literal
`2026-08-19T12:00:00Z`). Every key derivation uses the cheap KDF seam `{ m_kib: 8, t: 1, p: 1 }`;
`KdfParams::default()` appears only as *data* inside a `Root` fixture, never to derive. The largest
fixture (5,700 entries, ~1.25 MiB of JSON, zstd across five chunks) costs milliseconds, so the AUR
`check()` budget is unaffected.

`cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` are clean. Per the plan's scope,
only `sync::` was built and tested — no full workspace build, no `make test`.

## Threat Flags

None new. The reordering threat 1-04 owned (T-04-04) is mitigated one level further up, at
`Root.manifest_chunks`, by the same construction.

Worth flagging for 1-06 and Phase 2, though not a defect: `manifest_chunks` is unbounded in length.
It is inside authenticated plaintext, so it is not attacker-controlled — a hostile remote cannot
lengthen it without the key — which is why no bound was added. If Phase 2 ever grows a path where an
id list is read before its container authenticates, that path needs its own bound.

## User Setup Required

None — pure and offline.

## Next Phase Readiness

**Ready.**

- **1-06** Attack 8 has both shapes now: the file-level list inside a sealed manifest, and the
  manifest-level list inside a sealed root. The essential detail is unchanged from 1-04 — re-sealing
  a reordered list yields a *different* `ChunkId`, so a genuine attack must serve tampered ciphertext
  under the id the container names.
- **1-07** pins the format as it now stands: `manifest_chunks` is a list, `MANIFEST_VERSION` and
  `ROOT_VERSION` are 2. A manifest vector is reproducible (deterministic nonce); a root vector must
  still be decrypt-side, because the root nonce is random.
- **1-08** should document the chain as `root → manifest_chunks → manifest → chunk ids → chunks`.
- **Phase 2** no longer inherits a format decision here: it builds real manifests of any size against
  a settled `Root`.

## Self-Check: PASSED

`src/sync/model.rs` is 860 lines on disk; commit `6aa621e` is in git on `gsd/1-09`; every public item
listed above resolves in the compiled crate; `src/sync/{pack,chunk,crypto,passphrase,anchor}.rs` are
untouched (`git diff HEAD~1 --stat` lists `src/sync/mod.rs` and `src/sync/model.rs` only).
