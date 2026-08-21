---
phase: 01-encrypted-bundle-core
plan: 02
type: execute
wave: 2
depends_on: [1-01]
files_modified:
  - src/sync/chunk.rs
autonomous: true
requirements: [CRYPTO-01, CRYPTO-05]
must_haves:
  truths:
    - "A multi-megabyte buffer splits into 256 KiB chunks plus a tail and reassembles byte-exactly."
    - "A chunk id is a keyed hash of the raw plaintext, so it survives a zstd upgrade unchanged."
    - "Appending to a buffer leaves every previously sealed chunk id unchanged."
    - "A tail chunk's sealed size reveals only a power of two, not its exact length."
  artifacts:
    - src/sync/chunk.rs with the frame layout, the zstd stage, the seal/reassemble pipeline, and the identity recheck
  key_links:
    - "chunk.rs calls Keys::chunk_id and Keys::seal — it never constructs a cipher itself"
    - "open_chunk performs the chunk_id(plaintext) == id recheck after unframing; crypto.rs deliberately does not"
    - "pack.rs and model.rs both build on this module, which is why they land in the wave after it"
---

<objective>
The chunker: fixed 256 KiB offset-aligned chunks plus an explicit tail, each addressed by a keyed
hash of its raw plaintext, framed with its true length, compressed with zstd, padded to hide its
exact size, and sealed through `crypto.rs`.

Fixed-size chunking is a decision proven in `research/chunking-storage.md` §1.2, not a default: an
append displaces no byte below the old length, so every fully contained chunk keeps its id, and
content-defined chunking would buy nothing while leaking a plaintext fingerprint through chunk-size
boundaries (arXiv:2504.02095). Do not reopen it.

Implements **D-01 (D1)** pure and offline — this module takes `&[u8]`, never a `Path`.

Purpose: this is where dedup is either earned or lost.
Output: `src/sync/chunk.rs`.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/01-encrypted-bundle-core/1-CONTEXT.md
@.planning/research/chunking-storage.md
@.planning/research/encryption.md
@.planning/phases/01-encrypted-bundle-core/1-01-SUMMARY.md
@src/sync/crypto.rs
@src/sync/mod.rs
@CLAUDE.md
</context>

<tasks>

<task type="auto" tdd="true">
  <name>Task 1: The chunk frame — keyed id over plaintext, length prefix, zstd, padding, seal</name>
  <files>src/sync/chunk.rs</files>
  <behavior>
    - A 100-byte input frames, seals, opens, and unframes back to the identical 100 bytes.
    - A full 256 KiB input round-trips byte-exactly.
    - An empty input round-trips to an empty output rather than erroring.
    - The chunk id for a given plaintext equals `keys.chunk_id(plaintext)` computed directly — the id is a function of the plaintext alone and of nothing downstream of it.
    - Highly compressible input produces a sealed size strictly smaller than incompressible input of the same length.
    - Two seals of the same input under one build produce byte-identical ciphertext and the same id.
    - `open_chunk` given a ciphertext whose unframed plaintext does not hash to the supplied id errors, even though the AEAD tag verified.
    - A frame whose declared `comp_len` exceeds the buffer errors instead of panicking.
  </behavior>
  <action>
Implement the single-chunk pipeline in `src/sync/chunk.rs`.

**The id is a keyed hash of the raw plaintext.** `id = keys.chunk_id(plaintext)`, exactly as
`research/encryption.md` §3 specifies, computed before any framing or compression touches the bytes.
Write the reason into the module doc comment, because it is the kind of thing a later optimisation
would happily undo: hashing the compressed frame instead would tie every chunk id in every user's
bundle to the zstd version, so a routine dependency bump would re-id everything, force a full
re-upload, and drop dedup to zero across the upgrade. Hashing the plaintext means two machines on
different zstd versions may produce different ciphertext for one id — which is harmless, since both
decrypt to identical plaintext and the first upload simply wins.

Define the frame — the bytes handed to `Keys::seal` — as, in order: `u32` little-endian `true_len`
(the uncompressed byte count), `u32` little-endian `comp_len` (the exact length of the zstd frame
that follows), the zstd level-3 frame itself, then zero padding up to the next power of two of
`8 + comp_len`, capped at `CHUNK_SIZE`. Record this layout in the module doc comment; 1-08 copies it
into `docs/sync-format.md`.

Three properties make that layout the right one and each is worth a comment stating why. Explicit
`comp_len` means the padding is unambiguous, so decoding never has to guess where the zstd frame
ends. Padding *after* compression is what actually hides the tail length — padding first would let
zstd collapse the zeros and hand the exact plaintext size straight back through the ciphertext
length. And every step is deterministic within a build, so the same input yields the same frame and
the same ciphertext.

`pub fn frame(data: &[u8]) -> Result<Zeroizing<Vec<u8>>>` and
`pub fn unframe(frame: &[u8]) -> Result<Zeroizing<Vec<u8>>>`. `unframe` validates that `8 + comp_len`
fits inside the buffer before slicing, and that the decompressed length equals `true_len`, before
returning anything. Use `zstd::stream::encode_all` and `decode_all` at level 3.

`pub struct Blob { pub id: ChunkId, pub ciphertext: Vec<u8>, pub true_len: u32 }` — derive `Debug`;
it holds no key material and no plaintext.

`pub fn seal_chunk(keys: &Keys, data: &[u8]) -> Result<Blob>` takes `keys.chunk_id(data)` as the id,
frames the data, and seals the frame under that id.

`pub fn open_chunk(keys: &Keys, id: &ChunkId, ciphertext: &[u8]) -> Result<Zeroizing<Vec<u8>>>`
opens through `crypto.rs`, unframes, and **then** rechecks that `keys.chunk_id(&plaintext) == *id`,
returning an error if not. `crypto::Keys::open` deliberately does not do this — it sees only the
framed form, and the id addresses the plaintext — so this is the layer that owns the check. It is
belt and braces on top of the AEAD tag: it catches our own framing bugs as well as an adversary.
Return the plaintext inside `Zeroizing`; it is a fragment of a user's credential file.

Import nothing from `argon2` or `chacha20poly1305`; every cryptographic operation goes through
`crypto.rs`, and 1-01 ships a test that enforces it.

Write the `<behavior>` assertions as an inline `#[cfg(test)] mod tests` using the cheap KDF
parameters `{ m_kib: 8, t: 1, p: 1 }`.
  </action>
  <verify>
    <automated>cargo test --lib sync::chunk</automated>
  </verify>
  <done>Every `<behavior>` line passes. The id is proven to be a function of the plaintext alone, and the identity recheck lives in `open_chunk`.</done>
</task>

<task type="auto" tdd="true">
  <name>Task 2: Fixed 256 KiB split, tail, and reassembly</name>
  <files>src/sync/chunk.rs</files>
  <behavior>
    - A 1 MiB input yields exactly 4 chunks and no tail; a 1 MiB + 1 byte input yields 5, the last being a 1-byte tail.
    - A 700 KiB fixture seals and reassembles byte-exactly.
    - Appending 200 KiB to a 700 KiB fixture leaves the first 2 sealed chunk ids identical and changes only the tail plus any newly sealed chunk.
    - Reassembly with one chunk's ciphertext replaced by another chunk's errors and returns zero bytes — it does not return short or wrong output.
    - Reassembly with two chunks' entries transposed errors rather than producing a reordered buffer.
  </behavior>
  <action>
Add the file-level pipeline.

`pub fn split(data: &[u8]) -> impl Iterator<Item = &[u8]> + '_` yielding `CHUNK_SIZE` slices from
offset zero plus a final shorter tail when the length is not a multiple. Offset-aligned from the
start of each file, never across a concatenation of files — a change to one small file must not
re-chunk anything else.

`pub fn seal_all(keys: &Keys, data: &[u8]) -> Result<Vec<Blob>>` sealing each slice in order, and
`pub fn reassemble(keys: &Keys, chunks: &[(ChunkId, Vec<u8>)]) -> Result<Zeroizing<Vec<u8>>>` opening
each in the given order and concatenating. Reassembly must fail as a whole on any chunk failure and
must return no partial buffer — a half-decrypted credential file is worse than none (CRYPTO-03).

Expose `pub fn sealed_chunk_count(len: u64) -> u64` returning the number of full `CHUNK_SIZE` chunks
in a buffer of that length. Phase 2's append fast path re-hashes exactly the last sealed chunk, so it
needs this number without holding the data.

Write the `<behavior>` assertions. The append assertion is the one that proves the no-CDC decision:
compare the actual id lists before and after, not merely the counts. The transposition assertion must
assert on the error, not merely that the output differs — divergent output would also happen in a
format with no integrity at all, so it proves nothing on its own.
  </action>
  <verify>
    <automated>cargo test --lib sync::chunk</automated>
  </verify>
  <done>All `<behavior>` lines pass, including the append-stability assertion over real id lists and the transposition refusal.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|---|---|
| pack bytes → `unframe` | Length fields inside a decrypted frame are attacker-influenced until the AEAD tag has verified, and must still be range-checked afterwards |
| ciphertext lengths → observer | Anyone who can list objects sees per-chunk sizes |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|---|---|---|---|---|---|
| T-02-01 | Denial of service | `unframe` | high | mitigate | `comp_len` and `true_len` are bounds-checked against the buffer before any slice or allocation, so a crafted frame cannot panic or over-allocate |
| T-02-02 | Information disclosure | tail ciphertext length | medium | mitigate | Padding applied after compression reduces the tail-size signal to a power of two |
| T-02-03 | Tampering | `reassemble` under swap or reorder | high | mitigate | Id bound as associated data, plus the plaintext identity recheck in `open_chunk`; any failure aborts the whole reassembly with zero bytes returned |
| T-02-04 | Information disclosure | confirmation of a guessed file | high | mitigate | The id is a *keyed* hash, so repository read access alone does not let an attacker recompute the address of a guessed plaintext |
| T-02-05 | Information disclosure | total object count and sizes | low | accept | Hiding aggregate volume needs constant-rate cover traffic; documented as accepted leakage in 1-08 |
</threat_model>

<verification>
- `cargo test --lib sync::chunk` passes with `$HOME` unset.
- No test in this module opens a file or touches the clock.
</verification>

<success_criteria>
1. A multi-hundred-KiB fixture round-trips byte-exactly through split, frame, zstd, seal, open, unframe, reassemble.
2. Chunk ids are a keyed function of raw plaintext, asserted directly and independent of the compression stage.
3. An append leaves every previously sealed chunk id unchanged, asserted over the id lists.
4. A swapped, transposed, or malformed chunk aborts reassembly with zero bytes returned.
</success_criteria>

<output>
Create `.planning/phases/01-encrypted-bundle-core/1-02-SUMMARY.md` when done. Record the exact frame
layout and the `Blob` and `seal_chunk`/`open_chunk` signatures — 1-03 and 1-04 build directly on
them, 1-07 pins a vector against the layout, and 1-08 documents it.
</output>
