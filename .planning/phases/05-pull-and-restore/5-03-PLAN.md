---
phase: 05-pull-and-restore
plan: 03
type: execute
wave: 2
depends_on: ["5-01"]
files_modified:
  - src/sync/restore/merge.rs
autonomous: true
requirements: [SAFE-03, SYNC-06]
must_haves:
  truths:
    - "Every manifest entry becomes exactly one `ItemPlan` with exactly one `Disposition`, including the entries that are rejected or excluded — nothing is dropped on the floor (D6)."
    - "An item whose local bytes already hash to the manifest's chunk ids is `SkipIdentical`, decided **before** any timestamp is consulted, which is what makes a second apply a no-op (D7)."
    - "A local file whose mtime is newer than the snapshot's `created_at` is `SkipLocalNewer` and is never written without `force` (SAFE-03, D2)."
    - "Under `force`, a locally-newer **credential** becomes `NeedsCredentialConfirm`, not `Overwrite` — a second explicit confirmation is required and `force` alone does not grant it (D2)."
    - "Restore never plans a deletion. A local file absent from the manifest is left alone, and that is stated in the module doc as a decision, not an omission."
    - "Two machines that edited the same routine converge on the newer one, and the losing value is named in the `ItemPlan` the report renders (SYNC-06)."
    - "The disposition function is pure — local stat facts, the manifest entry, and the snapshot time arrive as arguments — so every branch is tested without a filesystem."
  artifacts:
    - src/sync/restore/merge.rs — `plan`, the pure `decide` it is built from, and the credential classification
  key_links:
    - "the snapshot's `Root.created_at` is the remote timestamp for every item, because `model::FileEntry` carries no per-file mtime — and plan 5-04 writes that same time onto every restored file, which is what makes the comparison exact rather than merely conservative"
    - "digest before timestamp: the identical case must not be a conflict, or a re-run of an interrupted restore would report 200 conflicts it does not have"
    - "`layout::from_manifest_path` and `layout::accept_for_write` are 5-01's; this module calls them and does not restate a path rule"
---

<objective>
The per-item decision. Turn an opened `Manifest` into a `RestorePlan` where every entry carries a
disposition and a reason: created, updated, identical, skipped because local is newer, overwritten
under force, held for a credential confirmation, excluded by policy, or rejected as a hostile path.

Purpose: this is SAFE-03 and SYNC-06. "The remote is newer" must be *established*, not assumed
from the fact that the user typed `pull`.

Output: `src/sync/restore/merge.rs`, filled.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/05-pull-and-restore/5-CONTEXT.md
@.planning/phases/05-pull-and-restore/5-01-SUMMARY.md
@CLAUDE.md
@src/sync/model.rs
@src/sync/scope.rs
@src/sync/plan.rs
@src/sync/restore/mod.rs
@src/sync/restore/layout.rs
@src/claude_desktop/merge.rs
</context>

<tasks>

<task type="auto" tdd="true">
  <name>Task 1: `decide` — one pure function, eight dispositions, no filesystem</name>
  <files>src/sync/restore/merge.rs</files>
  <behavior>
    - Local absent → `Create`.
    - Local present, chunk ids equal → `SkipIdentical`, whatever the timestamps say in either direction.
    - Local present, different, local mtime ≤ snapshot time, not a credential → `Update`.
    - Local present, different, local mtime > snapshot time, no force → `SkipLocalNewer` carrying both times.
    - The same with `force` and a non-credential category → `Overwrite` carrying both times.
    - The same with `force` and the credential category → `NeedsCredentialConfirm`, and `force_credentials` set turns that into `Overwrite`; `force` alone never does.
    - A credential that is *not* locally newer is an ordinary `Update` — the second confirmation guards the loss, not the category.
    - Ties: local mtime exactly equal to the snapshot time is **not** newer, so it updates. A test pins the boundary in both directions.
  </behavior>
  <action>
Fill `src/sync/restore/merge.rs` around one pure function, and build `plan` on top of it.

`fn decide(local: Option<&LocalFacts>, remote: &RemoteFacts, category: SyncCategory, opts: &RestoreOptions) -> Disposition`
where `LocalFacts { mtime: DateTime<Utc>, chunk_ids: Vec<ChunkId> }` and
`RemoteFacts { chunk_ids: &[ChunkId], created_at: DateTime<Utc> }`. No `Path`, no `fs`, no clock.
Every branch above is a unit test with no temp directory.

**The remote timestamp is the snapshot's `Root.created_at`, one value for every item.** Write the
reasoning into the module doc, because the next reader will want a per-file mtime and should find
out here why there is not one. `model::FileEntry` carries `path`, `mode`, `true_len`, and the chunk
ids — no mtime — and adding one is a `MANIFEST_VERSION` bump that changes an already-shipping wire
format for a refinement nothing yet needs. `created_at` is when the remote copy was captured, so
"local mtime is after the capture" is precisely "this machine changed it since". The comparison is
made exact rather than merely conservative by plan 5-04 stamping every restored file's mtime to
that same `created_at`: a restored-then-untouched file compares equal, not newer, so the next pull
of a newer snapshot updates it cleanly. Name the upgrade path — a per-file `mtime_ns` under
`MANIFEST_VERSION` 3 — for whoever needs sub-snapshot granularity.

**Digest before timestamp, always.** The identical case is decided by comparing the manifest's
ordered chunk id list against the local file's, and it short-circuits everything else. This is D7:
re-running an interrupted restore must report no conflicts, because it has none. A timestamp check
that ran first would turn every already-restored file into a `SkipLocalNewer` and make the second
run look like a disaster.

**Credentials get the strictest arm.** `SyncCategory::Credentials`, and `SyncCategory::Config`
entries whose file name is `.credentials.json`, are credential-bearing. A locally-newer one under
`force` becomes `NeedsCredentialConfirm`, never `Overwrite`; only `force_credentials` promotes it.
The doc comment says why in one sentence: silently reverting a live rotating OAuth token to a stale
one is a failure this project has already shipped once, in the two-stores-fighting-over-a-refresh-token
form, and a third path into that family is not being built.

**No deletions.** State it in the module doc as a decision: a local file the manifest does not
mention is left exactly as it is. Restore is additive. The `synced.json` baseline that
`claude_desktop::merge` uses to tell a deletion from "never had it" is the right machinery for a
future selective restore (REC-02, deferred to v2) and reaching for it here would build a second
reconciliation model for a case v1 does not have.
  </action>
  <verify>
    <automated>cargo test --lib sync::restore::merge</automated>
  </verify>
  <done>`cargo test --lib sync::restore::merge` is green. `decide` is pure — it takes no `Path`, opens no file, and reads no clock — and every one of the eight dispositions has a test, including both sides of the equal-timestamp boundary. `force` alone never overwrites a locally-newer credential. The module doc records why the remote timestamp is per-snapshot and names the `MANIFEST_VERSION` 3 upgrade path.</done>
</task>

<task type="auto" tdd="true">
  <name>Task 2: `plan` — every manifest entry, including the ones that are refused</name>
  <files>src/sync/restore/merge.rs</files>
  <behavior>
    - A manifest of N entries produces exactly N `ItemPlan`s; a test asserts the count against a manifest containing one of every case.
    - A path `layout::from_manifest_path` rejects becomes `RejectedPath` carrying the reason, with `dest: None`, and is counted in the plan.
    - A path `layout::accept_for_write` refuses becomes `ExcludedByPolicy` with `dest: None` — a bundle naming `bridge-state.json` or anything under `local-agent-mode-sessions/` is dropped on the write side whatever it claims.
    - Local facts are gathered by hashing the local file with the same `Keys::chunk_id` the push side uses, so an identical file is recognised as identical across machines.
    - A local file that cannot be read (permissions, or it is a directory where a file is expected) is `Update` with the io error recorded, not a panic and not a silent skip.
    - A local **symlink** where the manifest names a regular file is `SkipLocalNewer`-shaped refusal with its own message: restore does not follow a link the bundle chose the location of.
    - `packs_needed` and `bytes_to_fetch` on the returned plan count only the items that will actually be written.
  </behavior>
  <action>
Fill `pub fn plan(ctx: &RestoreCtx<'_>, resolved: &Resolved) -> Result<RestorePlan>`.

For each `FileEntry` in the manifest, in manifest order: run
`layout::from_manifest_path(ctx.roots, &entry.path)` and on error emit `RejectedPath` with the
message and `dest: None`. A rejected entry is **kept in the plan** — this is D6's report-not-prompt
applied to tampering. Silently dropping it is how a hostile bundle becomes invisible; the user gets
a line saying an entry was refused and why.

Then `layout::accept_for_write` on the resolved path; a refusal is `ExcludedByPolicy`, also kept.

Gather `LocalFacts` with `std::fs::symlink_metadata` — never `metadata`, which follows a link — so
a symlink planted at a destination is seen as a symlink and refused rather than written through.
For a regular file, hash it in `CHUNK_SIZE` buffers through `keys.chunk_id`, exactly the way
`plan::build` already does on the push side, and compare the ordered id list. Do not consult the
local SQLite index for this: the index is a cache keyed on the *push* side's stat tuple, and
trusting it here would let a stale row decide that a file the user changed is identical. Restore
hashes what is actually on disk. Say that in a comment.

mtime comes from the same `symlink_metadata` call, converted through the crate's existing
`mtime_ns` handling in `scope::push_path` rather than a second conversion.

Roll `packs_needed` and `bytes_to_fetch` from the items that will actually be written — resolve
each item's chunk ids through the `IndexObject` to their distinct packs and sum those packs'
`clen`. A dry run's headline number should be what a real run would fetch, and a number that
counted skipped items would be a lie in the direction that makes the operation look more expensive
and therefore more alarming than it is.
  </action>
  <verify>
    <automated>cargo test --lib sync::restore::merge</automated>
  </verify>
  <done>`cargo test --lib sync::restore::merge` is green. A manifest with one entry of every case produces one `ItemPlan` each and none is dropped. Rejected and excluded entries appear in the plan with `dest: None`. `symlink_metadata` is the only stat call in the file. Local identity is decided by hashing the file on disk, never by the SQLite index. `packs_needed` and `bytes_to_fetch` count only writable items. `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` are clean.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| manifest entry → destination path | Attacker-chosen strings resolved against local roots |
| destination path → local stat | The destination may already hold a symlink, a directory, or a device node |
| local credential → overwrite decision | A live rotating OAuth token can be reverted to a stale one |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|-----------|----------|-----------|----------|-------------|-----------------|
| T-5-21 | Tampering | a hostile manifest path silently dropped | high | mitigate | `RejectedPath` and `ExcludedByPolicy` stay in the plan and are rendered; the count of refused entries is visible rather than absent |
| T-5-22 | Elevation of privilege | a symlink planted at a destination | critical | mitigate | `symlink_metadata` is the only stat call; a symlink where a regular file is expected is refused, not written through |
| T-5-23 | Tampering | reverting a live OAuth token to a stale one | critical | mitigate | Credential-bearing items that are locally newer require `force_credentials`, a second explicit confirmation that `force` alone does not grant (D2) |
| T-5-24 | Tampering | a stale index row deciding a changed file is identical | high | mitigate | Local identity is computed by hashing the file on disk with `Keys::chunk_id`; the SQLite index is never consulted on the restore side |
| T-5-25 | Denial of service | a re-run reporting every restored file as a conflict | medium | mitigate | Digest is checked before any timestamp, so an already-correct item is `SkipIdentical` and D7's second apply is a genuine no-op |
| T-5-26 | Information disclosure | file contents in a disposition message | high | mitigate | `Disposition` carries paths, ids, and timestamps only; no variant holds bytes |
| T-5-SC | Tampering | npm/pip/cargo installs | high | mitigate | No new crates; `cargo machete` runs in the phase gate |
</threat_model>

<verification>
- `cargo test --lib sync::restore::merge` green.
- `grep -vn '^\s*//' src/sync/restore/merge.rs | grep -c 'fs::metadata('` is 0.
- `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` clean.
- `HOME= cargo test --lib sync::restore::merge` passes.
</verification>

<success_criteria>
1. Eight dispositions, each with a test, from one pure function that touches no filesystem.
2. Digest decides identity before any timestamp is read.
3. `force` alone never overwrites a locally-newer credential.
4. Every manifest entry produces exactly one `ItemPlan`, refusals included.
5. Restore plans no deletion, and the module says so as a decision.
</success_criteria>

<output>
Create `.planning/phases/05-pull-and-restore/5-03-SUMMARY.md` when done, recording the disposition
table and the credential-classification rule the report and the CLI both depend on.
</output>
