---
phase: 04-push-packs-atomic-flip-gc-rekey
plan: 04
subsystem: transport
tags: [compare-and-swap, conflict, merge-rule, bounded-retry, pointer, hermetic-tests]

requires:
  - phase: 04-push-packs-atomic-flip-gc-rekey
    plan: 01
    provides: "`pointer::load`, `pointer::commit`'s signature and no-conflict path, `Pointer`, `SnapshotRecord`, `write::put_contents`, `GithubError::Conflict`, `gate::Pushing`"
  - phase: 03-github-auth-and-the-private-repo-gate
    plan: 03
    provides: "`http::classify`, `http::actionable`, `with_retry`'s refusal to retry a `Conflict`"
provides:
  - "`pointer::commit`'s bounded, merging conflict path — one re-read, one rebuild against the winner, one further `PUT`, then report"
  - "The three merge rules recorded in `commit`'s doc comment and each driven by a test"
  - "`pointer::is_conflict` — 409 and the 422 `put_contents` maps onto it arrive as one thing"
affects: [4-05, 4-06]

tech-stack:
  added: []
  patterns:
    - "A conflict discriminated on `AppError::Http { status: 409, .. }` rather than by widening a frozen signature to leak `GithubError` — `From<GithubError>` already maps `Conflict` there, and `put_contents` already folds its 422 in."
    - "The rebuild closure is re-invoked per attempt, never captured-and-reused: the second call is what makes the retry a merge instead of a clobber."
    - "One mockito `match_request` recorder per server — mockito evaluates every mock's matcher against every request that clears method and path, so a second recorder double-records."

key-files:
  created: []
  modified:
    - src/sync/push/pointer.rs
    - src/sync/cli.rs

requirements-completed: [REPO-07, SYNC-04]

duration: 1h
completed: 2026-08-19
status: complete
---

# Phase 4 / Plan 04: The Bounded, Merging Compare-and-Swap

**A losing race now costs one round trip instead of another machine's backup.**
`pointer::commit`'s 409 arm re-reads the remote, re-runs `rebuild` against the
pointer that is *actually current*, and `PUT`s once more with the fresh `sha`. A
second collision reports and stops. Nothing in the file deletes anything, and a
test scans the whole file to keep that true.

## Task commits

1. `11d9e2d` — the 409 arm, the 422 that is one, and the bound on retries (RED)
2. `0323ea3` — the bounded, merging compare-and-swap (GREEN)

## Signatures changed

**None.** `pointer::load` and `pointer::commit` are byte-identical to the
signatures 4-01 froze, including `permit: &gate::Pushing` by reference and
`rebuild: F where F: Fn(Option<&Pointer>) -> Result<Pointer>`. `write.rs` and
`push/mod.rs` were not touched.

Two private helpers were added inside `pointer.rs`: `is_conflict(&AppError)` and
`put(...)` (serialize → size-check → `PUT`, factored out because it now has two
call sites).

## The merge rule, as tested

`rebuild` is written in **4-01's orchestrator, `src/sync/push/mod.rs`** — this
plan does not own that file. The three rules are recorded verbatim in `commit`'s
doc comment and each is driven here by `rebuild_like_the_orchestrator`, a test
closure reproducing them, so an edit to the real closure that breaks one stops
this file's assertions from describing it.

1. **Carry forward.** Every snapshot record the caller did not produce survives
   the rebuild. `a_conflict_re_reads_once_rebuilds_on_the_winner_and_retries_once`
   plants a record named `competitor` at the re-read and asserts it is present
   both in the returned pointer and in the retried body on the wire. Dropping it
   would make its packs unreferenced, which makes the next prune delete them,
   which strands that machine's backup (T-4-28).
2. **Truncate from the oldest end only**, to `keep_snapshots`, **inside the
   pointer being written**. `the_retried_pointer_is_still_oldest_first_and_still_capped_at_keep`
   drives a re-read of `[a, b, c]` with `keep = 2` and asserts both the returned
   pointer and the transmitted body are `[c, new]`. Because truncation happens in
   the body that is `PUT`, the snapshot record is removed by the flip itself —
   strictly before any pack is deleted, since deletion happens after `commit`
   returns. D2's mandatory ordering is structural, not a step to remember.
3. **The `keyfile` field comes from the pointer that arrived**, not from local
   state, unless this run is the one changing it — and only `rekey` is.
   `rule_three_takes_the_keyfile_from_the_pointer_that_arrived` covers both the
   carried case and the first-push case, where there is nothing to carry.

**Plan 4-05** receives an already-truncated pointer from `commit`; **plan 4-06**
depends on rule 3.

## Two invariants preserved

- **`commit` returns the pointer that went to the remote**, never the local
  candidate. On the retry path it returns `merged`, built on the winner. This is
  what lets `prune::run` be handed the pointer that *landed* — pruning against a
  candidate after a lost race would delete the winner's packs.
- **`load`'s two refusals still hold and still gate the merge.** The re-read goes
  through `load`, passing `next.repo_id` (this machine's own, copied by the
  closure from local configuration), so a pointer belonging to a different bundle
  is refused before the merge can see it (T-4-31). The version probe is unchanged
  and remains at-or-below `MAX_SUPPORTED_POINTER`, never an equality check.

## How a conflict is recognised

`GithubError::Conflict` converts to `AppError::Http { status: 409, .. }` via
`http::From<GithubError>`, and `write::put_contents` already maps its 422 — a
`sha`-less `PUT` against a path that already exists — onto `Conflict` at the one
call site that knows it omitted the `sha`. So both arrive at `commit` as the same
`AppError` and take one path, and no second layer of interpretation was added
here: anything that is not a conflict is returned **unchanged**, carrying Phase
3's `actionable` text.

`is_conflict` matches on that variant rather than widening a frozen signature to
leak `GithubError` out of `write.rs`.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 — Stale test] `src/sync/cli.rs`'s flip test asserted the pre-4-04 behaviour**

- **Found during:** Task 1, at the full-suite run.
- **Issue:** `a_failed_pointer_put_exits_non_zero_and_makes_no_second_attempt`
  (written by 4-01, when `commit` had no 409 arm) asserted `expect(1)` on the
  pointer `PUT`. Adding the bounded retry that this plan exists to add makes that
  two `PUT`s, so the test failed on a behaviour change it was not written to
  describe.
- **Fix:** `.expect(1)` → `.expect(2)`, renamed to
  `a_failed_pointer_put_exits_non_zero_after_exactly_one_bounded_retry`, and its
  doc comment now says what is still true — `with_retry` never retries a
  conflict; the one re-drive is `pointer::commit`'s, and it stops there. The test
  still asserts a non-zero exit and still asserts an exact `PUT` count, so it
  remains a bound on retries rather than a licence for them.
- **Files modified:** `src/sync/cli.rs` (two lines plus the comment).
- **Note:** this is the one file outside `src/sync/push/pointer.rs` this plan
  touched. It was unavoidable: the assertion is false by construction once the
  409 arm exists. Sibling plans editing `cli.rs` should expect a trivial conflict
  at exactly this hunk.
- **Commit:** `0323ea3`

**2. [Rule 3 — Test-infrastructure trap, fixed inline] mockito evaluates every mock's `match_request`**

`RemoteMock::matches` is `method && path && headers && body && request_matcher`,
and `handle_request` evaluates it for **every** mock rather than stopping at the
first hit. Attaching a body recorder to both the 409 mock and the 201 mock
therefore recorded each request twice, and the first draft's `bodies[1]` was the
first attempt's body seen through the second mock. Fixed by using exactly one
recorder per server; the reason is written into the helper's doc comment so it
does not get re-added.

## Threat Flags

None. No new network surface, no new persisted state, and no new dependency —
`Cargo.toml` and `Cargo.lock` are unchanged.

## Known Stubs

None.

## Verification

```
cargo test --lib sync::push::pointer      15 passed, 0 failed
cargo test --lib sync::                   all green
cargo test --lib                          1306 passed, 0 failed, 0 ignored   (baseline 1300)
cargo clippy --all-targets -- -D warnings clean
cargo fmt --check                         clean
Cargo.toml / Cargo.lock                   unchanged
```

Six tests added, all in `src/sync/push/pointer.rs`:

| Test | What fails if it regresses |
|---|---|
| `a_conflict_re_reads_once_rebuilds_on_the_winner_and_retries_once` | exactly one re-read and one further `PUT`, the fresh `sha` on the retry, and the competitor's record on the wire (T-4-28, T-4-29) |
| `the_retried_pointer_is_still_oldest_first_and_still_capped_at_keep` | rule 2 across the retry, asserted on the transmitted body (D2) |
| `a_second_conflict_names_another_machine_and_makes_no_third_attempt` | the bound — two `PUT`s, one re-read, an actionable error (T-4-30) |
| `a_422_on_a_sha_less_put_takes_the_same_re_read_and_retry_path` | the first-push-with-lost-local-state case, including "the first body carries no `sha` field at all" |
| `a_401_403_or_404_is_returned_unchanged_and_never_re_read` | Phase 3's text surviving, and no re-read after a terminal status (the GET mock is `expect(0)`) |
| `nothing_in_this_file_issues_a_delete_request` | SYNC-04 as a property of the file; needles assembled at runtime so the scan covers the test itself |

- No test sleeps, opens a socket outside its `mockito` base, reads a real `$HOME`
  or `$XDG` path, or touches a real token — every one builds its `Client` from an
  injected `Endpoints` with both fields pointed at `server.url()`, and every
  time-dependent call takes the fixed `NOW`.
- `grep -rn 'Utc::now' src/sync/push/pointer.rs` — no hits.
- `src/sync/github/write.rs` and `src/sync/push/mod.rs` are unchanged.

## Self-Check: PASSED

- `src/sync/push/pointer.rs` — FOUND (modified)
- `src/sync/cli.rs` — FOUND (modified)
- commit `11d9e2d` — FOUND
- commit `0323ea3` — FOUND
