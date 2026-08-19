---
phase: 1-encrypted-bundle-core
plan: 04
type: execute
wave: 2
depends_on: [1-01]
files_modified:
  - src/sync/model.rs
autonomous: true
requirements: [CRYPTO-02, CRYPTO-05]
must_haves:
  truths:
    - "A snapshot root names its manifest by id, and that id is bound as associated data at every hop."
    - "A reader refuses a format_version it does not know instead of guessing."
    - "The chunker identifier travels inside every snapshot, so the chunker can change without breaking restore."
    - "A manifest missing a referenced chunk is reported as a missing chunk, never as a shorter file."
  artifacts:
    - src/sync/model.rs with Manifest, Root, and IndexObject, each versioned and sealed
  key_links:
    - "root → manifest_id → manifest → chunk ids → chunks: every hop names the next and binds that name as associated data"
    - "created_at is an injected parameter, never a wall-clock read — that is what keeps the tests hermetic"
---

<objective>
The snapshot model: the manifest that maps files to ordered chunk ids, the root that names the
manifest and carries the monotonic counter, and the index object that maps chunk ids to their pack
location with a `supersedes` list.

The chain `root → manifest_id → manifest → chunk ids → chunks` is authenticated at every hop, and
every hop's identifier is bound as associated data into the object it names. That is what closes
chunk swap and truncation completely (`research/encryption.md` §4).

Implements **D-02 (D2)** — `format_version`, the chunker identifier, and the full KDF parameter set
travel with the data, and a reader refuses a version it does not know rather than guessing. The
project has already been bitten by a format that could not evolve; this one can.

Purpose: the object graph a restore walks.
Output: `src/sync/model.rs`.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/1-encrypted-bundle-core/1-CONTEXT.md
@.planning/research/encryption.md
@.planning/research/chunking-storage.md
@.planning/phases/1-encrypted-bundle-core/1-01-SUMMARY.md
@src/sync/crypto.rs
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
    - A manifest whose serialized `format` is bumped to an unknown value is refused on open, before any other validation.
    - The sealed manifest bytes contain none of the file paths in the clear.
    - `chunker` reads back as the literal `fixed-256k` from the constant, not from a re-typed string.
  </behavior>
  <action>
Implement the manifest in `src/sync/model.rs`.

`pub struct FileEntry { pub path: String, pub mode: u32, pub true_len: u64, pub chunks: Vec<ChunkId> }`
and `pub struct Manifest { pub format: u32, pub chunker: String, pub files: Vec<FileEntry> }`, both
serde-derived. Construct with `pub fn new(files: Vec<FileEntry>) -> Self` setting `format` from
`FORMAT_VERSION` and `chunker` from `CHUNKER_ID` — never a re-typed literal, so the one constant in
`mod.rs` stays the single source of truth.

`pub fn seal(&self, keys: &Keys) -> Result<(ChunkId, Vec<u8>)>` serializes to JSON, zstd level 3, and
seals through `chunk::seal_chunk`, so the manifest is an ordinary sealed chunk that happens to
describe the others. That is deliberate: no filenames, sizes, or directory structure ever sit in the
clear.

`pub fn open(keys: &Keys, id: &ChunkId, ciphertext: &[u8]) -> Result<Manifest>` opens, decompresses,
deserializes, and then — first, before touching any other field — refuses a `format` that is not
`FORMAT_VERSION`, with a message telling the user their client is too old for this bundle. Refuse an
unrecognised `chunker` the same way. Guessing here is exactly the failure D2 exists to prevent.

Add `pub fn missing_chunks(&self, available: &HashSet<ChunkId>) -> Vec<ChunkId>` returning every
referenced id not present in the given set. Phase 5 uses it to report "chunk missing" rather than
silently restoring a shorter file, which is the difference between a detected truncation and a
corrupted credential.

Write the `<behavior>` assertions inline with the cheap KDF parameters.
  </action>
  <verify>
    <automated>cargo test --lib sync::model</automated>
  </verify>
  <done>All `<behavior>` lines pass. An unknown `format` or `chunker` is refused before any other validation.</done>
</task>

<task type="auto" tdd="true">
  <name>Task 2: Snapshot root and the index object</name>
  <files>src/sync/model.rs</files>
  <behavior>
    - A root seals and reopens to an identical root including its counter and manifest id.
    - Two seals of the same root produce different bytes yet both reopen correctly — the root nonce is random, unlike a chunk's.
    - A root sealed under one master key does not open under another.
    - A root whose sealed bytes have one bit flipped fails to open.
    - An index object round-trips with a populated `supersedes` list and resolves a known chunk id to its pack, offset, and lengths.
    - Both readers refuse an unknown `format`.
  </behavior>
  <action>
Add the root and the index object.

`pub struct Root { pub format: u32, pub counter: u64, pub created_at: DateTime<Utc>, pub repo_id:
String, pub manifest_id: ChunkId }`. The constructor takes `now: DateTime<Utc>` as a parameter — no
function in this module reads the wall clock, so every test can pin a fixed timestamp and stay
hermetic. `chrono` is already a dependency.

`pub fn seal(&self, keys: &Keys) -> Result<Vec<u8>>` goes through `Keys::seal_root`, which prefixes a
fresh random 24-byte nonce. The root is the one mutable object in the format: its plaintext changes
every sync, so a content-derived nonce would leak whether two snapshots are identical. Comment that
distinction — it is the single place the deterministic-nonce rule is deliberately inverted.

`pub fn open(keys: &Keys, framed: &[u8]) -> Result<Root>` with the same version refusal as the
manifest. `repo_id` pins the repository's identity inside the root plaintext, so swapping the whole
repository for a different one is detected in addition to the wrong keyfile failing to unwrap.

`pub struct IndexEntry { pub id: ChunkId, pub pack: ChunkId, pub offset: u64, pub clen: u32, pub
true_len: u32 }` and `pub struct IndexObject { pub format: u32, pub entries: Vec<IndexEntry>, pub
supersedes: Vec<ChunkId> }`, sealed as an ordinary chunk like the manifest. `supersedes` names the
index objects a repack replaced, so Phase 4 can delete them in the right order — an index must stop
referencing a pack before that pack is deleted.

Add `pub fn resolve(&self, id: &ChunkId) -> Option<&IndexEntry>`.

Write the `<behavior>` assertions.
  </action>
  <verify>
    <automated>cargo test --lib sync::model</automated>
  </verify>
  <done>All `<behavior>` lines pass. Root sealing is non-deterministic by design and documented as the deliberate exception.</done>
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
| T-04-02 | Tampering | manifest truncation | high | mitigate | The manifest is itself a sealed chunk; dropping entries breaks its tag, and a dropped referenced chunk surfaces through `missing_chunks` as missing, never as a shorter file |
| T-04-03 | Spoofing | whole-repository substitution | high | mitigate | `repo_id` is pinned inside the root plaintext, on top of the wrong keyfile failing to unwrap |
| T-04-04 | Information disclosure | root plaintext equality across syncs | medium | mitigate | Fresh random 24-byte nonce per root, so two identical snapshots are not visibly identical |
| T-04-05 | Repudiation | unknown future format | medium | mitigate | `format` and `chunker` are refused when unrecognised, rather than parsed optimistically |
| T-04-06 | Tampering | snapshot rollback (a replay of authentic data) | high | mitigate | Cannot be solved by the crypto in this module; the local monotonic anchor in 1-05 is the answer |
</threat_model>

<verification>
- `cargo test --lib sync::model` passes with `$HOME` unset.
- No test reads the clock; `created_at` is a fixed injected `DateTime<Utc>` in every test.
</verification>

<success_criteria>
1. Manifest, root, and index object each seal and reopen intact, and each refuses an unknown `format`.
2. The chunker identifier reaches every snapshot from the single constant.
3. A bit flipped anywhere in a sealed root makes it fail to open.
4. A manifest referencing an absent chunk reports that chunk as missing.
</success_criteria>

<output>
Create `.planning/phases/1-encrypted-bundle-core/1-04-SUMMARY.md` when done. Record the root and
manifest field sets — 1-06 rolls a snapshot back and 1-08 documents the object graph.
</output>
