# Phase 4 Context — Push: Packs, Atomic Flip, GC, Rekey

**Decisions locked by the orchestrator.** This is the first phase that writes to the remote and
the first that can *delete* remote data, so the irreversible choices are pinned here.

## Locked decisions

### D1 — Retention: keep the last 10 snapshots

`[sync] keep_snapshots = 10` (config, not a constant). Rationale: the user syncs from a working
machine, so the realistic recovery need is "undo the last few syncs", not archival history.
Ten covers roughly a week of daily syncs with room to spare, while keeping the pack set small
enough that pruning stays cheap. Old snapshots are cheap anyway — they share chunks — so the
cost of 10 over 3 is small and the safety margin is real.

### D2 — Pruning is automatic after a successful push, but never blocks it

After the pointer flip succeeds, if snapshot count > `keep_snapshots`, drop the oldest snapshot
records and delete any pack no longer referenced by a surviving snapshot. A prune failure is a
**warning, not a push failure** — the push already succeeded and the user's data is safe;
leaving a few stale packs costs storage, not correctness. `sync prune` runs it on demand.

**Order is mandatory and non-negotiable:** delete the snapshot *record* first, then unreferenced
packs. The reverse order can strand a live snapshot pointing at a deleted pack — an
unrestorable backup, which is the single worst outcome this feature can produce.

### D3 — The flip is the only commit point

1. Upload every new pack as a Release asset. Packs are content-addressed and immutable, so a
   half-finished upload set is inert — nothing references it yet.
2. Verify each uploaded asset is retrievable and its digest matches.
3. **Only then** `PUT` the snapshot pointer through the Contents API with the `sha`
   precondition (compare-and-swap).

An interruption at any point before step 3 leaves the remote exactly as it was, satisfying
SYNC-04. A `409`/precondition failure at step 3 means another machine pushed first: re-read
the remote snapshot, re-reconcile, retry once, then report rather than loop.

### D4 — Resume reuses what already landed

Before uploading, list existing assets and skip packs already present with a matching digest.
This is what makes SYNC-05 fall out of the design instead of needing separate machinery: the
content-addressed naming means "already uploaded" is a question with an exact answer.

### D5 — Rekey re-wraps, and genuinely destroys the old wrapper

Changing the password re-wraps the master key and uploads a new keyfile. It then **deletes the
old keyfile asset**. This is the concrete payoff of choosing Release assets over git objects:
asset deletion actually removes the bytes, whereas a git object would survive in history and
make "password change" a comforting lie rather than a real control. The research flagged this
exact caveat; our store choice resolves it, so the deletion must actually happen and be
verified, not merely attempted.

Data packs are untouched by a rekey — that is CRYPTO-04's "without re-uploading the entire
bundle".

### D6 — Progress reporting

A first push moves ~115 MB in a handful of large assets, so per-asset progress (asset i of n,
bytes uploaded/total) is enough; no per-chunk chatter. Non-TTY output degrades to periodic
lines, never a spinner, so the menu bar's subprocess capture stays readable.

### D7 — Rate-limit discipline

Packs are ≤1.9 GiB and few, so the 80/min content-creation limit is not a practical risk — but
honour `Retry-After` and the `x-ratelimit-*` headers anyway, with bounded exponential backoff.
Never retry a `4xx` that is not `403`-with-rate-limit; a `401` retried is just a slower failure.

## Constraints inherited from the codebase

- Atomic local writes for the index and any state file (tempfile + persist), as
  `cache::atomic_write` already does.
- Tests hermetic: the entire push path must be testable against `mockito` with no real network
  and no real token. Live verification goes in `tests/live.rs` behind `#[ignore]`.
- The widget must still exit 0 regardless of sync state.
- Errors surface the vendor's message via the existing `sanitize_untrusted_field` path —
  remote-controlled strings are untrusted input, exactly as vendor API errors already are.

## Security note for the audit (2g)

This phase introduces the first outbound data path and the first destructive remote operation.
Worth hunting specifically: a pointer flip that could publish a snapshot referencing packs that
failed verification; a prune that could race a concurrent push from another machine and delete
a pack the *new* snapshot references; and any path where a rekey reports success while the old
keyfile asset survives.

---

## Risk propagated from Phase 1 verification

**The pack header is still single-chunk, and its slack is a function of `PACK_TARGET`.**

Gap-closure 1-09 removed the single-chunk size limit from manifests and index objects, but it
did not reach the **pack header** (`pack.rs`), which still seals through `chunk::seal_chunk` and
therefore keeps the `CHUNK_SIZE` ceiling.

That is sound *today*: a 32 MiB pack of 256 KiB chunks holds ~128 entries against a limit in the
thousands, and an oversized header errors cleanly rather than truncating.

**But if CAL-1 comes back positive and this phase raises `PACK_TARGET`, the entry count rises
with it and that ceiling must be re-checked.** It is the one place 1-09's fix deliberately did
not reach. Raising the pack target without re-examining the header limit would reintroduce
exactly the failure 1-09 was written to remove — this time on the object that names every chunk
in the pack.

The upgrade path, if needed, is a format-2 multi-chunk header via the same
`chunk::seal_all`/`reassemble` pair the manifest now uses.
