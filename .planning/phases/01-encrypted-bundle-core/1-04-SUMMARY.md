---
phase: 01-encrypted-bundle-core
plan: 04
subsystem: model
tags: [manifest, snapshot-root, index-object, format-versioning, ordering-integrity, chunker-id]

# Dependency graph
requires: [1-01, 1-02]
provides:
  - "src/sync/model.rs — Manifest, Root, IndexObject: the object graph a restore walks"
  - "Ordering integrity for chunk lists, owned here because the chunk layer demonstrably cannot provide it"
  - "Manifest::missing_chunks — a dropped chunk is reported missing rather than restored as a shorter file"
  - "IndexObject::resolve — chunk id → (pack, offset, clen, true_len)"
  - "Shared seal_object/open_object/probe_version/check_chunker helpers for every sealed JSON object"
affects: [1-06, 1-07, 1-08, phase-2, phase-4, phase-5]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Version is probed from a two-field VersionProbe *before* full deserialization, so a v2 object is refused with 'upgrade ai-usagebar' rather than a confusing missing-field error"
    - "Chunker acceptance by set membership (KNOWN_CHUNKERS), never equality with CHUNKER_ID"
    - "Ceiling-as-parameter seam (Manifest::open_with_ceiling), mirroring the KdfParams cheap-KDF seam"
    - "Attacker-supplied strings reaching an error message are char-bounded and {:?}-escaped"

key-files:
  created: []
  modified:
    - src/sync/model.rs

key-decisions:
  - "Ordering integrity is closed by construction, not by a check: the ordered id list lives inside the manifest's sealed plaintext, so a transposition either breaks the Poly1305 tag or changes the manifest's own id — and the root names the old id"
  - "Manifest::open routes through chunk::open_chunk, not Keys::open, so the chunk_id(plaintext) == id recheck runs and a re-sealed manifest served under the original id is refused"
  - "The manifest is handed to seal_chunk uncompressed — seal_chunk already runs zstd inside the frame, and a second compression stage would cost CPU and add a second determinism surface"
  - "A manifest past CHUNK_SIZE is refused by name rather than silently split: splitting means Root::manifest_id becomes a list, which is a format change and therefore a Phase 2 decision"
  - "Root carries chunker + kdf as informational duplicates of the keyfile's authoritative copy, because the root is the first object a reader touches"
  - "Root sealing is non-deterministic by design — the single deliberate inversion of the deterministic-nonce rule"

patterns-established:
  - "Every sealed JSON object in the format goes through one seal_object/open_object pair, so the size guard, the version probe, and the id recheck cannot be forgotten per-object"
  - "Hermetic time: created_at is a constructor parameter; no function in the module reads the clock"

requirements-completed: [CRYPTO-02, CRYPTO-05]

coverage:
  - id: D1
    description: "A manifest seals and reopens identically; opening it under the wrong id errors"
    requirement: CRYPTO-05
    verification:
      - kind: unit
        ref: "cargo test --lib sync::model — a_three_file_manifest_seals_and_reopens_identically, a_manifest_opened_under_the_wrong_id_is_refused"
        status: pass
    human_judgment: false
  - id: D2
    description: "Version acceptance is at-or-below a ceiling: an older bundle opens under a raised ceiling, one above the ceiling is refused before any other validation"
    requirement: CRYPTO-02
    verification:
      - kind: unit
        ref: "cargo test --lib sync::model — a_manifest_written_by_an_older_client_opens_when_the_ceiling_is_raised, a_manifest_one_version_above_the_ceiling_is_refused, a_root_below_the_ceiling_opens_and_one_above_it_is_refused, an_index_object_above_the_ceiling_is_refused"
        status: pass
    human_judgment: false
  - id: D3
    description: "The sealed manifest carries no file path in the clear, and the chunker reads back as the CHUNKER_ID constant"
    requirement: CRYPTO-05
    verification:
      - kind: unit
        ref: "cargo test --lib sync::model — the_sealed_manifest_carries_no_file_path_in_the_clear, the_chunker_reads_back_as_the_constant_this_build_writes"
        status: pass
    human_judgment: false
  - id: D4
    description: "Transposing two chunk ids inside a sealed manifest fails to open, returning zero entries (the 1-06 Attack 8 shape)"
    requirement: CRYPTO-05
    verification:
      - kind: unit
        ref: "cargo test --lib sync::model — transposing_two_chunk_ids_inside_a_sealed_manifest_yields_zero_entries"
        status: pass
    human_judgment: false
  - id: D5
    description: "A referenced chunk that is absent is reported missing rather than skipped"
    requirement: CRYPTO-05
    verification:
      - kind: unit
        ref: "cargo test --lib sync::model — a_referenced_chunk_that_is_absent_is_reported_missing_not_skipped"
        status: pass
    human_judgment: false
  - id: D6
    description: "A root round-trips with counter, chunker, KDF parameters and manifest id; two seals differ yet both reopen; a foreign master key and any single flipped bit both fail"
    requirement: CRYPTO-02
    verification:
      - kind: unit
        ref: "cargo test --lib sync::model — a_root_seals_and_reopens_with_every_field_intact, two_seals_of_one_root_differ_yet_both_reopen, a_root_does_not_open_under_a_different_master_key, one_flipped_bit_anywhere_in_a_sealed_root_fails_to_open"
        status: pass
    human_judgment: false
  - id: D7
    description: "An unknown chunker is refused by set membership and the message names the chunker found"
    requirement: CRYPTO-02
    verification:
      - kind: unit
        ref: "cargo test --lib sync::model — a_root_naming_an_unknown_chunker_is_refused_and_the_message_names_it, a_manifest_naming_an_unknown_chunker_is_refused_and_the_message_names_it"
        status: pass
    human_judgment: false
  - id: D8
    description: "An index object round-trips with a populated supersedes list and resolves a known chunk id to its pack, offset, and lengths"
    requirement: CRYPTO-02
    verification:
      - kind: unit
        ref: "cargo test --lib sync::model — an_index_object_round_trips_and_resolves_a_known_chunk"
        status: pass
    human_judgment: false
  - id: D9
    description: "The single-chunk manifest boundary is exercised: 1,000 files round-trip, 4,000 files exceed a chunk and are refused by name"
    requirement: CRYPTO-05
    verification:
      - kind: unit
        ref: "cargo test --lib sync::model — a_thousand_file_manifest_still_fits_in_one_chunk_and_round_trips, a_four_thousand_file_manifest_exceeds_one_chunk_and_is_refused_by_name"
        status: pass
    human_judgment: false

# Metrics
duration: 30min
completed: 2026-08-19
status: complete
---

# Phase 1 Plan 04: The Snapshot Object Graph Summary

**`root → manifest_id → manifest → chunk ids → chunks`, authenticated at every hop, with ordering
integrity owned by the manifest, per-object read ceilings that refuse only what is above them, and a
root that carries the chunker and KDF parameters so a reader can refuse before fetching a pack.**

## Performance

- **Duration:** ~30 min
- **Tasks:** 2/2
- **Files modified:** 1 (`src/sync/model.rs`, stub → 696 lines)
- **Test suite:** 19 new tests, 0.07 s wall clock; 72 across `sync::` total

## Accomplishments

- **Ordering integrity is closed, and closed by construction rather than by a check.** The ordered
  chunk-id list lives inside the manifest's sealed plaintext, so transposing two ids either breaks
  the manifest's Poly1305 tag (edited in place) or changes the manifest's own id (re-sealed) — and
  the root names the old id. `transposing_two_chunk_ids_inside_a_sealed_manifest_yields_zero_entries`
  asserts all three legs: `evil_id != manifest_id`, opening the re-sealed bytes under the original id
  errors, and a flipped bit in the honest ciphertext errors. This is the shape 1-06 Attack 8 needs.
- **`Manifest::open` routes through `chunk::open_chunk`, not `Keys::open`.** That is what makes the
  re-sealed-manifest leg fail rather than merely being a different object: the
  `chunk_id(plaintext) == id` recheck 1-02 put there is doing load-bearing work here.
- **Version acceptance is at-or-below a ceiling everywhere, and probed before deserialization.** A
  two-field `VersionProbe` reads `format` first, so a v2 object carrying a field this build has never
  heard of is refused with "upgrade ai-usagebar" instead of a serde error about a missing field. A
  `format = 1` manifest opens under a ceiling of 2, proving the forward-compatibility promise.
- **The chunker is checked by set membership.** `KNOWN_CHUNKERS: &[&str] = &[CHUNKER_ID]` — a build
  that introduces a second chunker still reads the bundles it wrote with the first.
- **No file path reaches the clear.** The manifest is an ordinary sealed chunk, and the test greps
  the sealed bytes for `.credentials.json`, `config.toml`, and `history.jsonl`.
- **The multi-chunk manifest boundary is loud, not silent.** A 4,000-file fixture is asserted to
  exceed `CHUNK_SIZE` as JSON and then asserted to be refused by name; a 1,000-file fixture
  round-trips. The module doc explains why the split is a Phase 2 format decision.

## Public surface of `src/sync/model.rs`

```rust
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry { pub path: String, pub mode: u32, pub true_len: u64, pub chunks: Vec<ChunkId> }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest { pub format: u32, pub chunker: String, pub files: Vec<FileEntry> }
impl Manifest {
    pub fn new(files: Vec<FileEntry>) -> Self;                      // format = MANIFEST_VERSION, chunker = CHUNKER_ID
    pub fn seal(&self, keys: &Keys) -> Result<(ChunkId, Vec<u8>)>;
    pub fn open(keys: &Keys, id: &ChunkId, ciphertext: &[u8]) -> Result<Manifest>;
    pub fn missing_chunks(&self, available: &HashSet<ChunkId>) -> Vec<ChunkId>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexEntry { pub id: ChunkId, pub pack: ChunkId, pub offset: u64, pub clen: u32, pub true_len: u32 }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexObject { pub format: u32, pub entries: Vec<IndexEntry>, pub supersedes: Vec<ChunkId> }
impl IndexObject {
    pub fn new(entries: Vec<IndexEntry>, supersedes: Vec<ChunkId>) -> Self;
    pub fn seal(&self, keys: &Keys) -> Result<(ChunkId, Vec<u8>)>;
    pub fn open(keys: &Keys, id: &ChunkId, ciphertext: &[u8]) -> Result<IndexObject>;
    pub fn resolve(&self, id: &ChunkId) -> Option<&IndexEntry>;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Root {
    pub format: u32, pub counter: u64, pub created_at: DateTime<Utc>, pub repo_id: String,
    pub manifest_id: ChunkId, pub chunker: String, pub kdf: KdfParams,
}
impl Root {
    pub fn new(counter: u64, now: DateTime<Utc>, repo_id: String, manifest_id: ChunkId, kdf: KdfParams) -> Self;
    pub fn seal(&self, keys: &Keys) -> Result<Vec<u8>>;             // random 24-byte nonce, via Keys::seal_root
    pub fn open(keys: &Keys, framed: &[u8]) -> Result<Root>;
}
```

Private: `KNOWN_CHUNKERS`, `check_chunker`, `VersionProbe`, `probe_version`, `seal_object`,
`open_object`, and `Manifest::open_with_ceiling` (the ceiling seam).

Nothing in this module imports `argon2` or `chacha20poly1305`; 1-01's containment gate is still green.

## Field sets, for 1-08's format document

**Manifest** — `format` (u32), `chunker` (string), `files` (array of `{path, mode, true_len, chunks[]}`).
Serialized as JSON, then handed **uncompressed** to `chunk::seal_chunk`, which applies zstd inside the
frame. Its `ChunkId` is `keys.chunk_id(manifest_json)`, and that id is what `Root::manifest_id` holds.

**Root** — `format`, `counter` (u64, the monotonic anchor 1-05 compares against), `created_at`
(RFC 3339 `DateTime<Utc>`, always injected), `repo_id` (pins repository identity inside the
plaintext), `manifest_id`, `chunker`, `kdf` (`{m_kib, t, p}`). Sealed via `Keys::seal_root`: a fresh
random 24-byte nonce stored inline, **not** content-derived.

**Index object** — `format`, `entries` (array of `{id, pack, offset, clen, true_len}`), `supersedes`
(the index objects a repack replaced; Phase 4 must stop referencing a pack before deleting it).

## The multi-chunk manifest boundary — read this before Phase 2

`chunk::seal_chunk` seals exactly one buffer of at most `CHUNK_SIZE` (262,144 bytes), measured on the
**raw** JSON, before zstd. A `FileEntry` costs roughly 135 bytes of JSON — a path, a mode, a length,
and a 64-hex chunk id — so a manifest crosses 256 KiB somewhere around 2,000 files. The milestone's
chat-session-index category alone is several thousand files, so **a real bundle's manifest will not
fit in one chunk.**

Phase 1 does not implement the split. `Manifest::seal` refuses an oversized manifest with a message
naming the "single-chunk limit", and `a_four_thousand_file_manifest_exceeds_one_chunk_and_is_refused_by_name`
holds that behaviour in place. Implementing the split means `Root::manifest_id` becomes a *list* of
ids rather than one id — a change to the on-disk format, and therefore a Phase 2 decision to be made
deliberately, not a Phase 1 improvisation.

## Decisions Made

- **`Manifest::open_with_ceiling` exists as a private seam.** `open` supplies `MAX_SUPPORTED_MANIFEST`;
  the seam lets a test open a `format = 1` manifest with a ceiling of 2 and prove the exact behaviour
  the plan names ("an older bundle stays readable by a newer client") rather than a re-derivation of
  it. This mirrors the project's existing "inject the parameter, never read the constant inside" rule
  — the same shape as the `KdfParams` cheap-KDF seam.
- **The version probe is a separate two-field struct.** The plan requires the version refusal to run
  "before any other validation". Deserializing the whole object first would mean a v2 object with a
  new required field fails with a serde message instead of the version message, which is exactly the
  unhelpful failure D2 exists to prevent. Cost is one extra JSON parse of the same buffer.
- **`seal_object` / `open_object` are shared by the manifest and the index object.** One place holds
  the size guard, the version probe, and the id recheck, so neither object can quietly lose one.
- **The unknown-chunker message bounds and escapes the name it echoes.** `found` arrives from a
  remote; it is truncated to 32 chars and printed with `{:?}` so a crafted chunker string cannot
  inject control characters into a terminal or a log.
- **`FileEntry` derives `Debug` and prints `path`.** A path is metadata, not key material, and no
  manifest leaves the process unsealed. D5's rule covers keys, passwords, and plaintext file
  *contents*, none of which live in this struct. The JSON is still wrapped in `Zeroizing` on both the
  seal and open paths.
- **`missing_chunks` does not deduplicate.** The plan asks for "every referenced id not present";
  two files referencing the same absent chunk report it twice, which is the honest count of broken
  references. Phase 5 can collapse it at the call site if it wants a set.
- **`IndexObject::resolve` is a linear scan**, marked with a `ponytail:` comment naming the ceiling
  and the upgrade path (build a `HashMap` at the call site if a restore ever resolves thousands of
  ids against one index). Phase 1 has no such call site.

## Deviations from Plan

1. **A manifest larger than one chunk is refused by name rather than sealed and reopened intact.**
   The plan's `<behavior>` line asks for a >1-chunk manifest that "still seals and reopens intact",
   but the same task's `<action>` says "Phase 1 does not need that split — every fixture here fits",
   and `chunk::seal_chunk` refuses any buffer over `CHUNK_SIZE` by construction. The two cannot both
   hold. Sealing a manifest intact past one chunk requires `Root::manifest_id` to become a list of
   ids — an on-disk format change the plan explicitly fixed the other way (`pub manifest_id:
   ChunkId`), and not something to improvise inside an execution plan. The boundary is instead
   **exercised** exactly as the orchestrator's direction words it: a 4,000-file fixture is asserted
   to exceed `CHUNK_SIZE` and asserted to produce a named refusal, a 1,000-file fixture round-trips,
   and the module doc explains the split and who owns it. Success criterion 5 ("a manifest larger
   than one chunk is exercised, and the single-chunk boundary is documented for Phase 2") is met.
2. **One commit rather than two task commits.** Both tasks own the same single file and share the
   `seal_object` / `open_object` / `probe_version` / `check_chunker` helpers; they were written in one
   pass. Splitting the commit retroactively would have manufactured an intermediate state that never
   existed. Same call, same reason, as 1-02 in this phase.
3. **No ceiling seam on `Root` or `IndexObject`.** Their behaviours are phrased as "a version below
   the ceiling opens; one above it is refused", which `format = MAX_SUPPORTED_ROOT - 1` and
   `format = MAX_SUPPORTED_ROOT + 1` test directly with no test-only plumbing. Only the manifest's
   behaviour is phrased in terms of a *raised* ceiling, so only the manifest carries the seam.

## Issues Encountered

None. First compile, first test run, all 19 green. Only `cargo fmt` had anything to say, and it was
line wrapping.

## Verification

All commands run in the worktree at `.claude/worktrees/1-04`.

| Check | Result |
|---|---|
| `cargo test --lib sync::model` | 19 passed, 0 failed, 0.07 s |
| `cargo test --lib sync::` | 72 passed, 0 failed — 1-01's containment gate still green |
| `env -u HOME -u XDG_CACHE_HOME -u XDG_CONFIG_HOME cargo test --lib sync::` | 72 passed — hermetic |
| `cargo clippy --all-targets -- -D warnings` | clean (exit 0) |
| `cargo fmt -- --check` | clean |

No test in this module opens a file, reads an environment variable, or touches the clock:
`created_at` is the fixed literal `2026-08-19T12:00:00Z` parsed in-test. Every test that derives keys
uses the cheap KDF parameters `{ m_kib: 8, t: 1, p: 1 }`. `KdfParams::default()` appears as *data*
inside a `Root` fixture — it is recorded, never used to derive, so nothing allocates a gibibyte.

Per the plan's scope, only `sync::` was built and tested; no full workspace build and no `make test`.

## Known Stubs

None in this file. `src/sync/pack.rs` remains owned by 1-03 and was not touched.

## Threat Flags

None beyond the plan's `<threat_model>`. T-04-01 through T-04-05 are mitigated and tested. T-04-06
(snapshot rollback via replay of authentic data) is, as the plan states, outside this module's reach
— `Root::counter` is the value 1-05's local monotonic anchor compares against, and that comparison is
where the mitigation lives.

## User Setup Required

None — pure and offline.

## Next Phase Readiness

**Ready.**

- **1-06** has the object graph its attacks need. Attack 8 (transpose two `ChunkId`s inside a sealed
  manifest, assert zero entries) has a working precedent in
  `transposing_two_chunk_ids_inside_a_sealed_manifest_yields_zero_entries`; the essential detail is
  that a re-sealed manifest has a *different* id, so the attack must serve the tampered ciphertext
  under the id the root names.
- **1-07** can pin a compatibility vector against a manifest and a root; note that the root's
  ciphertext is **not** reproducible (random nonce), so a root vector must be a decrypt-side vector.
- **1-08** has the field sets and the chain diagram above to copy into `docs/sync-format.md`.
- **Phase 2** must decide the multi-chunk manifest format before it builds real manifests — see the
  boundary section above.
- **Phase 4** has `IndexObject::supersedes` and the deletion-order rule.
- **Phase 5** has `Manifest::missing_chunks`.

## Self-Check: PASSED

`src/sync/model.rs` is 696 lines and exists on disk; commit `62e1fbb` is in git; every public item
listed above resolves in the compiled crate.
