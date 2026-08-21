---
phase: 05-pull-and-restore
plan: 02
subsystem: sync/restore
tags: [restore, fetch, ceilings, rollback-anchor, content-address, threat-model]
status: complete
requires:
  - "sync::restore::{RestoreCtx, RestoreOptions, Resolved, PackSource} (5-01, frozen)"
  - "sync::push::pointer::load, pack_asset_name, RELEASE_TAG, SnapshotRecord, RemoteIndexEntry (Phase 4)"
  - "sync::anchor::accept (Phase 1)"
  - "sync::model::{Root, Manifest, IndexObject}, sync::pack::read_header (Phase 1)"
provides:
  - "sync::restore::fetch::resolve — hardened; signature UNCHANGED"
  - "the four ceilings: MAX_MANIFEST_CHUNKS, MAX_INDEX_CHUNKS, MAX_PACKS_PER_RESTORE, MAX_RESTORE_BYTES"
affects:
  - "nothing — no cross-module type or signature moved. 5-03…5-07 are unaffected."
tech-stack:
  added: []
  patterns:
    - "a ceiling is derived from a measured quantity in docs/, names the observed value on refusal, and has a test that reaches it"
    - "two ceilings over two different resources (requests, transfer), neither implying the other"
    - "a tie in an authenticated field breaks on other authenticated fields, never on plaintext list order"
    - "every adversarial fixture is one mutation of a bundle the push side really produced"
key-files:
  created: []
  modified:
    - src/sync/restore/fetch.rs
decisions:
  - "MAX_PACKS_PER_SNAPSHOT (4096) renamed to MAX_PACKS_PER_RESTORE (512) — the bound is on one restore, across all three download rounds, not on what one snapshot may reference"
  - "snapshot selection breaks a tied counter on the root's sealed created_at then the sealed bytes; a tie is reachable and was letting the plaintext list's order decide (T-5-15)"
  - "Range: not implemented — CAL-1 still unrun; the landing spot is named in the module doc"
metrics:
  duration: ~55 min
  completed: 2026-08-19
---

# Phase 5 Plan 02: The verified chain — Summary

## Signature changes: NONE

`pub async fn resolve(ctx: &RestoreCtx<'_>, local_anchor: Option<&Anchor>) -> Result<Resolved>`
is byte-identical to 5-01's frozen form. No cross-module type moved, no field
was added to anything in `restore/mod.rs`, and `src/sync/restore/fetch.rs` is
the only file this plan touched. **Nothing here obliges 5-03…5-07 to change a
line.**

One internal constant was renamed — `MAX_PACKS_PER_SNAPSHOT` →
`MAX_PACKS_PER_RESTORE` — and it is private to this module.

---

## The four ceilings, and what each was derived from

| Constant | Value | Derived from | Bounds |
|---|---|---|---|
| `MAX_MANIFEST_CHUNKS` | 128 | `docs/sync-format.md` §5's **measured** sizing: 1,600 files = 2 chunks, 5,700 files = 5, at a representative 229 bytes/entry. 128 chunks ≈ 146,000 files, an order of magnitude past the largest bundle ever measured. | `Root.manifest_chunks` |
| `MAX_INDEX_CHUNKS` | 256 | `MAX_RESTORE_BYTES / CHUNK_SIZE` = 49,152 chunks in the largest bundle this build will fetch, one ~180-byte JSON index entry each ⇒ ~8.4 MiB ⇒ ~34 chunks. 256 is that with room. | the pointer's `index_chunks` bootstrap |
| `MAX_PACKS_PER_RESTORE` | 512 | A bound on **requests**. At `PACK_TARGET` a legitimate 512-pack snapshot is 16 GiB of stored data. | pack count, cumulative across all three rounds |
| `MAX_RESTORE_BYTES` | `256 * PACK_MAX` (12 GiB) | A bound on **transfer**, written as the arithmetic. | declared pack bytes, cumulative |

Two of these are deliberately not one ceiling. `MAX_PACKS_PER_RESTORE` does not
bound transfer (512 one-byte packs cost 512 round trips and no bytes);
`MAX_RESTORE_BYTES` does not bound requests (512 packs at `PACK_MAX` is 24 GiB,
twice the byte ceiling). Whichever binds first, binds, and **each refusal names
both the observed value and the ceiling**, so a user who legitimately outgrows
one gets a number to raise instead of a mystery.

Previously: `MAX_MANIFEST_CHUNKS` and `MAX_INDEX_CHUNKS` were both 8192 and
`MAX_PACKS_PER_SNAPSHOT` was 4096 — round numbers with no derivation, and at
`PACK_MAX` the pack ceiling permitted a 192 GiB download.

`MAX_MANIFEST_CHUNKS`'s comment says why it is the one that matters: the list is
safe *inside* the root's authenticated plaintext, but a reader consumes it to
decide **how many fetches to issue** — a decision made from a list before the
objects it names have authenticated anything. That is the exact shape Phase 1's
handoff flagged as living in restore (T-5-10).

## The tie nobody had broken (T-5-15)

Selection was `root.counter > best.counter`, which on a **tie** kept whichever
record the pointer listed first — so the plaintext list's order decided the
snapshot after all, which is precisely what T-5-15 says it must not.

The tie is not hypothetical: plan 4-08 is remediating a push-side bug where the
snapshot counter was computed before the compare-and-swap and never recomputed,
so two machines racing can publish **distinct** snapshots under one counter.

Selection now breaks the tie on the root's own sealed `created_at`, and then on
the sealed root bytes. Both are authenticated — a tampered root never opened —
so the tie-break is a deterministic total order that the remote cannot steer.
`two_snapshots_at_one_counter_resolve_the_same_way_in_either_order` runs both
list orders and asserts they agree.

## What else changed in `resolve`

- The pack ceilings moved onto the pointer's own `record.packs` list **and** into
  `fetch_packs`, where they are cumulative across the three download rounds and
  checked before the round's first byte is requested. The round-3 duplicate
  count check was deleted: a second copy is only something to diverge.
- `fetch_packs` deduplicates *within* a round as well as against already-held
  packs, so a repeated id is one download rather than a `holds_pack` miss.
- The pack-size refusal now names the declared size and `PACK_MAX`.
- The module doc names the `Range:` optimisation as the CAL-1-gated thing it is,
  and says exactly what would change if the measurement ever came back positive
  (a byte-range fetch keyed on `PackEntry`'s `offset`/`clen`, and nothing else in
  the file). It is not implemented on an assumption.

## The unauthenticated integers, and why they are inert

`RemoteIndexEntry` carries `offset`, `clen` and `true_len` in the clear. This
module reads **only** `.pack` and `.id` from it; the slicing offsets come from
each pack's own sealed header, which `pack::read_header` already bounds against
the pack's real length. Asserted rather than claimed:
`the_pointers_unauthenticated_offsets_never_index_into_anything` sets every one
of them to `u64::MAX`/`u32::MAX` and the restore still resolves.

`anchor::accept` is called with the **opened root's** `repo_id` and `counter`,
and this module persists nothing — `restore::run` step 7 does, after `resolve`
returns `Ok`. `resolve_persists_nothing` asserts the anchor file does not exist
after a successful resolve. The `repo_id`-mismatch arm is not routed around under
`allow_rollback`, and there is a test for that under the flag. The anchor path
comes from `ctx.anchor_path` and is never derived from the remote's claimed
`repo_id`, honouring `anchor.rs`'s module-doc constraint.

## Tests — 24 added, all in `src/sync/restore/fetch.rs`

Every remote is seeded by calling the **push** side (`push::packer::build`) and
serving the result through `mockito`; each adversarial case is then **one
mutation** of that real bundle — a flipped byte, a withheld chunk, a re-sealed
root, a renamed asset. A hand-written fixture would agree with a broken reader.

Bootstrap half: happy path with exactly one `list_assets`; empty snapshot list;
snapshot ceiling refused before any fetch (asserted as a zero-hit listing mock);
absent keyfile named; wrong passphrase giving the existing error **verbatim**
(`assert_eq!`, so an elaboration fails the test); a root sealed for another
bundle refused without naming the other bundle; selection by sealed counter in
both list orders; the tied-counter case; a lower counter naming
`--allow-rollback` and then accepted under it; `repo_id` mismatch refused under
`allow_rollback`; nothing persisted; a missing release as "nothing pushed yet";
an oversized root string refused before decoding.

Ceilings and packs: each of the four ceilings refused with the observed value and
the ceiling in the message; a tampered pack refused with "does not hash to that
name" **before** its header is read; a chunk opened under another chunk's id
failing while the same bytes open under their own; a 1,000-file bundle whose
2-chunk manifest has its last chunk withheld yielding no `Manifest` at all; an
orphan manifest chunk named in the refusal; every asset downloaded exactly once
across all three rounds (mockito hit counts); the same chunk twice giving
identical plaintext and no second request.

Hermetic: no real `$HOME`/`$XDG` (`SyncRoots::at` into a `TempDir` on both the
pushing and restoring side), no wall clock (`NOW` is a fixed const and `now` is a
`RestoreCtx` field), no network beyond the injected `Endpoints` base. The one
other base in the file is `http://127.0.0.1:1`, injected into the *push* client
so that a regression making `packer::build` dial would fail rather than reach
anything real — the 5-01 precedent.

Cheap Argon2 parameters (`m_kib: 8, t: 1, p: 1`) throughout: the AUR `check()`
runs these on an installer's machine.

## Deviations from Plan

### [Rule 1 — correctness] The tied-counter tie-break

Not in the plan's task list. The plan's own must-have says selection is by the
sealed counter "so a reordered list is inert", and with a tie it was not — the
plaintext list's order decided. Given 4-08's finding that ties are reachable, a
selection that quietly depends on remote-controlled ordering is a bug in the
stated property, not a new feature. Two lines and a test.

### `MAX_RESTORE_BYTES` derived as `256 * PACK_MAX`, not `MAX_PACKS_PER_RESTORE * PACK_MAX`

The plan specified "the byte ceiling from `MAX_PACKS_PER_RESTORE` times
`pack::PACK_MAX`". Written that way the byte ceiling is **unreachable** — the
count check refuses at 513 packs, so the sum can never exceed 512 × `PACK_MAX` —
and an unreachable check is a check nobody can trust and nobody can test. The two
constants now bound two different resources (requests, transfer), each is reached
by its own test, and the arithmetic is still written out in the constant.

### Ceiling values are smaller than the plan's "generous headroom" framing implies

`MAX_MANIFEST_CHUNKS` went from 8192 to 128 rather than up. 8192 was not headroom
over a measured quantity; it was a round number. 128 is the measured 5 chunks with
an order of magnitude on the *file count*, which is what the plan asked the
derivation to come from.

### Not done here

`docs/sync-format.md` §11 remains 5-08's. The `<verification>` grep
`upload_asset|put_contents|delete_asset` returns **0** and is recorded below
rather than added as a self-scanning test — the file is the only thing that could
break it and the phase gate runs the grep.

## Threat Flags

None. Nothing in this plan introduced a network endpoint, an auth path, a file
access pattern, or a schema at a trust boundary; the diff removes reach rather
than adding it.

## Verification

```
cargo test                                   1412 lib passed, 0 failed   (baseline 1388, +24)
                                             1453 total passed, 0 failed (baseline 1429, +24)
cargo test --lib sync::restore                 47 passed, 0 failed
HOME= XDG_CACHE_HOME= XDG_CONFIG_HOME=
  cargo test --lib sync::restore               47 passed, 0 failed
cargo clippy --all-targets -- -D warnings    clean
cargo fmt --check                            clean
git diff Cargo.toml Cargo.lock               empty — no new crates
grep -v '^\s*//' fetch.rs
  | grep -c 'upload_asset|put_contents|delete_asset'   0
grep 'Utc::now|std::env::var|temp_dir()|"/tmp"|home_dir' fetch.rs   none
git status --short                           clean; one file modified in the commit
```

`cargo machete` is not installed on this machine; `Cargo.toml` is byte-identical,
so no dependency could have become unused.

## Self-Check: PASSED

- `src/sync/restore/fetch.rs` — present, modified, the only source file in the diff.
- Commit `015bbe7` — in `git log` on `gsd/5-02`.
- `git diff --diff-filter=D HEAD~1 HEAD` — no deletions.
