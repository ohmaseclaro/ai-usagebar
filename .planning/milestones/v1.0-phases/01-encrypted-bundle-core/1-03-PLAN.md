---
phase: 01-encrypted-bundle-core
plan: 03
type: execute
wave: 3
depends_on: [1-01, 1-02]
files_modified:
  - src/sync/pack.rs
autonomous: true
requirements: [CRYPTO-01, CRYPTO-05]
must_haves:
  truths:
    - "Many blobs write into one pack and every one of them reads back byte-exactly."
    - "A pack's file name is derived from its own bytes, so a substituted pack cannot keep the name it is served under."
    - "The pack header is sealed under a keyed id stored in the trailer, so a reader can find that id before decrypting."
    - "A pack truncated by a single byte fails to open rather than yielding the blobs that survived."
  artifacts:
    - src/sync/pack.rs with PackWriter, the sealed trailing header, the trailer id, and the sharded name function
  key_links:
    - "The trailer is `<sealed header><32-byte header id><u32 LE header len>` — the id must be readable before the header can be opened"
    - "pack.rs seals through chunk::seal_chunk and names through crypto::content_address, importing no crypto crate itself"
---

<objective>
The pack format: blob ciphertexts concatenated, then a sealed header listing them, then that header's
keyed id, then a `u32` little-endian header length. This is restic's layout, simplified, and it is
what makes bulk upload possible at all — GitHub's content-creation limits of 80 per minute and 500
per hour make one request per chunk structurally impossible, so 5,000 chunks must become a handful of
objects.

Phase 1 builds the format only. Nothing here touches the filesystem or the network: a pack is a
`Vec<u8>` in, a `Vec<u8>` out, per **D-01 (D1)**.

Purpose: the unit of transfer.
Output: `src/sync/pack.rs`.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/01-encrypted-bundle-core/1-CONTEXT.md
@.planning/research/chunking-storage.md
@.planning/phases/01-encrypted-bundle-core/1-01-SUMMARY.md
@.planning/phases/01-encrypted-bundle-core/1-02-SUMMARY.md
@src/sync/crypto.rs
@src/sync/chunk.rs
@src/sync/mod.rs
@CLAUDE.md
</context>

<tasks>

<task type="auto" tdd="true">
  <name>Task 1: PackWriter and the sealed trailing header</name>
  <files>src/sync/pack.rs</files>
  <behavior>
    - Three blobs written into one pack all read back byte-exactly, each at its recorded offset and length.
    - The trailer parses: the last four bytes give a header length, the 32 bytes before that give a header id, and together they locate a header that opens.
    - The plaintext of a blob appears nowhere in the finished pack bytes.
    - A pack whose header ciphertext has one bit flipped fails to open.
    - A pack whose trailer header id is replaced by another valid id fails to open — the id is bound as associated data, so a substituted id breaks the tag.
    - Reading the header with the wrong `Keys` fails rather than returning entries.
    - A pack containing zero blobs is rejected at `finish` rather than producing a headerless object.
  </behavior>
  <action>
Implement the pack container in `src/sync/pack.rs`.

Layout, documented in the module doc comment:
`<blob0 ciphertext><blob1 ciphertext>…<blobN ciphertext><sealed header><32-byte header id><u32 LE header length>`.

**Where the header id lives is the point of this layout.** The header is sealed through
`chunk::seal_chunk`, which addresses it by `keys.chunk_id` of the serialized header — a *keyed* hash.
A reader needs that id before it can decrypt, because the id is bound as associated data, so the id
is written into the trailer immediately before the length. Two things follow and both belong in
comments. First, the id must be keyed rather than a `content_address`: the header is a list of chunk
ids the attacker can already see in the manifest, making it the most guessable object in the format,
and an unkeyed address would hand anyone with repository read access a confirmation oracle. Second,
storing the keyed id in the clear costs nothing — an attacker without `name_key` cannot recompute it,
and substituting it simply breaks the tag.

`pub struct PackEntry { pub id: ChunkId, pub offset: u64, pub clen: u32, pub true_len: u32 }`,
serde-serializable. The header is `pub struct PackHeader { pub format: u32, pub entries:
Vec<PackEntry> }` — serialize as JSON and hand it straight to `chunk::seal_chunk`, which already
compresses. Do not zstd it yourself; that would compress twice. Refuse a `format` greater than
`MAX_SUPPORTED_PACK_HEADER` on read, accepting anything at or below, so a future writer can raise the
version without orphaning today's packs.

The header records ciphertext length and the blob's true plaintext length. It records no path, no
filename, and no directory structure — those live in the manifest, which is itself a sealed chunk
(1-04). Say so in a comment: file paths leak account UUIDs and session ids, so they must never sit
in a pack header.

`pub struct PackWriter` with `pub fn new() -> Self`, `pub fn push(&mut self, blob: Blob)`,
`pub fn len_bytes(&self) -> usize`, `pub fn is_empty(&self) -> bool`, and
`pub fn finish(self, keys: &Keys) -> Result<(ChunkId, Vec<u8>)>` returning the pack's *content
address* — `crypto::content_address` over the finished bytes, which is naming only and never a
sealing address — and the bytes themselves. `finish` on an empty writer is an error.

`pub fn read_header(keys: &Keys, pack: &[u8]) -> Result<PackHeader>` — read the trailing `u32`, then
the 32-byte id before it, check that the implied header region lies inside the buffer, open it
through `chunk::open_chunk` (which performs the plaintext identity recheck), then check that every
entry's `offset + clen` also lies inside the blob region. All bounds checks happen before any entry
is returned. A truncated pack must fail here, not later at an out-of-bounds slice.

`pub fn blob_bytes<'a>(pack: &'a [u8], entry: &PackEntry) -> Result<&'a [u8]>` returning the
ciphertext slice, and `pub fn open_blob(keys: &Keys, pack: &[u8], entry: &PackEntry) ->
Result<Zeroizing<Vec<u8>>>` combining that with `chunk::open_chunk`.

Add the sizing constants with their source: `pub const PACK_TARGET: usize = 32 * 1024 * 1024;` and
`pub const PACK_MAX: usize = 48 * 1024 * 1024;` — the CAL-1 fallback recorded in the roadmap. Phase 3
may raise them if release assets turn out to honour `Range:`; 1-08 runs that probe.

Write the `<behavior>` assertions inline with the cheap KDF parameters.
  </action>
  <verify>
    <automated>cargo test --lib sync::pack</automated>
  </verify>
  <done>Every `<behavior>` line passes. The header id is readable from the trailer before decryption, is keyed, and a substituted id breaks the tag.</done>
</task>

<task type="auto" tdd="true">
  <name>Task 2: Content-addressed sharded pack names and the sealing policy</name>
  <files>src/sync/pack.rs</files>
  <behavior>
    - `shard_path` for an id beginning `ab` returns exactly `packs/ab/<64 lowercase hex>.pack`.
    - Two packs with identical bytes get identical names; changing one byte changes the name.
    - `should_seal` returns true once the next blob would push the writer past `PACK_MAX`, and false while it stays under.
    - A writer fed blobs until `should_seal` fires produces a pack no larger than `PACK_MAX`.
    - Removing the final byte, and separately removing the last kilobyte, both fail at `read_header` with no entry returned.
  </behavior>
  <action>
Add `pub fn shard_path(id: &ChunkId) -> String` returning `packs/<first two hex chars>/<full 64 hex>.pack`.
Two-level fanout keeps any single listing far below GitHub's 3,000-entry directory width even if the
store later moves back into a git tree; it costs one line, so take it.

Add `pub fn should_seal(current_len: usize, next_blob_len: usize) -> bool` returning true when
appending would exceed `PACK_MAX`. Keeping the decision as a pure function means Phase 2's plan
builder can size packs without instantiating a writer.

Add a module-level comment stating that packs are immutable once sealed: a chunk already inside a
pack is never re-packed except by an explicit prune-repack, which is Phase 4's job. That immutability
is what makes a crashed sync leave orphan packs — garbage, never corruption.

Write the `<behavior>` assertions, including the two truncation cases.
  </action>
  <verify>
    <automated>cargo test --lib sync::pack</automated>
  </verify>
  <done>All `<behavior>` lines pass. Pack names are a pure function of pack bytes, and truncation fails at header read.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|---|---|
| remote pack bytes → `read_header` | Every offset, length, and the trailer id in a fetched pack is attacker-controlled until validated |
| pack file name → reader | The name is public and chosen by whoever serves the object |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|---|---|---|---|---|---|
| T-03-01 | Tampering | `read_header` | critical | mitigate | Trailer length, trailer id, and every entry's `offset + clen` are bounds-checked before any entry is returned; the header is sealed with its id bound as associated data, so truncation or id substitution breaks its tag |
| T-03-02 | Spoofing | pack substitution | high | mitigate | The pack name is `content_address` of its own bytes, so a substituted pack cannot keep the name it is served under |
| T-03-03 | Information disclosure | confirming a guessed pack header | high | mitigate | The header is addressed by a *keyed* chunk id, never by an unkeyed content address, so repository read access does not permit recomputing it |
| T-03-04 | Information disclosure | pack header contents | high | mitigate | The header is sealed and carries no path, filename, or directory structure — only ids, offsets, and lengths |
| T-03-05 | Denial of service | crafted header length | high | mitigate | A header length larger than the pack is rejected before allocation |
</threat_model>

<verification>
- `cargo test --lib sync::pack` passes with `$HOME` unset.
- No test writes a file; a pack is a `Vec<u8>` throughout.
</verification>

<success_criteria>
1. Multiple blobs pack and unpack byte-exactly through the sealed header.
2. The trailer stores the header's keyed id, and a reader locates it before decrypting.
3. A flipped bit, a substituted header id, and a truncated pack all fail to open.
4. No blob plaintext appears anywhere in a finished pack.
5. Pack names are content-addressed and sharded two levels deep.
</success_criteria>

<output>
Create `.planning/phases/01-encrypted-bundle-core/1-03-SUMMARY.md` when done. Record the exact pack
layout including the trailer — 1-06 truncates a real pack and 1-08 documents the format.
</output>
