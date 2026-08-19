---
phase: 04-push-packs-atomic-flip-gc-rekey
plan: 07
type: execute
wave: 3
depends_on: ["4-01", "4-02", "4-03", "4-04", "4-05", "4-06"]
files_modified:
  - tests/sync_push_e2e.rs
  - docs/sync-github.md
  - README.md
autonomous: true
requirements: [SYNC-04, SYNC-05, SYNC-07, REPO-06, REPO-07]
must_haves:
  truths:
    - "Each of ROADMAP §Phase 4's seven success criteria has a named test in one integration file, so the phase's claim is a command rather than a review."
    - "A ~5,000-chunk bundle completes in under ten HTTP requests in total (REPO-06)."
    - "Killing the run after the uploads and before the flip leaves the previous pointer byte-identical, and re-running uploads only what was missing (SYNC-04, SYNC-05)."
    - "A stale-`sha` 409 re-plans, and no asset the competing pointer references is deleted (REPO-07)."
    - "Repeated syncs of a growing file leave the asset list smaller than the cumulative history (SYNC-07)."
    - "The whole protocol is exercised against `mockito` with no real token, no real `$HOME`, and no network."
    - "The user-facing document says what push does, what prune deletes, that a password change is not revocation, and that a multi-gigabyte bundle rewritten often is the profile GitHub's acceptable-use policy warns about."
  artifacts:
    - tests/sync_push_e2e.rs — one test per ROADMAP success criterion, each named after it
    - docs/sync-github.md extended with push, prune, retention, and rekey
    - a README pointer to it
  key_links:
    - "This is the only place the modules five parallel worktrees built are exercised together; a unit-green phase with an unwired protocol is exactly what an integration test catches"
    - "The no-new-object-kind claim is asserted here rather than reviewed — a grep-shaped test over the sealing call sites"
---

<objective>
Prove the protocol, not the parts. Five wave-2 plans each shipped one file green in isolation;
this plan drives the whole nine-step push against `mockito` and turns ROADMAP §Phase 4's seven
success criteria into seven named tests. Then it tells the user what the commands do and what they
cost.

Implements the observable halves of **SYNC-04**, **SYNC-05**, **SYNC-07**, **REPO-06**, and
**REPO-07**, and documents **D2**, **D5**, and the acceptable-use risk the research surfaced.

Purpose: the failure this catches is the one unit tests structurally cannot — modules that each
pass alone and do not compose.
Output: `tests/sync_push_e2e.rs`, and the user-facing half of `docs/sync-github.md`.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/04-push-packs-atomic-flip-gc-rekey/4-CONTEXT.md
@.planning/phases/04-push-packs-atomic-flip-gc-rekey/4-01-SUMMARY.md
@.planning/phases/04-push-packs-atomic-flip-gc-rekey/4-02-SUMMARY.md
@.planning/phases/04-push-packs-atomic-flip-gc-rekey/4-03-SUMMARY.md
@.planning/phases/04-push-packs-atomic-flip-gc-rekey/4-04-SUMMARY.md
@.planning/phases/04-push-packs-atomic-flip-gc-rekey/4-05-SUMMARY.md
@.planning/phases/04-push-packs-atomic-flip-gc-rekey/4-06-SUMMARY.md
@.planning/research/github-transport.md
@docs/sync-format.md
@docs/sync-github.md
@CLAUDE.md
</context>

<tasks>

<task type="auto" tdd="true">
  <name>Task 1: The seven success criteria, as seven named tests</name>
  <files>tests/sync_push_e2e.rs</files>
  <behavior>
    - A first push of a bundle whose plan holds ~5,000 chunks issues one upload per pack and fewer than ten HTTP requests in total, counted from the mock.
    - A run whose pointer `PUT` is made to fail leaves the previously stored pointer byte-identical, and every pack the stored pointer names is still present and in the uploaded state.
    - Re-running that killed push uploads only the packs that were missing or in a non-uploaded state, deletes the zombie, and reaches the same final pointer.
    - A stale-`sha` 409 on the pointer `PUT` re-reads, re-plans, and lands a pointer containing both machines' snapshot records; a prune immediately afterwards deletes none of the competing machine's packs.
    - A rekey against the same fixture leaves every pack asset byte-identical and the old keyfile asset absent.
    - Twelve pushes of a growing file leave the release holding fewer assets than the cumulative number ever uploaded, and every asset the final pointer names is present.
    - A push driven with a recording progress reporter emits advancing asset and byte counts, and every failure path returns a non-zero exit with a message that names an action.
  </behavior>
  <action>
Create `tests/sync_push_e2e.rs`. Give each test the name of the ROADMAP success criterion it
proves, so a reader comparing the roadmap to the suite can do it by eye — the phase's claim
becomes a command rather than a review.

Build one helper that seeds a `TempDir` tree, constructs `SyncRoots::at`, an `Index::at`, a
keyfile at **cheap** KDF parameters, a `mockito::Server` with both `Endpoints` fields pointed at
it, and a fixed `now`. Every test drives the real orchestrator through that helper. Never
production KDF parameters and never a real path: the AUR `check()` runs `cargo test` during
`makepkg` on installers' machines.

For criterion 1, the request count is the assertion. Reach ~5,000 chunks by generating the bytes
rather than writing a gigabyte to disk — the arithmetic is identical and every installer pays the
difference.

For criteria 2 and 3, simulate the kill by making the mock fail the pointer `PUT`, then re-drive
the orchestrator against a mock seeded with exactly the state the first run left. Assert the
stored pointer is byte-identical, then assert the second run's upload set is the complement of
what landed. That pair is SYNC-04 and SYNC-05 and is the most important test in the file.

For criterion 4, seed the mock so the first `PUT` answers 409 and the re-read returns a pointer
carrying a snapshot record this run has never seen. Then run prune and assert none of that
record's packs was deleted.

For criterion 6, loop a growing file through twelve pushes against `keep_snapshots` of 10 and
compare the final asset count against the cumulative number of distinct pack names ever uploaded.
The claim is that remote size tracks live data, so measure both numbers rather than asserting a
constant.

Add one test that is not a ROADMAP criterion but is a Phase 1 carry-forward: assert that the only
kinds of object sealed under `chunk_key` anywhere in `src/sync/push/` are the format's existing
four. The cheap version that actually catches a regression is a test reading the push module's own
sources through `include_str!` and checking that each sealing call site names one of the four
known kinds. Keep the check crude and the failure message loud, and have that message say what to
do: introducing a fifth kind is the trigger for the deferred AAD object-type separator, which is a
versioned format change and not an edit.

Add a second such test asserting `pack::PACK_TARGET` still reads 32 MiB, with a message pointing at
the single-chunk pack-header ceiling that tracks it and at 4-02's worst-case header test.

Nothing in this file reads a real `$HOME`, a real token, the real Keychain, or the network.
  </action>
  <verify>
    <automated>cargo test --test sync_push_e2e</automated>
  </verify>
  <done>`cargo test --test sync_push_e2e` is green. Seven tests are named after ROADMAP §Phase 4's seven success criteria and each asserts the property that criterion states. The kill-then-resume pair asserts a byte-identical stored pointer and a complementary second upload set. The two carry-forward guards are present and would fail loudly on a fifth object kind or a raised pack target. The whole file runs against mockito with no real token, `$HOME`, or network.</done>
  <precondition>All six earlier Phase 4 plans are merged. This plan calls the orchestrator, packer, uploader, pointer, prune, and rekey together; if any is still stubbed the tests would assert against a stub and pass for the wrong reason.</precondition>
</task>

<task type="auto">
  <name>Task 2: What push does, what prune deletes, and what it all costs</name>
  <files>docs/sync-github.md, README.md</files>
  <behavior>
    - A reader who has run `sync setup` can push, prune, and change their password from this document without reading the source.
    - The document states what a prune deletes and what it keeps, and names the config key that controls it.
    - The document states that changing the password is not revocation, in the same words the command prints.
    - The document states the acceptable-use risk of a large bundle rewritten frequently, without either alarming or hiding it.
    - The README links to it and does not duplicate it.
  </behavior>
  <action>
Extend `docs/sync-github.md`, which plan 3-05 created for pairing and tokens. Add the commands
this phase ships. Keep it a document a user reads, not a protocol spec — `docs/sync-format.md` §10
is the spec and this page links to it rather than repeating it.

Cover: what `sync push` does and what it does not (it never creates a repository, and it re-checks
that the repository is private immediately before uploading, every time); that an interrupted push
is safe to re-run and will reuse whatever already landed; what progress output looks like on a
terminal and in a captured subprocess.

Cover retention honestly. State the default of ten snapshots and the `[sync] keep_snapshots` key,
that old snapshots are cheap because they share packs, that pruning runs automatically after a
successful push, and that a prune failure is reported as a warning and never fails the push — a
user who sees that warning should know their data is safe and only some storage was not reclaimed.
Name `sync prune` as the on-demand form.

Cover `sync rekey` and repeat, in the document's own voice, what the command prints: the password
changes, the data keys do not, and anyone holding a copy of the old keyfile can still open it with
the old password forever. Real revocation would mean a new master key and re-uploading the whole
bundle, which is exactly what this command exists to avoid. `docs/sync-format.md` §9 already says
this; do not contradict it and do not soften it.

Add a short, unalarmed note on GitHub's acceptable-use policy: there is no prohibition on using a
private repository for backups, but a multi-gigabyte bundle rewritten frequently is the profile
that draws attention, and the two mitigations are already built in — content-addressed packs mean
unchanged data is never re-uploaded, and prune removes superseded generations. The user's account
should not be turned into a storage tier without their knowing.

In `README.md`, extend the sync section 3-05 added with one line per new command and a link. Do
not duplicate the content; a second copy is a second thing to keep true.
  </action>
  <verify>
    <automated>cargo test --test sync_push_e2e</automated>
  </verify>
  <done>`docs/sync-github.md` documents push, resume, retention and `keep_snapshots`, `sync prune`, `sync rekey` with the not-revocation statement, and the acceptable-use note. The README links to it without duplicating it. Nothing in either document contradicts `docs/sync-format.md` §9 or §10.</done>
  <precondition>Plan 3-05 is merged, so `docs/sync-github.md` and the README's sync section exist to extend rather than create.</precondition>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|---|---|
| test fixtures → the AUR `check()` | These tests run on installers' machines during `makepkg`; anything ambient they read is an install failure |
| documentation → the user's mental model | A document that overstates what a rekey achieves is a security failure delivered in prose |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|---|---|---|---|---|---|
| T-4-51 | Denial of service | a test reading a real `$HOME`, token, or network | high | mitigate | Every test builds its roots from a `TempDir` and points both `Endpoints` fields at one mockito server; cheap KDF parameters throughout, so `makepkg`'s `check()` cannot fail on an installer's config or time out |
| T-4-52 | Repudiation | a green suite that proves nothing because a module is still stubbed | high | mitigate | The plan depends on all six predecessors and its precondition says so; the request-count and byte-count assertions fail against a stub that issues no requests |
| T-4-53 | Tampering | a fifth object kind sealed under `chunk_key` slipping in later | high | mitigate | A source-reading guard test fails loudly and its message names the deferred AAD object-type separator as the required response |
| T-4-54 | Tampering | `PACK_TARGET` raised without re-checking the header ceiling | high | mitigate | A guard test pins the value and its message points at 4-02's worst-case header test |
| T-4-55 | Information disclosure | documentation implying a rekey revokes access | high | mitigate | The document repeats the command's own words, and `docs/sync-format.md` §9 is named as the authority so the two cannot drift apart quietly |
| T-4-56 | Information disclosure | documentation omitting the acceptable-use risk | low | mitigate | A short, factual note with the two built-in mitigations named |
| T-4-SC | Tampering | dependency surface | low | accept | Zero new crates; `mockito`, `tempfile`, and `pretty_assertions` are dev-dependencies today. `Cargo.toml` is not in `files_modified` |
</threat_model>

<verification>
- `cargo test --test sync_push_e2e` is green, and `make test` is green.
- `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, and `cargo machete` are clean.
- `cargo test` passes with `$HOME` unset and no network reachable.
- No file under `src/` is modified by this plan.
</verification>

<success_criteria>
Every one of ROADMAP §Phase 4's seven success criteria is asserted by a named test in one
integration file, driven through the real orchestrator against `mockito`. A user can push, resume
an interrupted push, prune, and change their password by following `docs/sync-github.md`, and
comes away with an accurate belief about what each one did.
</success_criteria>

<output>
Create `.planning/phases/04-push-packs-atomic-flip-gc-rekey/4-07-SUMMARY.md` when done.

Record the measured request count for the ~5,000-chunk push against REPO-06's under-ten target,
and the measured before-and-after asset counts for the twelve-push retention test — those two
numbers are what SYNC-07 and REPO-06 are actually claiming, and Phase 5 needs the second one to
know what a restore will have to fetch.
</output>
