---
phase: 04-push-packs-atomic-flip-gc-rekey
plan: 04
type: execute
wave: 2
depends_on: ["4-01"]
files_modified:
  - src/sync/push/pointer.rs
autonomous: true
requirements: [REPO-07, SYNC-04]
must_haves:
  truths:
    - "The pointer `PUT` always carries the `sha` the read returned, so two machines pushing concurrently cannot interleave (REPO-07)."
    - "A 409 re-reads the remote pointer, rebuilds this run's snapshot record on top of what is now there, and retries **once** — then reports rather than looping (D3)."
    - "The rebuild merges: the competing machine's snapshot records survive the retry, and no pack either pointer references is dropped from the list."
    - "A second 409 on the retry fails with an actionable message telling the user another machine is pushing and to re-run."
    - "A first push sends no `sha` at all, and a `sha` of null is never sent in its place."
    - "Nothing in this file deletes anything; a losing race costs a retry, never remote data (SYNC-04)."
  artifacts:
    - src/sync/push/pointer.rs — `load`, `commit` with its bounded conflict path, and the merge rule
  key_links:
    - "`commit` is the single linearization point of the whole format; every other step is invisible to a reader"
    - "The `rebuild` closure comes from the orchestrator, so the merge rule lives here and the ordering lives there — neither plan edits the other's file"
    - "A losing race never triggers a delete: prune runs after `commit` returns, against the pointer that landed"
---

<objective>
Make the compare-and-swap real. The pointer `PUT` is the only commit point in the format, so its
conflict behaviour is the difference between two machines converging and one silently erasing the
other's snapshot.

Implements **REPO-07** (the CAS precondition) and the conflict half of **D3** and **SYNC-04**.

Purpose: a 409 means another machine won the race, and its pointer may reference packs this run
is about to prune. Blind-overwriting is the one action that turns a race into data loss.
Output: `pointer::commit` with a bounded, merging retry.
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
@src/sync/github/http.rs
</context>

<tasks>

<task type="auto" tdd="true">
  <name>Task 1: The bounded, merging compare-and-swap</name>
  <files>src/sync/push/pointer.rs</files>
  <behavior>
    - A first push against a mock 404 on the read sends a `PUT` whose body carries no `sha` field at all — asserted by matching the request body, not by matching a null.
    - A subsequent push sends exactly the `sha` the read returned.
    - A `PUT` answered with 409 triggers exactly one re-read and exactly one further `PUT`, carrying the `sha` from the re-read.
    - After a 409 whose re-read shows a snapshot record this run has never seen, the retried body contains both that record and this run's — the competitor is not erased.
    - After a 409, the retried pointer's snapshot list is still ordered oldest to newest and still no longer than `keep_snapshots`.
    - A second 409 on the retry returns an error naming another machine and telling the user to re-run; no third attempt is made.
    - A `PUT` answered with 401, 403, or 404 is returned unchanged with Phase 3's actionable text and is not retried.
    - `load` refuses a pointer whose `format` is above the ceiling, and one whose `repo_id` is not the caller's own, before either reaches a merge.
  </behavior>
  <action>
Plan 4-01 created this file with `load` complete and `commit`'s no-conflict path working. Fill the
conflict path and the merge rule; do not change either signature, and do not touch the
orchestrator that supplies the closure.

`commit` takes `rebuild: Fn(Option<&Pointer>) -> Result<Pointer>` precisely so the conflict path
is a loop over *the same* function rather than a special case. The sequence:

1. Call `rebuild(current)` and `PUT` the result with `sha`.
2. On anything that is not `GithubError::Conflict`, return it unchanged. Phase 3's `classify`
   already produced the right variant and `actionable` already has the right text; a second layer
   of interpretation here would just make two messages for one failure.
3. On `Conflict`: re-`load` the pointer, call `rebuild` again with what is now there, and `PUT`
   once with the new `sha`.
4. On a second `Conflict`: stop. Return an error saying another machine is pushing to the same
   repository and to re-run in a moment. **Do not loop.** Two machines retrying without bound
   against each other is a livelock that burns the content-creation budget and never converges,
   and the human retry is a perfectly good backoff.

The shared `with_retry` helper deliberately does **not** retry a `Conflict`, which is what leaves
this bounded retry as the only path — do not add a second one, and do not relax the helper.

**The merge rule, which is the part that has to be right.** `rebuild` produces a pointer from
whatever the remote currently holds, and the orchestrator's implementation of it appends this
run's `SnapshotRecord` to the remote's list. Constrain it here, in `commit`'s doc comment and by a
test, so a future edit to the closure cannot silently break the invariant:

- Snapshot records the caller did not produce are **carried forward**, never dropped. The
  competing machine's snapshot references packs that exist; discarding its record makes those
  packs unreferenced, which makes prune delete them, which strands the other machine's backup.
  This is the exact stranding D2's ordering rule exists to prevent, arriving through a different
  door.
- Truncation to `keep_snapshots` drops from the **oldest** end only, and it happens inside the
  pointer being written, which means the snapshot record is removed by the flip itself — strictly
  before any pack is deleted, because pack deletion happens after `commit` returns. D2's mandatory
  ordering is therefore structural rather than a step someone has to remember.
- The `keyfile` field is taken from the remote's current value unless this run is the one changing
  it. A push that overwrote the keyfile name with a stale one would point every future reader at
  an asset that a rekey has already deleted.

Serialize with a struct whose `sha` carries `skip_serializing_if`, so a first push omits the field
rather than sending it as null — those are different requests to the Contents API, and only the
first means "create, and fail if it exists". Plan 4-01 froze `put_contents` this way; the test
here asserts it end to end from `commit`.

Nothing in this file deletes anything. A losing race costs one extra round trip; it must never
cost remote data.

The commit message on the `PUT` is a fixed, non-identifying string naming the tool and the
snapshot counter. It lands in the repository's git history in the clear forever, so it carries no
path, no hostname, no user name, and no byte count.

Every test drives `mockito::Server::new_async()` with both `Endpoints` fields pointed at
`server.url()` and a fixed `now`.
  </action>
  <verify>
    <automated>cargo test --lib sync::push::pointer</automated>
  </verify>
  <done>`cargo test --lib sync::push::pointer` is green. A 409 produces exactly one re-read and one retry; a second 409 produces an actionable error and no third attempt. A competing snapshot record present at the re-read is present in the retried body. A first push's body carries no `sha` field. Nothing in the file issues a delete. `load`'s version and `repo_id` refusals still hold.</done>
  <reversibility rating="one-way">Getting the merge rule wrong deletes another machine's backup, and the deletion happens on that machine's next prune rather than here, so it would surface long after this code ran. The carry-forward rule comes from `github-transport.md` §5.2 and D3 and is not to be relaxed.</reversibility>
  <precondition>Plan 4-01 is merged: `pointer::load`, `pointer::commit`'s signature and no-conflict path, `Pointer`, `SnapshotRecord`, `write::{get_contents, put_contents}`, and `GithubError::Conflict` all exist as `4-01-SUMMARY.md` records them.</precondition>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|---|---|
| remote pointer → merge input | The competing machine's snapshot list is attacker-controlled when the remote is hostile, and merged into what this machine publishes |
| this machine's pointer → remote | The single linearization point; whatever lands here is what every reader resolves through |
| commit message → permanent git history | Contents-API writes create commits that are never garbage-collected |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|---|---|---|---|---|---|
| T-4-28 | Tampering | a competing snapshot record dropped by the merge | critical | mitigate | Records the caller did not produce are carried forward, asserted by a test that plants an unseen record at the re-read and checks the retried body; dropping one makes prune strand the other machine's backup |
| T-4-29 | Tampering | a blind overwrite of a newer pointer | critical | mitigate | Every `PUT` carries the `sha` from the read that preceded it, and `with_retry` refuses to retry a `Conflict`, so the only path past a 409 is the re-read-and-rebuild here |
| T-4-30 | Denial of service | unbounded conflict retry between two machines | medium | mitigate | Exactly one retry, then an actionable error; a human re-run is the backoff |
| T-4-31 | Spoofing | a pointer from another bundle merged in | high | mitigate | `load` refuses a `repo_id` that is not the caller's own before the merge sees it, and every sealed root inside carries the reader's `repo_id` as associated data |
| T-4-32 | Tampering | a stale `keyfile` name republished over a rekeyed one | high | mitigate | The `keyfile` field is carried from the remote's current value unless this run is the one changing it |
| T-4-33 | Denial of service | an oversized or malformed pointer body | medium | mitigate | `MAX_POINTER_BYTES` and the `format` probe before deserialization, both from 4-01's `load`, which this plan does not relax |
| T-4-34 | Information disclosure | the commit message in permanent git history | medium | mitigate | A fixed string plus the snapshot counter — no path, hostname, user name, or byte count, and Contents-API commits are never collectable |
| T-4-SC | Tampering | dependency surface | low | accept | Zero new crates. `Cargo.toml` is not in `files_modified` |
</threat_model>

<verification>
- `cargo test --lib sync::push::pointer` is green.
- `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` are clean.
- The file issues no delete request of any kind.
- No test sleeps, opens a socket outside the mockito base, or reads a real `$HOME` or token.
- `src/sync/github/write.rs` and `src/sync/push/mod.rs` are unchanged by this plan.
</verification>

<success_criteria>
Two machines pushing concurrently converge: the loser re-reads, rebuilds on top of the winner's
pointer, and publishes a list containing both snapshots. A second collision reports rather than
loops. No packs are dropped from the pointer and nothing is deleted on either path.
</success_criteria>

<output>
Create `.planning/phases/04-push-packs-atomic-flip-gc-rekey/4-04-SUMMARY.md` when done.

Record the merge rule in full — what is carried forward, what is truncated and from which end,
and how the `keyfile` field is chosen — because plan 4-05 depends on the pointer it receives
already being the truncated one, and plan 4-06 depends on the `keyfile` rule.
</output>
</content>
