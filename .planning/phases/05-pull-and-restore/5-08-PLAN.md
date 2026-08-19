---
phase: 05-pull-and-restore
plan: 08
type: execute
wave: 4
depends_on: ["5-07"]
files_modified:
  - tests/sync_restore_e2e.rs
  - tests/live.rs
  - docs/sync-format.md
  - docs/configuration.md
  - README.md
autonomous: true
requirements: [SAFE-03, SAFE-04, SAFE-05, SYNC-06, UX-01]
must_haves:
  truths:
    - "**The round trip:** a tree pushed from root A and pulled into an empty root B reproduces it byte-for-byte, with every credential file at mode 0600 — one test, against mockito, no network and no real `$HOME`."
    - "Applying the same snapshot a second time writes zero files and reports zero conflicts (D7)."
    - "A rolled-back counter, a tampered pack, and a manifest naming a missing chunk each refuse and leave **zero** files in root B — asserted by walking it, not by trusting a return value."
    - "A failed pull leaves the local anchor byte-identical, so a forged high counter cannot lock the user out of their own bundle."
    - "The pre-restore archive exists before the first write, and running its printed rollback command restores the prior tree exactly."
    - "Two machines that edited the same routine converge on the newer one and the losing value is named in the summary (SYNC-06)."
    - "A restore interrupted between two items leaves no plaintext outside a destination directory and no half-written credential at its real name."
    - "The seven ROADMAP success criteria for this phase map one-to-one onto seven named tests, and the test names say which criterion they are."
  artifacts:
    - tests/sync_restore_e2e.rs — the round trip and the seven criteria, each a named integration test
    - "an `#[ignore]`d probe in tests/live.rs recording whether a private-repo release asset honours `Range:` — CAL-1, still unrun, with somewhere to land"
    - docs/sync-format.md §11 completed, docs/configuration.md and README.md sync-pull sections
  key_links:
    - "the fixture is built by calling the **push** side, so the test proves the pair rather than a hand-written idea of the format"
    - "root B is a second `SyncRoots::at` over a second `TempDir` with different directory names, which is what proves the manifest paths are relocatable rather than accidentally identical"
    - "every adversarial case mutates a byte of a *valid* pushed bundle, so the only difference between pass and fail is the tampering"
---

<objective>
The proof. One integration file that pushes from machine A, pulls into machine B, and compares —
plus the six refusals and the rollback, each a named test that says which ROADMAP criterion it is.
Then the docs a second machine's owner actually reads.

Purpose: the round trip is the strongest available evidence that this milestone does what the user
wants, which is to push from this laptop and restore on another one.

Output: `tests/sync_restore_e2e.rs`, a CAL-1 probe with somewhere to land, and the user-facing docs.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/05-pull-and-restore/5-CONTEXT.md
@.planning/phases/05-pull-and-restore/5-07-SUMMARY.md
@.planning/phases/05-pull-and-restore/5-02-SUMMARY.md
@.planning/phases/05-pull-and-restore/5-05-SUMMARY.md
@CLAUDE.md
@docs/sync-format.md
@docs/configuration.md
@tests/live.rs
@tests/anthropic_e2e.rs
@src/sync/restore/mod.rs
@src/sync/cli.rs
</context>

<tasks>

<task type="auto" tdd="true">
  <name>Task 1: Push from A, pull into B, compare — and do it twice</name>
  <files>tests/sync_restore_e2e.rs</files>
  <behavior>
    - `roundtrip_reproduces_the_pushed_tree_byte_for_byte`: seed root A across all five categories including a multi-chunk file above `CHUNK_SIZE`, push to mockito, pull into an empty root B built over a *differently named* temp directory, and assert every file's bytes and relative position match. Credential files are mode 0600, directories 0700.
    - `a_second_apply_of_the_same_snapshot_writes_nothing`: re-run the pull against the restored root B and assert zero written, zero overwritten, and no `SkipLocalNewer` in the plan (D7).
    - `a_newer_local_routine_wins_and_the_loser_is_named`: touch a routine in B after the restore, push a competing edit from A, pull into B, and assert the local one is skipped and named; then repeat with the edit older than the snapshot and assert it is overwritten and named in the summary (SYNC-06).
    - `the_backup_exists_before_the_first_write_and_its_command_restores_exactly`: assert the archive's mtime precedes every written file's, then clobber B, run the printed rollback command through `/bin/sh`, and compare trees including modes (SAFE-04).
    - Root B's `SyncRoots` uses different directory names from A's throughout, so a manifest path that only worked because the two happened to match would fail.
  </behavior>
  <action>
Create `tests/sync_restore_e2e.rs`, following `tests/anthropic_e2e.rs`'s conventions — `mockito`,
`TempDir`, everything injected.

Build the fixture by driving the **push** side end to end: seed root A, run the push orchestrator
against a `mockito::Server`, and serve what it uploaded back to the pull. Do not hand-write pointer
JSON or a pack. A hand-written fixture tests the test author's understanding of the format; this
one tests the pair, which is the only thing anyone cares about.

Root B is deliberately shaped differently: a second `TempDir` whose `config_dir`,
`desktop_data_dir`, `desktop_profiles_dir`, and `claude_home` all have different leaf names from
A's. That is what proves 5-01's relocatable path encoding. If A and B used the same temp layout,
a manifest full of absolute paths would pass this test — which is precisely the bug the encoding
exists to prevent.

Seed A with something from every category, and include one file larger than `CHUNK_SIZE` so the
reassembly path is exercised, one file with a name containing a space, and one nested three
directories deep. Compare by walking B and A in the same relative order and asserting bytes and
`#[cfg(unix)]` modes.

The idempotence test runs the *whole command* a second time rather than re-applying a cached plan:
D7's claim is about re-running an interrupted restore, and re-applying a stale plan would prove
something weaker.
  </action>
  <verify>
    <automated>cargo test --test sync_restore_e2e</automated>
  </verify>
  <done>`cargo test --test sync_restore_e2e` is green for the four tests above. The fixture is produced by the push side. Root B's directory names differ from A's throughout. The multi-chunk, spaced-name, and deeply-nested files all round-trip. The rollback command is executed and the restored tree matches including modes. No test opens a socket outside the mockito base or reads a real `$HOME`.</done>
</task>

<task type="auto" tdd="true">
  <name>Task 2: The refusals, the interruption, and the anchor that must not move</name>
  <files>tests/sync_restore_e2e.rs</files>
  <behavior>
    - `a_rolled_back_counter_refuses_and_writes_nothing`: seed B's anchor above the snapshot's counter, pull, assert non-zero, assert B is empty, and assert the anchor file is byte-identical afterwards.
    - `a_counter_from_another_bundle_refuses_even_with_allow_rollback`: same but with a different `repo_id`, and with the flag set.
    - `a_tampered_pack_refuses_and_writes_nothing`: flip one ciphertext byte in a served pack; assert one distinct "cannot decrypt" error, zero files in B, and zero bytes of plaintext anywhere under B or `TMPDIR`.
    - `a_manifest_naming_a_missing_chunk_refuses_and_writes_nothing`: withhold one chunk's pack; same assertions.
    - `a_traversal_path_is_refused_and_reported`: rewrite a manifest entry to `../../../../etc/x` and to an absolute path; assert nothing is written outside B, that the run reports both refusals, and that the report names them.
    - `an_interrupted_restore_leaves_no_plaintext_outside_a_destination`: inject a failure between two items; assert the items before it are complete, the item at it is absent under its real name, no `.tmp.` entry survives under B, and `TMPDIR` is untouched.
    - `a_failed_pull_never_advances_the_anchor`: for each refusal above, assert the anchor file's bytes are unchanged.
  </behavior>
  <action>
Add the refusal half of the file. Every adversarial case starts from a **valid** pushed bundle and
mutates exactly one thing, so the only difference between the passing and failing runs is the
tampering — a fixture built broken from the start can pass for the wrong reason.

Assert emptiness by walking root B and collecting every path, then asserting the collection is
empty. A return value saying "wrote 0" is the thing under test and cannot also be the evidence.

For the plaintext assertions, walk root B *and* the process's `TMPDIR` and grep both for a
distinctive byte string seeded into the fixture's contents. That is the only assertion that
actually covers SAFE-05's wording — that no plaintext exists at a temp path outliving the
operation — because a test that only checks the destination would pass while `/tmp` held a copy.

The anchor assertion is its own helper called from every refusal test: read the anchor file's bytes
before and after and assert equality. This is the Phase 1 handoff's risk expressed as a test —
advancing on a failed verify would let an attacker with repo write access permanently lock the user
out of their own bundle, and it is the kind of ordering bug that survives review and dies to an
assertion.

Add an `#[ignore]`d test to `tests/live.rs` for **CAL-1**: issue a `Range:` request against a real
private-repo release asset after the 302 and record whether the signed-storage host honours it.
CAL-1 has now gone unrun through Phases 1, 3, and 4; 5-02 shipped whole-pack fetch on the
pessimistic assumption, which is correct either way. The probe is there so the measurement has
somewhere to land, and its doc comment states what would change if it comes back positive: a
byte-range fetch in `PackSource` keyed on `PackEntry.offset` and `clen`, and nothing else.
  </action>
  <verify>
    <automated>cargo test --test sync_restore_e2e</automated>
  </verify>
  <done>`cargo test --test sync_restore_e2e` is green for all eleven tests. Every adversarial case mutates one thing in a valid bundle. Emptiness is asserted by walking, not by a return value. The plaintext search covers both root B and `TMPDIR`. The anchor-unchanged helper is called from every refusal test. `tests/live.rs` carries an `#[ignore]`d CAL-1 probe. `cargo test` passes with `$HOME` unset.</done>
</task>

<task type="auto">
  <name>Task 3: The docs a second machine's owner reads</name>
  <files>docs/sync-format.md, docs/configuration.md, README.md</files>
  <behavior>
    - `docs/sync-format.md` §11 describes the full reader chain, the manifest path encoding with its four prefixes, the four read ceilings and what each bounds, and the rule that the anchor advances only after the root verifies.
    - §9's honest-limits section gains the restore-side entries: first-contact TOFU is still an accepted residual risk, and a restore is additive and never deletes.
    - `docs/configuration.md` documents `sync pull` and every flag, with `--force` and `--force-credentials` stating what they can lose.
    - The README sync section shows the two-machine flow end to end: push here, `sync setup` there, `sync pull` there, and the backup path with its rollback command.
    - Every command in the docs is one a reader can paste; no placeholder that does not resolve.
  </behavior>
  <action>
Complete §11 of `docs/sync-format.md` from 5-01's skeleton and 5-02's constants. It should be
enough, with §10, for someone to write an independent reader: the chain, the path encoding, the
ceilings with the quantity each was derived from, and the anchor ordering with the reason.

Extend §9 rather than starting a section. Two additions: first-contact TOFU remains an accepted
residual risk on the restore side too — it is documented in `anchor::accept`'s doc comment and owed
a mention in the milestone's residual-risk section — and restore is additive, so a file deleted on
machine A is not deleted on machine B by pulling. Both are decisions, and §9 exists for decisions
that would otherwise read as oversights.

`docs/configuration.md` gets the `sync pull` flag table. `--force` and `--force-credentials` do not
get a mechanism description; they get a sentence about what they can lose, because that is what a
user needs at the moment they are typing one.

The README section is the two-machine story, in order, with real commands: push from the first
machine, `sync setup` on the second, `sync pull` to see what would land, `sync pull --apply` to
land it, and where the backup went with the exact command to undo it. Keep it short. State plainly
that a pull writes nothing by default, and why.
  </action>
  <verify>
    <automated>make test</automated>
  </verify>
  <done>`docs/sync-format.md` §11 is complete and §9 carries both restore-side residual entries. `docs/configuration.md` lists every `sync pull` flag with its consequence. The README shows the two-machine flow with pasteable commands. `make test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, and `cargo machete` are all clean.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| a valid bundle mutated by one byte → the whole restore path | The adversarial fixtures cross every boundary the phase defends |
| the test's own assertions → the evidence | A test that trusts a return value proves nothing about the filesystem |
| documentation → a user's expectations | What §9 omits, a user assumes is handled |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|-----------|----------|-----------|----------|-------------|-----------------|
| T-5-70 | Repudiation | a test asserting emptiness from a return value | high | mitigate | Every "writes zero files" assertion walks root B and collects paths; the return value under test is never the evidence for itself |
| T-5-71 | Information disclosure | plaintext surviving in `TMPDIR` | critical | mitigate | The interruption and tampering tests grep both root B and `TMPDIR` for a distinctive seeded byte string, which is the only assertion that covers SAFE-05's actual wording |
| T-5-72 | Denial of service | an anchor advanced on a failed verify | critical | mitigate | A byte-comparison helper is called from every refusal test; the Phase 1 handoff's risk becomes an assertion rather than a review note |
| T-5-73 | Tampering | a fixture broken from the start passing for the wrong reason | high | mitigate | Every adversarial case starts from a bundle produced by the real push side and mutates exactly one thing |
| T-5-74 | Spoofing | absolute manifest paths passing because A and B share a layout | high | mitigate | Root B's four directories all have different leaf names from A's |
| T-5-75 | Repudiation | a residual risk that reads as an oversight | medium | mitigate | §9 records first-contact TOFU and the never-deletes decision explicitly |
| T-5-76 | Tampering | a live probe leaking into the AUR `check()` | high | mitigate | The CAL-1 probe is `#[ignore]`d in `tests/live.rs`, and the rollback test skips when `/usr/bin/tar` or `/bin/sh` is absent |
| T-5-SC | Tampering | npm/pip/cargo installs | high | mitigate | No new crates; `cargo machete` runs in this plan's gate |
</threat_model>

<verification>
- `cargo test --test sync_restore_e2e` green — eleven named tests.
- `HOME= cargo test` passes with the network unavailable.
- `make test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, `cargo machete` clean.
- `cargo test -- --ignored --list` shows the CAL-1 probe.
</verification>

<success_criteria>
1. A pushed tree pulls into a differently-shaped second root byte-for-byte, credentials at 0600.
2. A second apply writes nothing and reports no conflicts.
3. A rolled-back counter, a tampered pack, a missing chunk, and a traversal path each refuse with zero files written.
4. The backup precedes the first write and its printed command restores exactly.
5. A failed pull leaves the anchor byte-identical.
6. An interrupted restore leaves no plaintext outside a destination directory.
7. The two-machine flow is documented with pasteable commands, and §9 records both restore-side residual risks.
</success_criteria>

<output>
Create `.planning/phases/05-pull-and-restore/5-08-SUMMARY.md` when done, mapping each of the seven
ROADMAP success criteria to the test that proves it, and recording CAL-1 as still unmeasured.
</output>
