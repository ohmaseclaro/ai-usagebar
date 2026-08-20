---
phase: 6
plan: 8
subsystem: sync
tags: [sync, push, packer, manifest, corruption, restore, report]
requires: [4-02 packer::build, 5-06 restore::report]
provides: [manifest-describes-what-was-packed, apply-report-says-apply]
affects: [src/sync/push/packer.rs, src/sync/restore/report.rs, src/sync/cli.rs]
status: complete
---

# Phase 6 Plan 8: the manifest describes what was packed, not what was planned

A two-machine run pushed 2.1 GiB, reported success, and produced a snapshot the
receiving machine refused:

```
sync: this snapshot names chunk f94fa134…, which its index does not describe —
refusing rather than restoring a partial tree
```

The refusal was right. `plan::build` hashed each file, and `packer::build`
reopened it minutes later and sealed a block **only when the id it computed was
one the plan had asked for** — while building the manifest entry from the
plan's list. The bundle's contents are live Claude Code transcripts appended to
continuously, so a file changed mid-push has a block whose new id is not in
`wanted`: never sealed, still named. That is the ordinary case for a push that
takes minutes, not a race anyone had to engineer.

## Signature changes (read this first)

Two, both narrow.

1. **`restore::report::render_plan` takes the flag.** Public within the crate:

   ```rust
   pub fn render_plan(plan: &RestorePlan, applying: bool) -> String
   ```

   Call sites: `sync/cli.rs` ×2 (`false` for the dry run, `true` for the
   `--apply` path), `report::confirm_apply` and `report::render_outcome`
   (`false` — both are genuinely reporting a run that wrote nothing), and
   `tests/sync_restore_e2e.rs` ×2. The private `headline` and `footer` take the
   same flag. `report.rs`'s own test module shadows the two-argument form with a
   one-argument `render_plan` that passes `false`, so the sibling colour branch
   sees three changed lines in the renderer rather than twelve in the tests.

2. **`packer::pack_file` — private, one call site.**

   ```rust
   fn pack_file(ctx, packs, path, reusable: &HashMap<ChunkId, ChunkLocation>)
       -> Result<(u32, u64, Vec<ChunkId>)>   // mode, true_len, chunks as read
   ```

   It took `wanted: &HashSet<ChunkId>` and returned `()`.

## The fix

`packer::build` no longer copies the plan's ids into the manifest. Every
`FileEntry` is built from what `pack_file` actually read: the chunk list is the
ids of the blocks it hashed, `true_len` is the bytes it counted, and `mode`
comes from the **open handle's** metadata rather than a separate
`std::fs::metadata` on the path. A block it reads gets sealed unless something
already locates it; a stale planner id that no longer exists simply is not in
the manifest.

`named` — the id set the index object and `referenced_packs` are derived from —
is now the union over the finished `FileEntry` list rather than over
`plan.file_plans`, so the index describes exactly what the manifest names.

### The two consequences, decided

**`true_len` and the chunk list come from one source, always.** On the read
path that source is the read itself. On the short-circuit path (below) it is
the recorded plaintext lengths (`ChunkLocation::plen`) of the very chunks being
named — summed by the new `reused_len`, which returns `None` the moment one of
them is not locatable and so doubles as the short-circuit's precondition. The
old code took `meta.len()` from a stat unrelated to either. A `true_len` that
disagrees with its own chunk list is a restore that aborts at `write.rs:257`.

**Dedup is intact; both halves of it.** `reusable` (published packs the arriving
pointer names) and `packs.holds` (packed by this run) are still what stop a
re-upload, and they are now consulted with the id of the block that was read.
A reusable chunk is skipped, and still appears in the manifest, in the index
object and in `referenced_packs` — `a_second_push_reseals_no_data_chunk_and_
still_names_the_pack_holding_it` asserts exactly that, and now also asserts the
reused entry's `true_len`.

**SYNC-02's "not opened at all" is kept.** A file whose every planned chunk is
already locatable is still not read — otherwise every push would re-read the
whole tree, and a 2 GiB tree is the case this bug came from. The narrow cost is
that such a file is described as of the plan, not as of the pack. That snapshot
is *stale* but internally consistent: every chunk it names is in a pack the
snapshot references. `packs.holds` alone no longer buys the short-circuit
(it carries no length), so a file whose chunks were first packed by an earlier
file in the same run is now read; that costs a read and nothing else.

### Which guarantee this buys

**Guaranteed: the manifest never names a chunk nothing sealed.** Every id in
every `FileEntry` is either in a pack this bundle publishes or in a pack a
published snapshot already names.

**Not guaranteed, deliberately: a torn read.** A file written *while* the packer
reads it yields a prefix of one version and a tail of the next — a plausible,
internally consistent snapshot of a growing file. That is what any backup of a
live file does, and it restores.

## The second fix: `sync pull --apply` no longer calls itself a dry run

The plan phase always runs with `apply: false` (that is how the report exists at
all), so the report could not know `--apply` had been given: it printed
`DRY RUN`, then `To apply it, run: ai-usagebar sync pull --apply` — the command
the user had just run. `render_plan` now takes the flag; under `--apply` the
header reads `RESTORING`, the fetch line reads `will fetch`, the footer reads
`Nothing has been written yet — this is the plan --apply is about to run.`, and
the closing line is omitted. Three conditionals in the renderer, no
restructuring — the sibling colour branch was left room.

## Tests

- `a_file_appended_to_between_planning_and_packing_names_only_chunks_it_sealed`
- `a_file_truncated_between_planning_and_packing_names_only_chunks_it_sealed`

  Both plan a tree, change the file on disk **after** planning and before
  packing, and assert the real invariant directly: every chunk id the manifest
  names is present in some pack the bundle publishes — read back through
  `read_header`/`blob_bytes`/`Manifest::open`, not from the builder's own
  bookkeeping. Both fail on the pre-fix code with *"the manifest names a chunk
  of claude-home/history.jsonl that no pack in this bundle holds"*; verified by
  reverting the entry construction and re-running.
- `a_second_push_reseals_no_data_chunk_and_still_names_the_pack_holding_it` —
  extended with the reused path's `true_len` and chunk list. Unchanged tree,
  two packs (manifest + index object), no data chunk re-sealed.
- `an_apply_report_names_neither_a_dry_run_nor_the_command_already_given`.

All hermetic: `TempDir`, the fixture's parked-port client, fixed `NOW`, cheap
KDF parameters.

## Call sites of everything added

| Added | Production call sites |
|---|---|
| `packer::reused_len` | 1 — `packer::build` |
| `packer::pack_file` (changed) | 1 — `packer::build` |
| test helper `manifest_of` | test-only; 3 tests |
| test helper `chunk_bytes` | test-only; `manifest_of` |
| test helper `assert_every_named_chunk_was_packed` | test-only; 2 tests |
| test shadow `report::tests::render_plan` | test-only; 12 tests |

Nothing added is uncalled.

## Also touched

Two doc comments (`index.rs::bind_to`, and a test doc in `plan.rs`) described the
key-change corruption as *"the packer seals a block only when the id it computes
is one the plan asked for"*. That mechanism no longer exists; both now say so and
say what the binding still buys — not re-reading every file.

## Verification

- `cargo test` — **1733 passed, 0 failed, 16 ignored** (baseline 1730 + 3 new).
- `cargo clippy --all-targets -- -D warnings` — clean.
- `cargo fmt --check` — clean.
- `make test` — cargo suite plus the GNOME, KDE and Omarchy frontend contract
  suites, all green.
- `Cargo.toml` and `Cargo.lock` untouched; no crate added.

Run with `< /dev/null`: `sync::cli`'s tests hang on an inherited open pipe.
