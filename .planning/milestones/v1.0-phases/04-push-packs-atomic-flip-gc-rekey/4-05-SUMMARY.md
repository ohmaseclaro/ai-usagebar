---
phase: 04-push-packs-atomic-flip-gc-rekey
plan: 05
subsystem: transport
tags: [prune, retention, sync-07, d1, d2, prune-grace, landed-pointer, orphan-keyfile, hermetic-tests]

requires:
  - phase: 04-push-packs-atomic-flip-gc-rekey
    plan: 01
    provides: "`PushCtx`, `Pointer`, `SnapshotRecord`, `PRUNE_GRACE`, `pack_asset_name`, `keyfile_asset_name`, `pointer::{load, commit}`, `write::{Asset, list_assets, delete_asset, ensure_release}`, `gate::Pushing`, `[sync] keep_snapshots`"
  - phase: 04-push-packs-atomic-flip-gc-rekey
    plan: 02
    provides: "`Index::forget_chunks` — the reason this plan is in wave 3"
  - phase: 04-push-packs-atomic-flip-gc-rekey
    plan: 06
    provides: "the orphan-keyfile gap, found in `rekey.rs` and handed here"
provides:
  - "`plan_deletions` — the whole retention rule, pure"
  - "`prune::run` — the delete pass, plus `forget_chunks` on the confirmed set"
  - "`prune::run_on_demand` — the whole of `ai-usagebar sync prune`, filled"
  - "The orphan-keyfile sweep 4-06 could not close in its own file"
affects: [phase-5-restore, phase-6-readme]

tech-stack:
  added: []
  patterns:
    - "One truncation rule, in one function, reused by the on-demand rebuild closure rather than restated in it."
    - "A recogniser that returns an enum of the two shapes this build writes, so 'unrecognised' is the fall-through arm rather than a rule someone has to remember to add."
    - "A sibling `#[cfg(test)] mod` for the mock-server half, sharing the pure half's fixtures through `pub(super)`, so the table-driven tests stay readable."

key-files:
  created: []
  modified:
    - src/sync/push/prune.rs

requirements-completed: [SYNC-07]

duration: 1h
completed: 2026-08-19
status: complete
---

# Phase 4 / Plan 05: Retention, and the one destructive pass

**Ten snapshots survive, everything no surviving snapshot references and older
than a day is deleted, and there is no expressible way to run the pass against
a pointer that did not land.** The keyfile a pointer names is excluded under
every snapshot shape, an asset this build does not recognise is somebody
else's, and an empty snapshot list proposes nothing rather than proposing
everything.

## Task commits

1. `dadd6d4` — the retention rules, as failing tests
2. `ef367ad` — what is live, computed from the pointer alone
3. `498a7cc` — the delete pass and the on-demand entry, as failing tests
4. `3c27374` — the delete pass, and the on-demand entry point

## The retention rule, in full

`plan_deletions(pointer, assets, keep, now, grace) -> (Pointer, Vec<u64>)`.
Pure: no client, no filesystem, no clock. `now` and `grace` are parameters, so
every rule below is proven against a table.

1. `pointer.snapshots` is truncated from the **oldest** end down to `keep`.
   `keep` is clamped to at least 1 — config refuses `keep_snapshots = 0`, but the
   truncated pointer this returns is *published* by `run_on_demand`, so a zero
   arriving by any other route must not empty the snapshot list.
2. The live pack set is the union of `packs` across every **surviving** record.
3. An asset is deletable only when all three hold:
   - its name is one this build writes — `pack-<64 hex>.bin` or
     `keyfile-<64 hex>.json`, with the affixes exact and the id parsed as 64 hex
     characters;
   - no surviving record names it. For a pack that is its id's absence from the
     live set; for a keyfile it is not being the one `pointer.keyfile` names;
   - `now - asset.created_at > grace`, where `grace` is `PRUNE_GRACE` = **24
     hours**, computed from `Asset.created_at` — the field 4-01 put on `Asset`
     for exactly this.

### The exclusion list, and why each one is there

| Excluded | Why |
|---|---|
| The asset named by `pointer.keyfile` | No snapshot's `packs` list names it, so the naive rule collects it. The wrapped master key inside is the only route to the data, for every machine, permanently. Tested by name (`the_keyfile_the_pointer_names_is_never_deleted`) across three snapshot shapes — none, one empty record, two records — at an age of a year. |
| Anything younger than `PRUNE_GRACE` | The only cover for a competitor mid-push. See below. Both sides of the boundary are asserted: at exactly 24 h it survives, at 24 h + 1 s it goes. |
| A name matching neither shape | It might be a future version's object, or something the user attached by hand. Nine near-misses are asserted retained, including `pack-<hex>` with no extension, `pack-<hex>.bin.bak`, a non-hex id, and a 32-character id. |
| Every asset, when no snapshot survives | The union of an empty set is empty, which read naively says "everything is garbage". That arithmetic is how a first-push race or a hand-edited pointer would wipe a release. |

## D2's ordering — structural for this machine, and it covers only this machine

**The record is gone before any pack is deleted, and that is not a step
anywhere in this file.** The truncated pointer is published *by the flip*:
`push::run`'s rebuild closure truncates inside the pointer it writes, and
`run_on_demand` puts `plan_deletions`' truncated pointer through the same
`pointer::commit`. Deletion happens only after `commit` has returned. The record
is therefore always off the remote before the first `DELETE` is issued. **Nobody
may later "optimise" the delete pass to run in parallel with the flip**, and
nobody should rewrite this into an ordered pair of statements — the ordering is
a consequence of where the two operations sit, not of their sequence in a
function body.

**Say just as plainly what that does not buy.** It orders *this machine's* record
against *this machine's* deletes. It says nothing about a snapshot another
machine has not published yet: machine 2 uploads pack `P` and has not flipped;
machine 1 commits, prunes, sees `P` referenced by no record in the pointer that
landed, and deletes it; machine 2 then flips a pointer naming `P`. A live
snapshot pointing at deleted data, D2's single worst outcome, with neither
machine doing anything wrong.

**Two independent guards, both required, neither substituting for the other:**

- **`landed`.** `prune::run` takes the pointer `pointer::commit` returned, never
  the one this run built. If a competitor won the compare-and-swap, `landed` is
  *its* pointer, its records are in the list, and its packs are consequently
  live. This closes the **committed** competitor (T-4-35), and there is no
  expressible way to hand `run` the local candidate: the only value with that
  type comes out of `commit`.
- **`PRUNE_GRACE`.** The age floor closes the **in-flight** competitor
  (T-4-35b), which the landed pointer cannot see by definition. The cost is that
  genuine garbage lingers a day; the alternative is an unrestorable backup.

## The extra task from 4-06: the orphan keyfile

4-06 found that an interrupted first push, or a rekey against a bundle with no
published pointer (its local-only arm 3), can leave a **keyfile asset no pointer
names** — an old password still opens it. It could not close it in `rekey.rs`,
because closing it there meant calling `ensure_release`, which *creates* a
release, on a bundle that has none, purely to run a delete.

It is closed here, under exactly the same two rules as packs: the same grace
window and the same landed pointer. `an_orphan_keyfile_no_pointer_names_is_swept_like_a_pack`
plants three keyfiles — the published one, an old orphan, a two-hour-old orphan
— and asserts only the middle one is proposed.

The **live** keyfile is excluded first and unconditionally, and that exclusion is
what
`the_keyfile_the_pointer_names_is_never_deleted` exists to pin. Sweeping
keyfiles is the one change here that widens what this function may destroy, so
its guard is the one tested most.

## The delete pass

`run(ctx, release_id, landed, keep, permit)`: `list_assets` once, `plan_deletions`,
then delete **sequentially**. Sequential deliberately — the whole set is a handful
of requests, deletion is the one irreversible operation in this crate, and
stopping at the first error leaves a state a human can read. A 404 is success:
the asset is already gone, which is what was asked for, and `write::delete_asset`
already treats it that way.

`Index::forget_chunks` is then called with the packs **actually** deleted —
never the planned ones — and it is called whichever way the pass ended. A chunk
row that survives a failed delete is correct; one dropped for a pack still on the
remote costs a re-upload, which is the safe direction. The remote's refusal is the
more important error, so if both a delete and the local bookkeeping failed, the
refusal is what is returned.

This matters more than it looks: 4-02's chunk table records what this machine
**packed**, not what **landed**, and reuse intersects it with the arriving
pointer. A pack deleted here is by construction absent from the pointer that
landed, so reuse would already skip it — but the row is still a claim this
machine cannot honour, and dropping it keeps the two sources of truth from
drifting.

**A prune failure is a warning on a successful push, never an `Err` that reaches
the user's exit code.** `run` returns its error honestly; `push::run` maps it to
`PushOutcome.prune_warning`, an `Option` — 4-01's encoding of D2 — and
`cli.rs`'s `a_prune_failure_is_a_warning_line_and_the_push_still_exits_zero`
asserts the exit code at the level where the exit code lives. Nothing here can
make it fatal, because nothing here reaches the exit code.

## `run_on_demand`

Gate → load → (nothing published ⇒ `Ok(0)`) → `ensure_release` → `commit` the
truncated pointer → the delete pass against what landed.

- **The gate is first**: `gate_now` runs `fetch_facts` + `check_drift` +
  `assert_pushable` + `spend`, the same sequence the push path uses. A public
  repository is refused with exactly one request in the trace — `GET /repos/o/n`
  — and the asset listing mock is at `expect(0)` (T-4-42b).
- **The pointer is loaded before `ensure_release`**, so a bundle with nothing
  published never *creates* a release purely to run a delete. That is precisely
  what 4-06 declined to do, and the same reasoning applies from this side.
- The rebuild closure calls `plan_deletions` for the truncation rather than
  restating it, so there is exactly one truncation rule in the crate. Its `None`
  arm errors — the pointer disappearing mid-prune must not be answered by writing
  one with an empty snapshot list.
- A failure here **is** a failure; the user asked for exactly this.

## Deviations from the plan

**1. [Additive, handed over by 4-06] Keyfile assets are swept, not merely
excluded.** The plan's rule was "only names matching the pack shape are
deletable". The coordinator's brief adds the orphan-keyfile sweep, so
`asset_kind` returns an enum of the two shapes and the keyfile arm is deletable
exactly when the name is not `pointer.keyfile`. Every other guard — grace,
landed pointer, empty-snapshot — applies to it unchanged.

**2. [Rule 2] `keep` is clamped to at least 1.** `plan_deletions`' truncated
pointer is published by `run_on_demand`, so `keep = 0` would publish an empty
snapshot list — the bundle's records gone, from a maintenance command. Config
already refuses zero; this is the second, structural refusal, at the one place
both callers route through.

**3. [Rule 2] `run_on_demand` returns `Ok(0)` when no pointer is published**
rather than creating a release. Nothing is published, so nothing is proven
garbage, and `ensure_release` is a write that would leave a release behind where
there was none. Consequence, stated so Phase 5 knows: an orphan keyfile on a
bundle that has *never* published a pointer is not swept. There is no pointer to
sweep it against, and inventing the liveness set locally is exactly the
"everything is garbage" arithmetic the empty-snapshot guard exists to refuse.

**4. [Contract, inherited] Built against `4-01-SUMMARY.md`, not the plan text.**
`run` takes `permit: &gate::Pushing` and `run_on_demand` takes no `release_id`,
as 4-01's deviation 3 recorded.

## Security properties, and how each is enforced

| Property | Enforcement |
|---|---|
| T-4-35 — deleting a **committed** competitor's pack | `run` takes `landed`, whose only producer is `pointer::commit`. Test plants a two-record pointer whose newer record is a competitor's and asserts its pack's `DELETE` mock is at `expect(0)` while the genuine garbage still goes — with every asset well past the grace window, so the age floor is provably not doing the work. |
| T-4-35b — deleting an **in-flight** competitor's pack | `now - created_at > grace`, `grace` = `PRUNE_GRACE` = 24 h. Four ages asserted across the boundary, including exactly 24 h (retained) and 24 h + 1 s (deleted). `the_pass_uses_the_shared_grace_window` pins that the module does not re-declare the constant. |
| T-4-36 — deleting the keyfile | Excluded by name, before any other rule can reach it. Its own test, by name, across three snapshot shapes at an age of a year. |
| T-4-37 — an empty pointer read as "everything is garbage" | The zero-surviving-snapshot case returns early with an empty list, explicitly, rather than falling out of an empty union. |
| T-4-38 — deleting an object this build does not recognise | `asset_kind` returns `None` for anything that is not exactly `pack-<64 hex>.bin` or `keyfile-<64 hex>.json`, and `None` is retained. Nine near-misses asserted. |
| T-4-39 — pruning before or during the flip | `run`'s only pointer source is `commit`'s return value, which cannot exist before the flip. On the on-demand path a trace assertion pins `PUT` strictly before `DELETE`, and the published body is decoded and compared against the expected truncation. |
| T-4-40 — a prune failure taking down a successful push | `run` returns the error; `push::run` puts it in `prune_warning`, an `Option`. The exit code is asserted in `cli.rs`, which already had that test. |
| T-4-41 — unbounded deletion against a hostile asset list | 4-01's page cap bounds the input; deletion is sequential and stops at the first error, asserted with the second `DELETE` mock at `expect(1)` and the first at `expect(1)`. |
| T-4-42b — pruning against a repository that is no longer private | `gate_now` first. Trace is exactly `["GET /repos/o/n"]` and the listing mock is at `expect(0)`. |
| T-4-42 — the local index claiming a deleted pack still holds chunks | `forget_chunks` with the confirmed set. Test seeds three packs' rows — deleted, refused, live — and asserts only the deleted one's rows are gone. |
| T-4-SC — dependency surface | Zero new crates. `Cargo.toml` and `Cargo.lock` are byte-identical to the branch point. |

## Verification

```
cargo test --lib sync::push::prune          17 passed, 0 failed          (0.07s)
cargo test --lib sync::                    368 passed, 0 failed
cargo test --lib                          1350 passed, 0 failed, 0 ignored   (baseline 1333)
cargo clippy --all-targets -- -D warnings   clean
cargo fmt --check                           clean
Cargo.toml / Cargo.lock                     unchanged
git diff --stat                             src/sync/push/prune.rs only (+96 −17)
```

- `grep -c 'Utc::now\|SystemTime::now\|Instant::now\|std::env::var' src/sync/push/prune.rs`
  → **0.** `now` and the grace window are parameters everywhere.
- No test reads a real `$HOME`/`$XDG` path, an environment variable, the
  network, or the wall clock: a `TempDir` holds the index, `mockito` holds the
  remote, `NOW` is a fixed timestamp, and nothing sleeps.
- The one keyfile a test creates uses `MIN_KDF_MEMORY_KIB`, `t = 1`, `p = 1` —
  never production parameters, because the AUR `check()` runs these on an
  installer's machine.
- `src/sync/index.rs`, `src/sync/push/mod.rs` and `src/sync/github/write.rs` are
  unchanged by this plan, as its verification block required.
- The REPO-03 guard and 4-01's two write-path guards still pass.

## Not done here

- **`STATE.md`, `ROADMAP.md` and `REQUIREMENTS.md` were not touched.** Sibling
  plans are executing in parallel worktrees against the same lines; the
  coordinator owns them after the merge. SYNC-07 is complete and ready to be
  checked off.
- An orphan keyfile on a bundle that has never published a pointer — see
  deviation 3.

## Self-Check: PASSED

- `src/sync/push/prune.rs` — FOUND
- `dadd6d4`, `ef367ad`, `498a7cc`, `3c27374` — all FOUND in `git log`
