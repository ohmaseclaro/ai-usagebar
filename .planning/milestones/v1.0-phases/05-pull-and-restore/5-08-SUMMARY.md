---
phase: 05-pull-and-restore
plan: 08
subsystem: sync/restore
status: complete
tags: [e2e, two-machine, round-trip, refusals, anchor, docs, calibration]
requires:
  - "sync::push::run (Phase 4, 4-08) — the fixture is produced by the push side, never hand-written"
  - "sync::restore::run + RestoreOptions/Disposition/RestoreOutcome (5-01, frozen)"
  - "sync::restore::report::{render_plan, render_outcome} (5-06)"
  - "sync::push::{anchor_path, prune::run_on_demand, rekey::run} (4-08)"
provides:
  - "tests/sync_restore_e2e.rs — 16 named integration tests over the push/restore pair"
  - "docs/sync-format.md §11 — the restore side of the format, enough to write an independent reader"
  - "docs/configuration.md — `sync pull` and every flag, and `[sync] keep_snapshots`"
affects:
  - "nothing — no source file under src/ was touched; Cargo.toml and Cargo.lock are byte-identical"
tech-stack:
  added: []
  patterns:
    - "the adversarial fixture is one mutation of a bundle the real push side produced"
    - "emptiness asserted by walking the tree, never by the return value under test"
    - "a hostile manifest entry produced through a real push rather than hand-built"
key-files:
  created:
    - tests/sync_restore_e2e.rs
  modified:
    - tests/live.rs
    - docs/sync-format.md
    - docs/configuration.md
decisions:
  - "root B's four roots all have different leaf names from A's, under a different username, in a separate TempDir — the only thing that proves the path encoding is relocatable"
  - "the traversal refusal is reached by giving machine A a config path spelled with a `..`, which the real push renders verbatim, rather than by hand-building a manifest"
  - "the TMPDIR half of SAFE-05 stays a structural assertion in write.rs rather than a walk of a process-global directory a concurrent test also owns"
  - "no TDD RED gate: this plan asserts behaviour that is already merged, so negative controls are the equivalent evidence and four were run"
metrics:
  lib_tests: 1512
  total_tests: 1572
  failing: 0
  new_crates: 0
  duration: ~2h
  completed: 2026-08-20
---

# Phase 5 Plan 08: The proof — Summary

Machine A's roots push a bundle. Machine B's roots — a different username, a
different directory tree, an empty index, no local keyfile — pull it and end up
byte-identical. Sixteen named tests, one mock remote, and every fixture a bundle
the push side really produced.

**No source file under `src/` was touched.** `Cargo.toml` and `Cargo.lock` are
byte-identical. Every claim in the docs below was checked against merged code
before it shipped, and three were wrong when checked.

---

## THE SEVEN ROADMAP CRITERIA, AND THE TEST THAT PROVES EACH

| # | Criterion | Test |
|---|---|---|
| 1 | a pushed tree pulls into a differently-shaped second root byte for byte, credentials at 0600 | `criterion_1_a_pushed_tree_restores_byte_for_byte_under_a_second_machines_roots` |
| 2 | a second apply writes nothing and reports no conflicts | `criterion_2_a_second_apply_of_the_same_snapshot_writes_nothing_and_reports_no_conflict` |
| 3 | a rolled-back counter, a tampered pack, a missing chunk and a traversal path each refuse with zero files written | `criterion_3_a_rolled_back_pointer_is_refused_and_prune_does_not_collect_its_orphans`, `criterion_3_a_tampered_pack_refuses_and_leaves_no_plaintext_under_machine_b`, `criterion_3_a_snapshot_naming_a_pack_the_release_withholds_refuses_and_writes_nothing`, `criterion_3_a_manifest_entry_that_escapes_its_root_is_refused_and_named_in_the_report` |
| 4 | the backup precedes the first write and its printed command restores exactly | `criterion_4_the_backup_precedes_the_first_write_and_its_printed_command_restores_exactly` |
| 5 | a failed pull leaves the anchor byte-identical | `criterion_5_a_failed_pull_never_advances_the_anchor`, plus the `assert_anchor_frozen` helper called from every refusal test |
| 6 | an interrupted restore leaves no plaintext outside a destination directory | `criterion_6_an_interrupted_restore_leaves_no_half_written_file_and_no_anchor_move` |
| 7 | the two-machine flow is documented, and §9 records both restore-side residual risks | `docs/configuration.md`'s `sync pull` section; `docs/sync-format.md` §9's two new subsections and §11 |

And five that are not numbered criteria but are each a defect that was found and
fixed rather than a hypothetical:

| Test | The defect it pins |
|---|---|
| `a_dry_run_writes_nothing_at_all_and_never_downloads_a_pack_it_only_needs_for_file_data` | 5-02 made the file-data round structural; without two pushes the assertion is vacuous, because a one-push bundle's every pack also carries metadata |
| `a_push_killed_before_the_flip_costs_one_refused_push_and_the_resume_restores_the_tree` | the *first* re-run reuses everything — one refused push, not two — and what the resume published is what actually restores |
| `two_machines_racing_publish_distinct_counters_and_a_pull_takes_the_newest` | 4-08 NEW-1: before it, both machines published at one counter and the anchor read the loser's distinct snapshot as already-seen |
| `force_alone_never_overwrites_a_live_credential_and_force_credentials_needs_force` | D2: `--force` does not grant the second consent, and the credential flag alone grants nothing |
| `a_symlink_at_a_destination_is_refused_and_no_flag_promotes_it` | T-5-22: `SkipLocalNewer` is the one disposition `--force` promotes, so a symlink reported that way is written through by the obvious next command |
| `a_machine_that_missed_a_rekey_is_refused_a_push_and_can_still_pull` | 4-08 NEW-3, composed: the refusal is real *and* the machine is not stranded by it — a pull consults no local keyfile |

---

## WHAT THIS FILE ASSERTS THAT A UNIT SUITE STRUCTURALLY CANNOT

**Root B is shaped differently on purpose.** Its `config_dir`,
`desktop_data_dir`, `desktop_profiles_dir` and `claude_home` all have different
leaf names from A's, under a different username, in a separate `TempDir`. If the
two layouts matched, a manifest full of absolute paths would pass this file —
which is precisely the bug the relocatable encoding exists to prevent. 5-01
proved the encoding at the unit level; this proves the whole command path over
it.

**Emptiness is asserted by walking.** Every "writes zero files" claim collects
every path under machine B and asserts the collection is empty (T-5-70). The
return value under test is never the evidence for itself.

**Every adversarial case mutates exactly one thing in a real bundle** (T-5-73).
The tampered-pack test flips one ciphertext byte and then re-runs the *same*
bundle untampered to prove the refusal was about the byte. The withheld-pack test
answers 404 for one asset. The rollback test replaces the pointer with its own
earlier bytes. Nothing is hand-rolled, because a hand-rolled fixture agrees with
a broken reader.

**The anchor is compared byte for byte after every refusal.** `assert_anchor_frozen`
is called from the rollback, tampering, withholding, malformed-pointer,
bundle-identity and partial-restore tests. Plan 1-05's recorded risk — that
advancing on a *claim* lets anyone with repo write access lock the user out of
their own bundle permanently — is now an assertion rather than a review note
(T-5-72).

**The archive is proven to be the reversal set, not just to exist.** Criterion 4
extracts the archive and compares its members against the *pre-restore* bytes,
then clobbers those destinations and executes the rendered `tar -xzf … -C …`
through `/bin/sh`, comparing contents **and modes**. A credential archived at
0600 and restored at 0644 would be a leak created by the safety mechanism.

---

## NEGATIVE CONTROLS — FOUR RUN, ALL REVERTED

Every injection was made in `src/`, watched to fail on the exact defect, and
undone. `git status` was clean before the first commit; the three commits touch
`tests/` and `docs/` only.

| Injection | Result |
|---|---|
| `push::run`'s rebuild closure derives the counter from `ctx.previous` instead of the arriving pointer (the pre-4-08 shape) | `no two snapshots may claim one counter: [1, 2, 2]` — exactly the collision 4-08 fixed |
| `prune::run_on_demand`'s `assert_no_rollback` call removed | the rolled-back pointer's orphans are collected; the criterion-3 rollback test fails |
| `merge::decide`'s credential arm relaxed so `force` alone satisfies it | `--force` replaces the live token; the credential test fails |
| `restore::run` step 7's `applied.failed_at.is_none()` guard replaced with `true` | a partial restore advances the anchor; criterion 6 fails |

**One injection that correctly does *not* fail, recorded honestly.** Reversing
`plan::build`'s per-category sort (`a.path.cmp(&b.path)` → `b.path.cmp(&a.path)`)
leaves `a_push_killed_before_the_flip_…` green — and should. Reversing is still
*deterministic*, and the defect the sort fixed was non-determinism: pack content
addresses depend on the order blobs land inside a pack, so an order that varied
between runs made the first resume reuse nothing. Non-determinism cannot be
injected by an edit, so the reuse-on-first-resume assertion is a real regression
test with no clean negative control. Stated rather than implied.

---

## THE DOCS, AND THE THREE CLAIMS THAT WERE WRONG WHEN CHECKED

This milestone's most repeated defect is text asserting behaviour that does not
exist. Every sentence written here was checked against merged code, and three
drafts did not survive it.

1. **"a tie the push side can produce when two machines race."** True before
   4-08, false after: a writer now derives the counter from the pointer its
   compare-and-swap actually lands against, so the loser re-seals one above the
   winner. §11 now says a tie is no longer something a *correct writer* produces,
   that a hostile remote can still hand-write one, and that a reader must
   therefore still break it deterministically.
2. **"the anchor advance is the caller's job"** was true but incomplete. §9 now
   states that **all three** operations that publish a pointer must run the check
   — push, garbage collection, and a password change — because each carries the
   arriving records forward and so launders a rollback, and that guarding only
   the push leaves garbage collection as the executioner. It also records that
   the override lives on `sync push --allow-rollback` alone.
3. **"`keep_snapshots` defaults to 10 and a monthly retention tail adds a
   dozen."** There is no monthly retention tail — see the finding below. The
   ceiling is derived from `keep_snapshots` alone.

Everything else the orchestrator flagged was checked and is stated correctly: a
dry run is the absence of `--apply` and not a flag anything reads; `--force` does
not grant `--force-credentials` and `force_credentials` requires `force`; a piped
run gives stdin to the password and answers with flags while a terminal is
offered both gates; `keep_snapshots` is clamped to ≥1 by prune and `0` is refused
at config load; **`--force-rehash` is documented as deliberately absent from
`sync pull`**, with the reason.

### `docs/sync-format.md` §11 — enough to write an independent reader

The seven-step order and why two steps are only correct in it; the three download
rounds and why a dry run stops after two; the four-prefix relocatable path
encoding and every spelling the reader refuses, including why the result is built
one component at a time and why `canonicalize` is never called; the read ceilings
with the quantity each was derived from and why the request and transfer bounds
are deliberately not one bound; what a reader believes about a pointer (which
pack holds a chunk, and nothing else) and what it does not; the per-item decision
table with the three refusals decided before it; the write ordering and what
survives a kill mid-restore; the pre-restore archive and its three-outcome
contract.

### §9 — the two restore-side residuals

- **First-contact TOFU applies to a second machine's first pull too**, and is
  accepted rather than mitigated. Written down because a reader told "the anchor
  catches replays" will otherwise assume the first fetch was covered.
- **A restore is additive and never deletes**, including under `--force`, with
  the reason (an authoritative restore needs a per-machine baseline, and the
  failure mode of getting that wrong is deleting a user's data on a machine that
  only asked to receive a copy) and the practical consequence stated plainly.

### `docs/configuration.md`

A `sync pull` flag table where `--force` and `--force-credentials` say **what
they can lose** rather than how they work; who owns stdin on a terminal versus a
pipe; where the archive lands and that its undo command is printed; five things a
pull will not do; and `[sync] keep_snapshots`, previously undocumented.

---

## CAL-1 AND CAL-5 — BOTH STILL UNRUN

**Neither has been measured, and nothing shipped depends on either.** Both need a
real private repository and a real token; CAL-5 additionally *writes and deletes*
release assets. Both probes stay `#[ignore]`d, so `cargo test` and the AUR
`check()` reach neither — `cargo test -- --ignored --list` shows both.

- **CAL-1** (does a private-repo release asset honour `Range:` after the 302) is
  now open through **four** phases. Phase 5 shipped a whole-pack fetch, which is
  correct whichever way it goes. The probe's doc comment and §7 now name the
  single landing spot for a positive answer: a byte-range fetch in
  `restore::PackSource` keyed on the `PackEntry`'s `offset` and `clen`, which a
  reader already has out of each pack's own sealed header — and nothing else
  moves, not the ceilings, not the content-address check, not the three rounds.
- **CAL-5** (a torn upload's `state`, and whether `digest` is populated) gains
  its own §7 subsection saying it is unmeasured and that both shipped branches
  are the conservative one: an unrecognised `state` re-uploads and the size check
  runs regardless, and a `digest` that is not assumed present means the verifying
  download happens unconditionally. Measuring it could remove that download from
  a first push; it can never justify dropping the size check.

---

## Deviations from Plan

### Scope narrowed by the orchestrator — `README.md` was not touched

The plan lists `README.md` in `files_modified` and its Task 3 done-criteria ask
for the two-machine flow there. The orchestrator's file list for this run is
`tests/sync_restore_e2e.rs`, `tests/live.rs`, `docs/sync-format.md`,
`docs/configuration.md`, and I honoured it rather than widening the diff.

**This leaves a real gap, recorded rather than lost: `README.md`'s "Sync
(optional)" section lists `status`, `push`, `prune` and `rekey` and does not
mention `sync pull` at all.** The command a second machine's owner types is
absent from the project's entry-point document, which is the same defect class
this phase kept closing, inverted. It is four lines of work: a `sync pull` bullet
alongside the other four, and a pointer to `docs/configuration.md`'s new section.
See **Findings** below.

### [Rule 1 — correctness] The traversal case is produced by a real push, not hand-built

The plan asks the test to "rewrite a manifest entry to `../../../../etc/x`". Doing
that means re-sealing the manifest, re-packing it, rebuilding the index object
and re-sealing a root — which is re-implementing `packer::build` in the test, and
directly contradicts the plan's own key link that "every adversarial case mutates
a byte of a *valid* pushed bundle".

Instead, machine A's `config_file` is spelled `…/ai-usagebar/../ai-usagebar/config.toml`
— a path a user can genuinely configure, pointing at the same real file, which
`scope::push_path` accepts and `packer::manifest_path` renders **verbatim** as
`config/../ai-usagebar/config.toml`. The bundle is produced by a real
`push::run`; the only difference from the passing run is one root path. Machine B
refuses the entry as `RejectedPath`, gives it no destination, names it in the
rendered report, and restores its honest sibling anyway.

The absolute-path flavour the plan also names is **not** reachable this way —
`manifest_path` always emits a root prefix — and is pinned instead by
`layout.rs`'s eleven hostile-spelling unit tests, which already cover it.

### [Rule 1] The backup-ordering assertion is by content, not by mtime

The plan asks the test to "assert the archive's mtime precedes every written
file's". That cannot work: 5-04 stamps every restored file with the **snapshot's**
`created_at`, so restored files are back-dated to the fixture's fixed `NOW` and
compare *earlier* than the archive on every run. The assertion would fail against
correct code.

Replaced by a strictly stronger, clock-free one: the archive is extracted and its
members compared against the **pre-restore** bytes. An archive taken after the
write would hold the new bytes and fail. `record.members` is additionally
asserted equal to the number of writable items, so the archive is proven to be
exactly the reversal set.

### Scoped out: the `TMPDIR` walk (T-5-71)

The plan asks the interruption and tampering tests to grep `TMPDIR` as well as
root B. 5-04 scoped the same requirement out of its own suite, for reasons that
have not changed: observing it means mutating a process-global env var, which is
racy under the harness's threads, `unsafe` in edition 2024, and exactly the
"branch on an ambient env var" CLAUDE.md forbids — and a *read-only* walk of the
host's real temp directory is both slow and contaminated by every other test's
`TempDir`.

What ships instead: every refusal and the interruption walk **all of machine B's
tree** for a distinctive marker string seeded into the credential fixture's
plaintext, and assert no `.tmp.` file survives. The `TMPDIR` half stays the
structural assertion 5-04 built — `write.rs`'s own source, comments stripped,
contains no `temp_dir`, no `"/tmp"` and no `into_temp_path`, so plaintext has no
route to a shared temp directory because no such call exists.

### No TDD RED gate on tasks 1 and 2

Both tasks are marked `tdd="true"`. There is no honest RED phase available: this
plan asserts behaviour that is already merged and already green, so a failing-first
run would only mean the test was wrong. The equivalent evidence is the four
negative controls above — each one reintroduces the defect the assertion exists
for and watches it fail. Recorded rather than faked.

### One fixture trap, paid for once and then written down

An edit to an already-pushed file that keeps the **same length** and the **same
mtime** is invisible to the planner: the local index keys change detection on
`(path, size, mtime_ns, inode)` and `fs::write` over an existing file keeps its
inode, so the file is never re-read and the next snapshot silently carries the old
bytes. Two fixtures hit it. The file now has a `Machine::edit` helper that stamps
a later mtime, with the trap written next to it.

---

## Findings for whoever owns these files

**1. `README.md` does not mention `sync pull`.** Described under Deviations
above. Not this run's file; four lines of work.

**2. `src/sync/restore/fetch.rs:60` cites a retention feature that does not
exist.** `MAX_SNAPSHOTS_IN_POINTER`'s doc comment reads "`keep_snapshots`
defaults to 10 (see `config.rs`) and the monthly retention tail adds a dozen
more". `grep -rn monthly src/` finds nothing in the sync tree: there is no
monthly retention tail, and the pointer's length is bounded by `keep_snapshots`
alone. The ceiling's **value** is unaffected — 256 is still an order of magnitude
past any legitimate pointer — but the derivation is written against a feature
that was never built, which is this milestone's most repeated defect appearing in
a comment rather than in a message. `docs/sync-format.md` §11 deliberately does
not repeat it. `src/` is outside this run's scope.

**3. Both of 5-07's call-site findings are already closed.** `layout::to_manifest_path`
has been deleted — `grep -rn to_manifest_path src/` returns nothing, and
`push::packer::manifest_path` is the single encoder, which `layout.rs`'s drift
test now calls directly. `RestoreOptions::force_rehash` is gone too; `sync pull`
offers no such flag and `restore/` reads no such field. Recorded here because
5-07 assigned both to this plan and neither needed doing.

---

## Known Stubs

None.

## Threat Flags

None. This plan adds no network endpoint, no auth path, no file-access pattern
and no schema at a trust boundary. It adds one test file and three documentation
edits; `src/` is untouched.

Register coverage:

| ID | Mitigation | Where |
|---|---|---|
| T-5-70 | every "writes zero files" claim walks root B and collects paths | `Machine::restored`, called from six tests |
| T-5-71 | a distinctive marker searched across all of machine B; the `TMPDIR` half stays structural in `write.rs` | `plaintext_anywhere_under`; see the deviation |
| T-5-72 | the anchor's bytes compared before and after every refusal | `assert_anchor_frozen`, called from six tests |
| T-5-73 | every adversarial case is one mutation of a bundle `push::run` produced | the whole file; the tampering test re-runs untampered to prove it |
| T-5-74 | root B's four roots all have different leaf names from A's | `bob_roots` vs `alice_roots` |
| T-5-75 | §9 records first-contact TOFU on the restore side and the never-deletes decision | `docs/sync-format.md` §9 |
| T-5-76 | the CAL-1 and CAL-5 probes stay `#[ignore]`d; the rollback test skips without `/bin/sh` or `/usr/bin/tar` | `tests/live.rs`; `criterion_4`'s opening guard |
| T-5-SC | no new crates | `git diff Cargo.toml Cargo.lock` empty |

---

## Verification

| Gate | Result |
|---|---|
| `cargo test` | **1512 lib / 1572 total, 0 failing** (baseline 1512 / 1556 — **+16**, all in the new file) |
| `cargo test --test sync_restore_e2e` | 16 passed, 0 failed, in 1.4 s |
| `cargo clippy --all-targets -- -D warnings` | clean |
| `cargo fmt --check` | clean |
| `make test` | exit 0, plus the GNOME, KDE and Omarchy JS contract suites |
| `env -u HOME -u XDG_CONFIG_HOME -u XDG_CACHE_HOME cargo test --test sync_restore_e2e` | 16 passed — hermeticity holds |
| `cargo test -- --ignored --list` | `cal1_range_on_private_release_asset` and `cal5_release_asset_state_and_digest` both listed |
| `Cargo.toml` / `Cargo.lock` | unchanged; no new crates |
| `cargo machete` | not installed on this machine; `Cargo.toml` is byte-identical, so no dependency could have become unused |
| relative links in the two edited docs | all three resolve; one pre-existing broken link fixed |

Every test is hermetic: both machines' roots are `TempDir`s, both `Endpoints`
point at one mockito server, `NOW` and the remote's clock are distinct fixed
constants, every local mtime is stamped rather than read from the wall clock, KDF
parameters are `{ m_kib: 8, t: 1, p: 1 }` except the one rekey test which uses the
8 MiB write-path floor, and no test reads a real `$HOME`, a real token, or the
macOS Keychain. The two tests that shell out inject `/usr/bin/tar` and `/bin/sh`
explicitly and return early when either is absent; the read-only-directory test
checks its own premise by trying it, so it skips rather than lying when run as
root, and needs no `libc` dependency to do so.

## Commits

| Commit | What |
|---|---|
| `463a585` | `test(5-08)` — `tests/sync_restore_e2e.rs`, 16 tests |
| `130816b` | `docs(5-08)` — what CAL-1 and CAL-5 would change, and that neither has been run |
| `0944c16` | `docs(5-08)` — §11, §9's two residuals, and `sync pull`'s flags |

## Self-Check: PASSED

- `tests/sync_restore_e2e.rs` — FOUND
- `tests/live.rs` — FOUND, modified
- `docs/sync-format.md` — FOUND, modified
- `docs/configuration.md` — FOUND, modified
- `463a585`, `130816b`, `0944c16` — all FOUND in `git log` on `gsd/5-08`
- `git diff --diff-filter=D` across the three commits — no deletions
- `git status --short` after the third commit — clean
