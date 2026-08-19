---
phase: 04-push-packs-atomic-flip-gc-rekey
plan: 05
type: execute
wave: 2
depends_on: ["4-01"]
files_modified:
  - src/sync/push/prune.rs
autonomous: true
requirements: [SYNC-07]
must_haves:
  truths:
    - "After repeated syncs of a growing file, the asset list shrinks: remote size tracks live data rather than cumulative history (SYNC-07)."
    - "Retention keeps the newest `keep_snapshots` records, from `[sync] keep_snapshots`, defaulting to 10 (D1)."
    - "The snapshot record is gone before any pack is deleted — structurally, because the record is dropped by the flip and deletion happens only after `commit` returns (D2)."
    - "Only assets referenced by **no** surviving snapshot are deleted, computed from the pointer that landed, so a concurrent push's packs can never be collected (D2)."
    - "A prune failure is a warning on a successful push, never a push failure — and it is an `Option` on the outcome rather than an `Err`, so no later edit can make it fatal (D2)."
    - "The keyfile asset and any asset whose name this build does not recognise are never deleted."
    - "`sync prune` runs the same code on demand and reports what it removed."
  artifacts:
    - src/sync/push/prune.rs — the pure liveness computation, the delete pass, and the on-demand entry point
  key_links:
    - "`prune::run` is handed the pointer that **landed**, not the one this run built — that is what makes deleting a competitor's pack impossible rather than merely unlikely"
    - "`plan_deletions` is pure and takes a pointer plus an asset list, so every retention and safety rule is testable without a server"
    - "`Index::forget_chunks` keeps the local index from claiming a chunk still lives in a pack that has been deleted"
---

<objective>
Stop the remote growing without bound, without ever being the reason a backup becomes
unrestorable.

Implements **SYNC-07** and **D1**/**D2**: keep the newest ten snapshots, delete only what no
surviving snapshot references, run automatically after a successful flip, and never take a
successful push down with you.

Purpose: superseded tail chunks are the main garbage source, so pruning is a correctness
requirement rather than housekeeping — and the one operation in this project that can destroy a
user's data.
Output: `prune::run`, a pure `plan_deletions`, and the on-demand `sync prune` body.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/04-push-packs-atomic-flip-gc-rekey/4-CONTEXT.md
@.planning/phases/04-push-packs-atomic-flip-gc-rekey/4-01-SUMMARY.md
@.planning/research/github-transport.md
@docs/sync-format.md
@CLAUDE.md
@src/sync/push/mod.rs
@src/sync/github/write.rs
@src/sync/index.rs
</context>

<tasks>

<task type="auto" tdd="true">
  <name>Task 1: What is live, computed from the pointer alone</name>
  <files>src/sync/push/prune.rs</files>
  <behavior>
    - `plan_deletions` given a pointer whose two surviving snapshots reference packs A and B, and an asset list holding A, B, C, and the keyfile, returns exactly C.
    - A pack referenced by only the **oldest** surviving snapshot is retained; sharing is what makes ten snapshots cheap.
    - The asset named by the pointer's `keyfile` field is never returned, even though no snapshot's `packs` list names it.
    - An asset whose name matches neither the pack shape nor the keyfile shape is never returned — an unrecognised asset is somebody else's, and a garbage collector that deletes what it does not understand is not one.
    - A pointer with more than `keep_snapshots` records is truncated from the oldest end, and the packs freed by the truncation appear in the deletion list.
    - A pointer with zero snapshots returns an empty deletion list rather than proposing to delete everything.
    - `plan_deletions` is pure: no client, no clock, no filesystem.
  </behavior>
  <action>
Write `pub fn plan_deletions(pointer: &Pointer, assets: &[Asset], keep: usize) -> (Pointer, Vec<u64>)`
returning the truncated pointer and the asset ids to delete. Pure, so every rule below is testable
against a table rather than a server.

Truncate `pointer.snapshots` from the **oldest** end down to `keep`. Then the live pack set is the
union of `packs` across every **surviving** record. An asset is deletable when its name parses as
a pack name whose id is not in that set. Everything else is retained, and the retention list is
deliberately generous:

- The asset named by `pointer.keyfile` — no snapshot's `packs` names it, and deleting it makes the
  entire bundle permanently unreadable. It is the single worst thing this function could do.
- Any asset whose name matches neither the pack shape nor the keyfile shape. It might be a future
  version's object, or something the user attached by hand. A collector that deletes what it does
  not recognise turns every format addition into a data-loss bug.

Guard the empty case explicitly: a pointer with no snapshots yields no deletions. The union of an
empty set is empty, which read naively says "everything is garbage", and that arithmetic is how a
first-push race or a hand-edited pointer would wipe a release.

**The ordering rule is structural, and say so in the doc comment.** D2 is absolute that the
snapshot record must be deleted before any pack, because the reverse can leave a live snapshot
pointing at a deleted pack — an unrestorable backup, the worst outcome this feature can produce.
That ordering is not a step in this function: the truncated pointer is published by the flip, and
this function's deletion list is acted on only after `commit` has returned. The record is
therefore always gone from the remote before the first `DELETE` is issued. Write that down here so
nobody later "optimises" the delete pass to run in parallel with the flip.
  </action>
  <verify>
    <automated>cargo test --lib sync::push::prune</automated>
  </verify>
  <done>`plan_deletions` is pure and has a test for every rule in the behaviour block, including the empty-pointer case and the keyfile exclusion. A pack shared with the oldest surviving snapshot is retained.</done>
  <reversibility rating="one-way">This function decides what to destroy on the user's remote. A wrong retention rule deletes a pack a live snapshot needs, and there is no undo — the bytes are gone from a release asset, which is the whole reason Release assets were chosen over git objects.</reversibility>
</task>

<task type="auto" tdd="true">
  <name>Task 2: The delete pass, and the warning that never fails a push</name>
  <files>src/sync/push/prune.rs</files>
  <behavior>
    - `run` against a mock listing three assets, one of them unreferenced, issues exactly one delete and returns 1.
    - A delete answered with 404 counts as success — the asset is already gone, which is what was asked for.
    - A delete answered with 403 leaves the remaining deletes unattempted, returns an error, and deletes nothing further.
    - The error `run` returns is rendered by the caller as a warning and the push still exits 0 — asserted at the CLI level, where the exit code lives.
    - `forget_chunks` is called with exactly the packs actually deleted, never with the ones that were only planned.
    - `run` issues no delete at all when the pointer it was handed is not the one that landed — enforced by taking the committed pointer as its only source of truth, so the wrong pointer cannot be supplied by accident.
  </behavior>
  <action>
Fill `prune::run(ctx: &PushCtx<'_>, release_id: u64, landed: &Pointer, keep: usize) -> Result<usize>`,
whose signature plan 4-01 froze, returning the number of assets deleted.

`landed` is the pointer `pointer::commit` returned — the one that is actually on the remote — and
never the one this run built. That is the whole mitigation for the race D2 warns about: if
another machine won the flip, `landed` is *its* pointer, its snapshot records are in the list, and
its packs are consequently live. Deleting a competitor's pack stops being unlikely and becomes
impossible. Name that in the parameter's doc comment.

`list_assets` once, call `plan_deletions`, then delete sequentially. Sequential rather than
concurrent, deliberately: the whole set is a handful of requests, deletion is the one irreversible
operation here, and a partial failure that stops at the first error leaves a comprehensible state.
Stop at the first error and return it with however many succeeded already counted — leaving a few
extra packs costs storage, and D2 says storage is what a prune failure is allowed to cost.

A 404 on a delete is success; `write::delete_asset` already treats it that way.

After the pass, call `Index::forget_chunks` with the packs that were **actually** deleted, so the
local index stops claiming those chunks are present. Pass only the confirmed ones: a chunk row
that survives a failed delete is correct, and one that is dropped for a pack still on the remote
just costs a re-upload.

Add the on-demand entry point behind `sync prune`, which plan 4-01 wired into the CLI: load the
pointer, publish the truncated one through `pointer::commit` — because on-demand pruning must drop
the records in the same ordered way a push does, through the same compare-and-swap — then run the
delete pass against what landed. Report what was removed. The on-demand path returns a real
non-zero exit on failure; it is the push path, not this one, where a failure is only a warning.

This function never returns success while having failed. `PushOutcome.prune_warning` being an
`Option` rather than an `Err` is 4-01's encoding of D2, and the correct behaviour here is to
return the error honestly and let the orchestrator decide it is not fatal.
  </action>
  <verify>
    <automated>cargo test --lib sync::push::prune</automated>
  </verify>
  <done>`cargo test --lib sync::push::prune` is green. `run` deletes only the unreferenced assets, stops at the first failure, and calls `forget_chunks` with the confirmed set. The on-demand path publishes the truncated pointer through the compare-and-swap before deleting anything. A test drives a two-snapshot pointer where the newest belongs to a competing machine and asserts none of that machine's packs is proposed for deletion.</done>
  <precondition>Plan 4-01 is merged: `PushCtx`, `Pointer`, `SnapshotRecord`, `prune::run`'s signature, `pointer::commit`, `write::{list_assets, delete_asset}`, and the `[sync] keep_snapshots` key all exist as `4-01-SUMMARY.md` records them.</precondition>
  <precondition>`Index::forget_chunks` exists. It is plan 4-02's task 1. If 4-02 has not merged, implement the delete pass without the index call and record the omission rather than adding the method to `index.rs`, which this plan does not own.</precondition>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|---|---|
| remote pointer → deletion decision | The list that decides what survives is attacker-controlled when the remote is hostile |
| remote asset list → deletion decision | Asset names are attacker-controlled and are parsed to decide whether an object is ours |
| process → remote delete | The only irreversible remote operation in the project |
| deleted packs → local index | The index must stop claiming a chunk lives in a pack that no longer exists |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|---|---|---|---|---|---|
| T-4-35 | Tampering | deleting a pack a live snapshot references | critical | mitigate | Liveness is the union over **surviving** records of the pointer that landed; a test plants a competing machine's snapshot and asserts none of its packs is proposed |
| T-4-36 | Tampering | deleting the keyfile asset | critical | mitigate | The `keyfile` name is excluded explicitly, with its own test; deleting it makes the bundle permanently unreadable and there is no recovery by design |
| T-4-37 | Tampering | an empty or truncated pointer read as "everything is garbage" | critical | mitigate | The zero-snapshot case returns an empty deletion list explicitly, rather than falling out of an empty union |
| T-4-38 | Tampering | deleting an object this build does not recognise | high | mitigate | Only names matching the pack shape are deletable; anything unrecognised is retained, so a future format addition cannot be collected by an older build |
| T-4-39 | Elevation of privilege | pruning before or during the flip | critical | mitigate | `run` takes the committed pointer, which only exists after `commit` returns; the snapshot record is therefore always gone from the remote before the first delete, satisfying D2's mandatory order structurally |
| T-4-40 | Denial of service | a prune failure taking down a successful push | high | mitigate | `run` returns its error honestly and the orchestrator maps it to `PushOutcome.prune_warning`, an `Option` rather than an `Err`, so the exit code stays 0 (D2) |
| T-4-41 | Denial of service | unbounded deletion against a hostile asset list | medium | mitigate | `list_assets`' page cap from 4-01 bounds the input; deletion is sequential and stops at the first error |
| T-4-42 | Repudiation | the local index claiming a deleted pack still holds chunks | medium | mitigate | `forget_chunks` is called with the confirmed deletions only; the failure direction costs a re-upload, never a missing chunk |
| T-4-SC | Tampering | dependency surface | low | accept | Zero new crates. `Cargo.toml` is not in `files_modified` |
</threat_model>

<verification>
- `cargo test --lib sync::push::prune` is green.
- `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` are clean.
- `plan_deletions` takes no client, no clock, and no path, so every retention rule is proven
  without a server.
- No test sleeps, opens a socket outside the mockito base, or reads a real `$HOME` or token.
- `src/sync/index.rs`, `src/sync/push/mod.rs`, and `src/sync/github/write.rs` are unchanged by this
  plan.
</verification>

<success_criteria>
Repeated syncs of a growing file leave the release holding only the packs the surviving snapshots
reference, and the asset list shrinks. A concurrent push's packs are never collected. A prune that
fails leaves a successful push successful, with a warning naming what was not cleaned up.
</success_criteria>

<output>
Create `.planning/phases/04-push-packs-atomic-flip-gc-rekey/4-05-SUMMARY.md` when done.

Record the full retention rule and the exclusion list, and state plainly how D2's
record-before-pack ordering is enforced — it is structural, not sequential, and the next reader
must not "simplify" it back into an ordered pair of steps.
</output>
</content>
