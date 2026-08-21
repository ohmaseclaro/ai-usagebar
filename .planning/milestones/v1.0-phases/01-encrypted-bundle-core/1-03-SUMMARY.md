---
phase: 01-encrypted-bundle-core
plan: 03
subsystem: pack
tags: [pack-format, sealed-header, keyed-addressing, content-addressing, bounds-checking, sharding]

# Dependency graph
requires: [1-01, 1-02]
provides:
  - "src/sync/pack.rs — the pack container: concatenated blob ciphertexts, a sealed trailing header, its keyed id in the clear, and a u32 LE header length"
  - "PackWriter (new/push/len_bytes/is_empty/finish) and read_header/blob_bytes/open_blob"
  - "shard_path — two-level content-addressed pack names"
  - "should_seal + PACK_TARGET/PACK_MAX — pure pack sizing for phase 2's plan builder"
affects: [1-04, 1-06, 1-07, 1-08, phase-2, phase-3, phase-4]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Sealed objects that a reader must locate before decrypting store their *keyed* id in the clear; a keyed id reveals nothing and substituting it breaks the tag"
    - "The DoS bound is the address arithmetic itself: checked_sub of a declared length, so an impossible header has no start offset and is refused before any allocation"
    - "A check no test can reach through the public API gets split into a pure function and tested directly, then negative-checked"

key-files:
  created: []
  modified:
    - src/sync/pack.rs

key-decisions:
  - "The pack header is sealed through chunk::seal_chunk (keys.chunk_id), never under crypto::content_address — a header is a list of chunk ids an attacker can already see, making it the most guessable object in the format, and an unkeyed address would be exactly the confirmation-of-plaintext oracle keyed ids exist to deny"
  - "The header's keyed id is stored in the clear in the trailer because read_header needs it as AAD before it can decrypt; without it the reader is not slower but impossible"
  - "content_address is used for one thing only: naming the finished pack, whose bytes are already public ciphertext"
  - "checked_sub on the declared header length is the T-03-05 mitigation — no separate size comparison, no allocation before the check"
  - "entries_within is a private pure function rather than an inline loop, because a header with out-of-range entries cannot be produced by PackWriter and the loop was otherwise unreachable from any test"
  - "finish_at_version is a private seam so a test can write a header from the future and prove the at-or-below ceiling refuses it while still accepting version 0"

patterns-established:
  - "Adversarial tests substitute *valid* values, not random ones: the trailer-id test writes a genuinely valid id of a genuinely sealed object from the same pack"
  - "Bounds mitigations are negative-checked before being trusted, following 1-01's containment gate"

requirements-completed: [CRYPTO-01, CRYPTO-05]

coverage:
  - id: D1
    description: "Many blobs write into one pack and every one reads back byte-exactly at its recorded offset and length"
    requirement: CRYPTO-05
    verification:
      - kind: unit
        ref: "cargo test --lib sync::pack — three_blobs_read_back_byte_exactly_from_their_recorded_offsets"
        status: pass
    human_judgment: false
  - id: D2
    description: "The trailer parses: last four bytes give a length, the 32 before give a keyed id, and together they locate a header that opens"
    requirement: CRYPTO-01
    verification:
      - kind: unit
        ref: "cargo test --lib sync::pack — the_trailer_locates_a_header_that_opens"
        status: pass
    human_judgment: false
  - id: D3
    description: "No blob plaintext appears anywhere in a finished pack"
    requirement: CRYPTO-01
    verification:
      - kind: unit
        ref: "cargo test --lib sync::pack — no_blob_plaintext_appears_anywhere_in_the_finished_pack"
        status: pass
    human_judgment: false
  - id: D4
    description: "A flipped header bit, a substituted trailer id, and the wrong Keys each fail to open rather than returning entries (T-03-01)"
    requirement: CRYPTO-05
    verification:
      - kind: unit
        ref: "cargo test --lib sync::pack — a_flipped_bit_in_the_header_ciphertext_fails_to_open, a_substituted_trailer_header_id_fails_to_open, the_wrong_keys_yield_an_error_rather_than_entries"
        status: pass
    human_judgment: false
  - id: D5
    description: "A pack truncated by one byte, and by a kilobyte, fails at read_header rather than yielding the blobs that survived"
    requirement: CRYPTO-05
    verification:
      - kind: unit
        ref: "cargo test --lib sync::pack — truncation_fails_at_read_header_with_no_entry_returned"
        status: pass
    human_judgment: false
  - id: D6
    description: "A crafted header length larger than the pack is rejected before allocation (T-03-05)"
    requirement: CRYPTO-05
    verification:
      - kind: unit
        ref: "cargo test --lib sync::pack — a_header_length_larger_than_the_pack_is_refused_before_any_allocation (negative-checked: a saturating_sub makes it fail)"
        status: pass
    human_judgment: false
  - id: D7
    description: "Every entry's offset + clen is bounds-checked against the blob region before any entry is returned (T-03-01)"
    requirement: CRYPTO-05
    verification:
      - kind: unit
        ref: "cargo test --lib sync::pack — an_entry_reaching_past_the_blob_region_is_refused (negative-checked), an_entry_pointing_past_the_blob_region_is_refused_before_it_is_returned"
        status: pass
    human_judgment: false
  - id: D8
    description: "A pack's name is a pure function of its own bytes, so a substituted pack cannot keep the name it is served under (T-03-02)"
    requirement: CRYPTO-01
    verification:
      - kind: unit
        ref: "cargo test --lib sync::pack — a_pack_name_is_a_function_of_the_pack_bytes, shard_path_shards_on_the_first_two_hex_characters"
        status: pass
    human_judgment: false
  - id: D9
    description: "Pack sizing is a pure decision, and a writer filled until it fires stays within PACK_MAX"
    requirement: CRYPTO-05
    verification:
      - kind: unit
        ref: "cargo test --lib sync::pack — should_seal_fires_exactly_at_the_ceiling, a_writer_filled_until_should_seal_fires_stays_within_pack_max"
        status: pass
    human_judgment: false
  - id: D10
    description: "A header format above MAX_SUPPORTED_PACK_HEADER is refused; at or below it is accepted (CRYPTO-02's at-or-below rule)"
    requirement: CRYPTO-01
    verification:
      - kind: unit
        ref: "cargo test --lib sync::pack — a_header_version_is_accepted_at_or_below_the_ceiling_and_refused_above"
        status: pass
    human_judgment: false
  - id: D11
    description: "A pack with zero blobs is rejected at finish rather than producing a headerless object"
    requirement: CRYPTO-05
    verification:
      - kind: unit
        ref: "cargo test --lib sync::pack — an_empty_writer_refuses_to_finish"
        status: pass
    human_judgment: false

# Metrics
duration: 25min
completed: 2026-08-19
status: complete
---

# Phase 1 Plan 03: The Pack Format Summary

**Blob ciphertexts concatenated into one object, a sealed header listing them, that header's
*keyed* id in the clear, and a `u32` little-endian length — restic's layout, simplified, with the
one change that makes it safe here: the header is addressed by a keyed hash, never by a content
address.**

## Performance

- **Duration:** ~25 min
- **Tasks:** 2/2
- **Files modified:** 1 (`src/sync/pack.rs`, stub → 566 lines)
- **Test suite:** 16 new tests, 1.8 s wall clock; 69 across `sync::` total

## The canonical pack layout

1-06 truncates a real pack against this and 1-08 documents it.

| Region | Size | Contents |
|---|---|---|
| blob region | sum of `clen` | `<blob0 ciphertext><blob1 ciphertext>…<blobN ciphertext>`, in push order |
| sealed header | `header_len` | `chunk::seal_chunk(keys, serde_json::to_vec(&PackHeader))` — framed, zstd'd, XChaCha20-Poly1305 sealed |
| header id | 32 | `keys.chunk_id(header_json)` — the **keyed** id, in the clear |
| header length | 4 | `header_len` as `u32` **little-endian**, the last four bytes of the file |

The trailer is therefore always the final 36 bytes, and the reader walks it backwards: length, then
id, then `id_at - header_len` gives the header's start. The blob region ends exactly where the
sealed header begins, which is the bound every entry is checked against.

```rust
pub struct PackEntry { pub id: ChunkId, pub offset: u64, pub clen: u32, pub true_len: u32 }
pub struct PackHeader { pub format: u32, pub entries: Vec<PackEntry> }
```

`offset` is from the start of the *pack*, not the start of the blob region — they are the same
position, and saying so avoids a fencepost in phase 2.

## Why the id is keyed, and why it is in the trailer

Both halves are in the module doc comment, because both are the kind of thing a later reader would
"simplify".

**Keyed.** A pack header is a list of chunk ids anyone holding the repository can already see, plus
offsets and lengths he can measure against the file in front of him. It is the single most guessable
object in the format. Sealing it under `crypto::content_address` — an unkeyed BLAKE3 — would let him
hash his guess and compare against the id sitting in the trailer, which is exactly the
confirmation-of-plaintext oracle that keyed chunk ids exist to deny. `crypto.rs` already carries the
rule "never seal anything under an unkeyed address"; the pack header is the object that would have
broken it first. `content_address` appears in this module for one purpose only: naming the finished
pack, whose bytes are already public ciphertext.

**In the trailer.** `read_header` needs the id *before* it can decrypt: the id derives the nonce and
is bound as associated data. Storing it in the clear costs nothing — it is a keyed hash, so an
attacker without `name_key` cannot recompute it from the header he is staring at, and substituting
some other id simply breaks the tag. The test proves that with a *valid* id rather than random
bytes: it writes the id of a genuinely sealed blob from the same pack into the trailer, and the open
still fails.

## Public surface of `src/sync/pack.rs`

```rust
pub const PACK_TARGET: usize = 32 * 1024 * 1024;   // CAL-1 fallback
pub const PACK_MAX: usize    = 48 * 1024 * 1024;

pub struct PackEntry { pub id: ChunkId, pub offset: u64, pub clen: u32, pub true_len: u32 }
pub struct PackHeader { pub format: u32, pub entries: Vec<PackEntry> }

pub struct PackWriter;
impl PackWriter {
    pub fn new() -> Self;
    pub fn push(&mut self, blob: Blob);
    pub fn len_bytes(&self) -> usize;       // blob ciphertext only, no trailer
    pub fn is_empty(&self) -> bool;
    pub fn finish(self, keys: &Keys) -> Result<(ChunkId, Vec<u8>)>;   // (content address, bytes)
}

pub fn read_header(keys: &Keys, pack: &[u8]) -> Result<PackHeader>;
pub fn blob_bytes<'a>(pack: &'a [u8], entry: &PackEntry) -> Result<&'a [u8]>;
pub fn open_blob(keys: &Keys, pack: &[u8], entry: &PackEntry) -> Result<Zeroizing<Vec<u8>>>;
pub fn shard_path(id: &ChunkId) -> String;      // packs/ab/ab….pack
pub fn should_seal(current_len: usize, next_blob_len: usize) -> bool;
```

Private: `ID_LEN = 32`, `LEN_LEN = 4`, `TRAILER_LEN = 36`, `PackWriter::finish_at_version`,
`entries_within`. Nothing here imports `argon2` or `chacha20poly1305`; 1-01's containment gate is
still green.

## Task Commits

1. **Tasks 1 + 2: the pack container, sharded names, and the sizing decision** — `5e6108a` (feat)
2. **Negative-checked bounds on the two forgeable numbers** — `6c4a2ce` (feat)

Tasks 1 and 2 touch the same file and were written in one pass; splitting them retroactively would
have produced an artificial intermediate state rather than a meaningful one, exactly as in 1-02. The
second commit is a genuine follow-up: it adds coverage that the first commit's code could not reach.

## Decisions Made

- **`checked_sub` *is* the T-03-05 mitigation.** A header length larger than the pack has no start
  offset, so the subtraction fails and the read is refused before a byte is allocated. No separate
  "is `header_len` plausible?" comparison exists, because it would be a second place to get the
  same bound wrong.
- **`entries_within` is a private pure function, not an inline loop.** A header whose entries point
  past the blob region cannot be produced by `PackWriter` — it can only be forged by whoever serves
  the pack — so the loop was unreachable from any test written against the public API. Splitting it
  out makes it directly testable, and it was then negative-checked (neutering the comparison turns
  the test red).
- **`finish_at_version` exists solely as a version seam.** `finish` writes `PACK_HEADER_VERSION`; the
  private variant lets the test seal a header at `MAX_SUPPORTED_PACK_HEADER + 1` and prove it is
  refused, *and* seal one at version 0 and prove it still opens. The second assertion is the one that
  goes red if anyone rewrites the ceiling as an equality check.
- **The header is sealed as a single chunk, and that is a documented ceiling.** `seal_chunk` refuses
  more than `CHUNK_SIZE`, which bounds a header at some thousands of entries — a 32 MiB pack of
  256 KiB chunks holds ~128, so this is slack rather than a limit. If pathologically small blobs ever
  crowd one pack, `finish` returns a clean error and the upgrade path is a format-2 header sealed as
  several chunks. The comment says so at the call site.
- **The `PACK_MAX` fill test clones one sealed blob rather than sealing 191.** The property under
  test is arithmetic between `len_bytes` and `should_seal`; 191 zstd passes over 256 KiB would buy
  nothing and would run on an installer's machine during the AUR `check()`.
- **`should_seal` uses `saturating_add`.** A caller passing a nonsense length gets `true` — seal now
  — rather than an overflow panic in a release build's wrapping arithmetic.

## Deviations from Plan

1. **Two commits rather than two task-shaped commits** — see above. Both tasks landed in the first;
   the second adds tests the first could not reach.
2. **`entries_within` was extracted from `read_header`.** The plan specified the check inline. The
   behaviour is identical; the extraction exists only so the check can be exercised, which the plan's
   own T-03-01 requires it to be.

Everything else — layout, trailer order, keyed sealing, the naming-only use of `content_address`,
the at-or-below version ceiling, `PACK_TARGET`/`PACK_MAX`, `shard_path`, `should_seal` — is as
written.

## Issues Encountered

One compile error: the test module's local `keys()` helper was shadowed by a `let keys = keys();`
binding in the wrong-keys test. Renamed the binding. No design impact.

## Verification

All commands run in the worktree at `.claude/worktrees/1-03`.

| Check | Result |
|---|---|
| `cargo test --lib sync::pack` | 16 passed, 0 failed, 1.8 s |
| `cargo test --lib sync::` | 69 passed, 0 failed — 1-01's containment gate still green |
| `env -u HOME -u XDG_CACHE_HOME cargo test --lib sync::` | 69 passed — hermetic |
| `cargo clippy --all-targets -- -D warnings` | clean |
| `cargo fmt -- --check` | clean |
| Negative check: `if end > blob_region` → `if false` | `an_entry_reaching_past_the_blob_region_is_refused` fails, as intended |
| Negative check: `checked_sub(header_len)` → saturating | `a_header_length_larger_than_the_pack_is_refused_before_any_allocation` fails, as intended |

No test in this module opens a file, reads an environment variable, or touches the clock — a pack is
a `Vec<u8>` in and a `Vec<u8>` out throughout, per D-01 (D1). Every test that derives keys uses the
cheap KDF parameters `{ m_kib: 8, t: 1, p: 1 }`. The incompressible fixture is the same deterministic
xorshift 1-02 uses.

## Known Stubs

None in this file. `src/sync/model.rs` remains a doc-only stub owned by 1-04 and was not touched.

## Threat Flags

None beyond the plan's `<threat_model>`. T-03-01 through T-03-05 are all mitigated and tested; the
two that could only be reached through a forged pack (T-03-01's entry bound, T-03-05's crafted
length) were additionally verified to go red when their mitigation is removed.

One thing 1-06 should know before it attacks this: **transposing two whole pack entries is not
detectable here**, for the same reason 1-02 recorded at the chunk layer. Each entry still names a
blob that decrypts cleanly and hashes to its own id; a pack carries no notion of which entry *should*
come first. Ordering integrity remains the manifest's invariant (1-04).

## User Setup Required

None — pure and offline.

## Next Phase Readiness

**Ready.** 1-04 gets `PackEntry`/`PackHeader` to reference from the manifest and `open_blob` to pull
a chunk out of a fetched pack. 1-06 gets a real pack to truncate, bit-flip, and re-name. 1-07 gets a
fixed layout to pin a round-trip vector against. 1-08 gets the layout table above to copy into
`docs/sync-format.md`, plus the `Range:` probe that decides whether `PACK_TARGET` can grow. Phase 2
gets `should_seal` as a pure sizing decision that needs no writer instance.

## Self-Check: PASSED

`src/sync/pack.rs` is 566 lines and exists on disk; commits `5e6108a` and `6c4a2ce` are in git; every
public item listed above resolves in the compiled crate.
