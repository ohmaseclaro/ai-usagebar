---
phase: 01-encrypted-bundle-core
plan: 04
type: execute
wave: 3
depends_on: [1-01, 1-02]
files_modified:
  - src/sync/model.rs
autonomous: true
requirements: [CRYPTO-02, CRYPTO-05]
must_haves:
  truths:
    - "A snapshot root names its manifest by id, and that id is bound as associated data at every hop."
    - "A reader accepts any object version at or below its ceiling and refuses only what is greater."
    - "The root carries the chunker identifier and the KDF parameters, so a reader can refuse before touching anything else."
    - "A manifest missing a referenced chunk is reported as a missing chunk, never as a shorter file."
  artifacts:
    - src/sync/model.rs with Manifest, Root, and IndexObject, each versioned with a read ceiling
  key_links:
    - "root → manifest_id → manifest → chunk ids → chunks: every hop names the next and binds that name as associated data"
    - "created_at is an injected parameter, never a wall-clock read — that is what keeps the tests hermetic"
    - "The root is the first object a reader touches, which is why the chunker id and KDF params live there"
---

<objective>
The snapshot model: the manifest that maps files to ordered chunk ids, the root that names the
manifest and carries the monotonic counter, and the index object that maps chunk ids to their pack
location with a `supersedes` list.

The chain `root → manifest_id → manifest → chunk ids → chunks` is authenticated at every hop, and
every hop's identifier is bound as associated data into the object it names. That is what closes
chunk swap and truncation completely (`research/encryption.md` §4).

Implements **D-02 (D2)** — `format_version`, the chunker identifier, and the full KDF parameter set
travel with the snapshot, and a reader refuses only a version *above* what it understands. The
project has already been bitten by a format that could not evolve; this one can, in both directions.

Purpose: the object graph a restore walks.
Output: `src/sync/model.rs`.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/01-encrypted-bundle-core/1-CONTEXT.md
@.planning/research/encryption.md
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
  <name>Task 1: Manifest — files, modes, true lengths, ordered chunk ids</name>
  <files>src/sync/model.rs</files>
  <behavior>
    - A manifest with three file entries seals and reopens to an identical manifest.
    - Opening a sealed manifest under the wrong id errors.
    - A manifest at `format = 1` still opens when `MAX_SUPPORTED_MANIFEST` is raised to 2 — an older bundle stays readable by a newer client.
    - A manifest whose serialized `format` is one above the ceiling is refused, before any other validation.
    - The sealed manifest bytes contain none of the file paths in the clear.
    - `chunker` reads back as the literal `fixed-256k` from the constant, not from a re-typed string.
    - A manifest describing enough files to exceed one chunk still seals and reopens intact.
  </behavior>
  <action>
Implement the manifest in `src/sync/model.rs`.

`pub struct FileEntry { pub path: String, pub mode: u32, pub true_len: u64, pub chunks: Vec<ChunkId> }`
and `pub struct Manifest { pub format: u32, pub chunker: String, pub files: Vec<FileEntry> }`, both
serde-derived. Construct with `pub fn new(files: Vec<FileEntry>) -> Self` setting `format` from
`MANIFEST_VERSION` and `chunker` from `CHUNKER_ID` — never a re-typed literal, so the one constant in
`mod.rs` stays the single source of truth.

`pub fn seal(&self, keys: &Keys) -> Result<(ChunkId, Vec<u8>)>` serializes to JSON and hands it
straight to `chunk::seal_chunk`, which already applies zstd inside the frame. Do not compress before
calling it; that would compress twice for no gain. The manifest is therefore an ordinary sealed chunk
that happens to describe the others — deliberately, so no filenames, sizes, or directory structure
ever sit in the clear.

Note in the module doc comment that the manifest for a large bundle **will exceed one chunk**: the
milestone's chat-session-index category alone is several thousand files, and their entries plus
32-byte chunk ids run past 256 KiB of JSON. `chunk::seal_chunk` seals a single buffer, so a manifest
larger than one chunk must be split across chunks and referenced by a list of ids. Phase 1 does not
need that split — every fixture here fits — but Phase 2 builds real manifests and would otherwise
inherit a silent single-chunk assumption. Flag it here so it is a known boundary rather than a
surprise. Include one `<behavior>` fixture that exceeds a chunk so the boundary is exercised now.

`pub fn open(keys: &Keys, id: &ChunkId, ciphertext: &[u8]) -> Result<Manifest>` opens, deserializes,
and then — first, before touching any other field — refuses a `format` **greater than**
`MAX_SUPPORTED_MANIFEST`, with a message telling the user their client is too old for this bundle.
Anything at or below the ceiling is accepted; an equality check here would mean a v2 client could not
read a v1 bundle, which is the CRYPTO-02 promise inverted. Refuse an unrecognised `chunker` the same
way, by membership in a known set rather than by equality with the current one.

Add `pub fn missing_chunks(&self, available: &HashSet<ChunkId>) -> Vec<ChunkId>` returning every
referenced id not present in the given set. Phase 5 uses it to report "chunk missing" rather than
silently restoring a shorter file, which is the difference between a detected truncation and a
corrupted credential.

Write the `<behavior>` assertions inline with the cheap KDF parameters.
  </action>
  <verify>
    <automated>cargo test --lib sync::model</automated>
  </verify>
  <done>All `<behavior>` lines pass, including the forward-compatible open and the larger-than-one-chunk manifest.</done>
</task>

<task type="auto" tdd="true">
  <name>Task 2: Snapshot root and the index object</name>
  <files>src/sync/model.rs</files>
  <behavior>
    - A root seals and reopens to an identical root including its counter, chunker, KDF parameters, and manifest id.
    - Two seals of the same root produce different bytes yet both reopen correctly — the root nonce is random, unlike a chunk's.
    - A root sealed under one master key does not open under another.
    - A root whose sealed bytes have one bit flipped fails to open.
    - A root naming an unknown chunker is refused, and the message names the chunker it found.
    - A root at a version below the ceiling opens; one above it is refused.
    - An index object round-trips with a populated `supersedes` list and resolves a known chunk id to its pack, offset, and lengths.
  </behavior>
  <action>
Add the root and the index object.

`pub struct Root { pub format: u32, pub counter: u64, pub created_at: DateTime<Utc>, pub repo_id:
String, pub manifest_id: ChunkId, pub chunker: String, pub kdf: KdfParams }`.

The last two fields are why the root is worth reading first. `1-CONTEXT.md` D2 requires the snapshot
header to record the chunker and the full KDF parameter set, and the root is the cheapest place to
honour that: it is the first object any reader touches, so an unknown chunker or an unsupported KDF
configuration can be refused before a single pack is fetched. Note in a comment that the KDF
parameters here are informational — the authoritative copy lives in the keyfile, where they are bound
as associated data — and that a mismatch between the two is a signal worth reporting rather than
silently preferring one.

The constructor takes `now: DateTime<Utc>` as a parameter. No function in this module reads the wall
clock, so every test can pin a fixed timestamp and stay hermetic. `chrono` is already a dependency.

`pub fn seal(&self, keys: &Keys) -> Result<Vec<u8>>` goes through `Keys::seal_root`, which uses the
root subkey and prefixes a fresh random 24-byte nonce. The root is the one mutable object in the
format: its plaintext changes every sync, so a content-derived nonce would leak whether two snapshots
are identical. Comment that distinction — it is the single place the deterministic-nonce rule is
deliberately inverted.

`pub fn open(keys: &Keys, framed: &[u8]) -> Result<Root>` with the same at-or-below version rule as
the manifest, plus the chunker check. `repo_id` pins the repository's identity inside the root
plaintext, so swapping the whole repository for a different one is detected in addition to the wrong
keyfile failing to unwrap.

`pub struct IndexEntry { pub id: ChunkId, pub pack: ChunkId, pub offset: u64, pub clen: u32, pub
true_len: u32 }` and `pub struct IndexObject { pub format: u32, pub entries: Vec<IndexEntry>, pub
supersedes: Vec<ChunkId> }`, sealed as an ordinary chunk like the manifest and versioned the same
way. `supersedes` names the index objects a repack replaced, so Phase 4 can delete them in the right
order — an index must stop referencing a pack before that pack is deleted.

Add `pub fn resolve(&self, id: &ChunkId) -> Option<&IndexEntry>`.

Write the `<behavior>` assertions.
  </action>
  <verify>
    <automated>cargo test --lib sync::model</automated>
  </verify>
  <done>All `<behavior>` lines pass. The root carries the chunker and KDF parameters, and root sealing is non-deterministic by design and documented as the deliberate exception.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|---|---|
| remote root bytes → `Root::open` | The pointer is the most attacker-interesting object: it names everything else |
| remote manifest → `Manifest::open` | Paths and chunk lists arrive untrusted |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|---|---|---|---|---|---|
| T-04-01 | Tampering | the root → manifest → chunk chain | critical | mitigate | Every hop's identifier is bound as associated data into the object it names, so a substituted manifest or chunk fails its tag |
| T-04-02 | Tampering | manifest truncation or reordering | high | mitigate | The manifest is itself a sealed chunk; dropping or transposing entries breaks its tag, and a dropped referenced chunk surfaces through `missing_chunks` as missing, never as a shorter file |
| T-04-03 | Spoofing | whole-repository substitution | high | mitigate | `repo_id` is pinned inside the root plaintext, on top of the wrong keyfile failing to unwrap |
| T-04-04 | Information disclosure | root plaintext equality across syncs | medium | mitigate | Fresh random 24-byte nonce per root, so two identical snapshots are not visibly identical |
| T-04-05 | Denial of service | a bundle written by a future client | medium | mitigate | Read ceilings per object refuse only versions above what this build understands, and the root's chunker field lets that refusal happen before any pack is fetched |
| T-04-06 | Tampering | snapshot rollback (a replay of authentic data) | high | mitigate | Cannot be solved by the crypto in this module; the local monotonic anchor in 1-05 is the answer |
</threat_model>

<verification>
- `cargo test --lib sync::model` passes with `$HOME` unset.
- No test reads the clock; `created_at` is a fixed injected `DateTime<Utc>` in every test.
</verification>

<success_criteria>
1. Manifest, root, and index object each seal and reopen intact, each accepting versions at or below its ceiling and refusing only what is greater.
2. The chunker identifier and the KDF parameters reach every root, and an unknown chunker is refused early.
3. A bit flipped anywhere in a sealed root makes it fail to open.
4. A manifest referencing an absent chunk reports that chunk as missing.
5. A manifest larger than one chunk is exercised, and the single-chunk boundary is documented for Phase 2.
</success_criteria>

<output>
Create `.planning/phases/01-encrypted-bundle-core/1-04-SUMMARY.md` when done. Record the root and
manifest field sets and the multi-chunk manifest boundary — 1-06 rolls a snapshot back and reorders a
manifest, 1-08 documents the object graph, and Phase 2 builds real manifests.
</output>
