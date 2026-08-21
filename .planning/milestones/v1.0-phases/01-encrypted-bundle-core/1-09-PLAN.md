---
plan: 1-09
phase: 1
title: Multi-chunk manifest
gap_closure: true
depends_on: ["1-02", "1-04"]
wave: 4
autonomous: true
requirements: ["CRYPTO-01", "CRYPTO-05"]
files_modified:
  - src/sync/model.rs
---

# 1-09 — Multi-chunk manifest (gap closure)

## The gap, measured

`Manifest::seal` produces exactly **one** sealed chunk, and `chunk::frame` refuses any input
where `data.len() > CHUNK_SIZE` — a check on the **plaintext** length, so compression does not
rescue it.

Measured against the real machine this feature is being built for, using the actual default
bundle (chat session indexes + Desktop profiles + routines):

```
entries:                 1558
serialized manifest:     448 KiB
CHUNK_SIZE:              256 KiB
→ exceeds by:            192 KiB
(compresses to ~48 KiB, but the refusal is on plaintext length, so that is irrelevant)
```

**The default bundle cannot seal.** This is not the opt-in transcripts case — with transcripts
enabled the manifest is ~5700 entries and the problem is merely larger.

Plan `1-04` discovered the boundary, exercised it with a named refusal, and handed the split to
Phase 2 because its own plan was self-contradictory about whether Phase 1 needed it. That
deferral is wrong for one specific reason: **`Root`'s shape is on-disk format.** Changing
`manifest_id` to a list in Phase 2 would mean altering a format that Phase 1 declared complete
and that `1-07` is about to pin with regression vectors. This phase has already had three
permanent-format defects caught by adversarial review; shipping a fourth knowingly is worse
than the others, because we can see it now.

## The fix

`1-02` already provides `split` / `seal_all` / `reassemble`, built and tested for exactly this
shape. Reuse them rather than inventing a manifest-specific container.

### Task 1 — `Root.manifest_id: ChunkId` → `manifest_chunks: Vec<ChunkId>`

- `Manifest::seal` returns `Vec<Blob>` via `chunk::seal_all`, in order.
- `Manifest::open` takes the ordered `&[(ChunkId, Vec<u8>)]` and goes through
  `chunk::reassemble`, which already performs the per-chunk `chunk_id(plaintext) == id` recheck.
- Bump `MANIFEST_VERSION` / `ROOT_VERSION` write-versions. Readers keep the at-or-below ceiling
  rule — do **not** turn this into an equality check.

### Task 2 — Ordering integrity survives the change

This is the requirement `1-04` was told to own, and the fix must not weaken it.

The ordered `manifest_chunks` list lives inside `Root`'s sealed plaintext, so transposing two
ids requires re-sealing `Root` — which changes nothing an attacker can forge without the key.
Reassembling in the wrong order yields different bytes, and the per-chunk id recheck inside
`reassemble` fires first anyway.

Add a test that transposes two entries in `manifest_chunks` **after** `Root` is sealed (i.e.
serve the reordered list under the original root ciphertext) and assert it fails, returning
zero entries — the same shape `1-06` Attack 8 will use.

### Task 3 — Exercise the real size

Replace `1-04`'s "4000-file fixture must be refused" test with one that **seals and reopens
intact**. Assert:
- a ~1600-entry manifest (this machine's real default bundle) round-trips byte-identically;
- it occupies more than one chunk, so the test actually exercises the split rather than passing
  vacuously;
- a ~5700-entry manifest (default + transcripts) also round-trips.

Delete the now-obsolete single-chunk refusal test and its "hand the split to Phase 2" note in
the module doc; replace with a short statement of the real layout.

## Verify

```
cargo test --lib sync::model
cargo test --lib sync::
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

## Done when

- The default-bundle-sized manifest seals and reopens byte-identically.
- Transposing sealed `manifest_chunks` is detected and returns zero entries.
- No equality version check was introduced.
- `src/sync/pack.rs`, `chunk.rs`, `crypto.rs`, `passphrase.rs`, `anchor.rs` untouched.
