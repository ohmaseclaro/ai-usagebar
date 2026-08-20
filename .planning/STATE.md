---
gsd_state_version: 1.0
milestone: v1.0
milestone_name: milestone
status: verifying
stopped_at: Completed 6-05-PLAN.md — release prepared at 1.2.0, NOT tagged (fork/upstream divergence)
last_updated: "2026-08-20T06:47:25.094Z"
last_activity: 2026-08-20
progress:
  total_phases: 6
  completed_phases: 6
  total_plans: 43
  completed_plans: 48
  percent: 100
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-08-17)

**Core value:** Answer "how much quota do I have left, and on which account?" instantly and
correctly — without the user opening a browser, and without ever mis-reporting one account's
usage as another's.
**Current focus:** Phase 1 — encrypted-bundle-core

## Current Position

Phase: 5 (pull-and-restore) — complete, all 8 merged
Plan: 8 of 8 merged in phase 5
Status: Phase complete — ready for verification
Last activity: 2026-08-20

Progress: [██████████] 100%

## Performance Metrics

**Velocity:**

- Total plans completed: 0
- Average duration: —
- Total execution time: 0 hours

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| - | - | - | - |

**Recent Trend:**

- Last 5 plans: —
- Trend: —

**Per-Plan Metrics:**

| Plan | Duration | Tasks | Files |
|------|----------|-------|-------|
| Phase 5 P08 | ~2h | 3 tasks | 4 files |
| Phase 6 P05 | ~2h | 2 tasks | 10 files |

## Accumulated Context

### Decisions

Decisions are logged in PROJECT.md Key Decisions table.
Milestone-shaping decisions made during setup:

- [Milestone]: Default bundle excludes the 4.0 GB of chat transcripts (measured: 4110 files);
  transcripts are opt-in and bounded

- [Milestone]: Credential-bearing bundles are private-repo only
- [Milestone]: Content-addressed **encrypted chunks**, not whole-file encryption — ciphertext
  defeats delta compression, so a 1-line append would otherwise re-upload the whole file

- [Milestone]: Conflicts resolve last-write-wins per item, with a report
- [Research]: Object store is **GitHub Release assets** + Contents-API compare-and-swap for
  the snapshot pointer — chosen over git objects because asset deletion actually reclaims
  space, while git's `gc` is not user-triggerable

- [Research]: **No content-defined chunking.** Proven unnecessary: appends displace no bytes,
  so fixed 256 KiB chunks keep their hashes. `fastcdc` dropped

- [Research]: **The tool never creates a repository** — withholding `Administration: write`
  makes it structurally incapable of creating a public one

- [Research]: Argon2id m=1 GiB, t=3, **p=1** (p>1 costs the defender ~10% and helps a
  parallel attacker, since `argon2` 0.5.3 has no threading)

- [Research]: Syncing credentials by default **overrides** the research recommendation not to;
  it is the user's stated purpose, bounded by the private-repo gate

- [Phase ?]: 5-08: the e2e fixture is built by driving push::run, so every adversarial case is one mutation of a bundle the real push side produced
- [Phase ?]: 5-08: root B's four sync roots all have different leaf names from A's — the only thing that proves the manifest path encoding is relocatable rather than accidentally identical
- [Phase ?]: 5-08: no TDD RED gate on a plan that asserts already-merged behaviour; four negative controls in src/ (all reverted) are the equivalent evidence
- [Phase ?]: Release prepared at 1.2.0 but NOT tagged: v1.2.0/v1.3.0/v1.3.1 already exist as upstream tags and are not ancestors of our main. Version number and release target are a maintainer decision (6-05)

### Pending Todos

None yet.

### Blockers/Concerns

- **Four assumptions are unverified and scheduled as calibrations**, not guesses: whether
  private-repo release assets honour `Range:` (CAL-1, Phase 1), Claude Desktop LevelDB
  compaction behaviour (CAL-2, Phase 2), Argon2id timings on slow aarch64 Linux (CAL-3,
  Phase 1), and the real compressed size of the 115 MB default bundle (CAL-4, Phase 2).
  Each has a named fallback so can block its phase.

- **`.planning/` must not reach an upstream PR.** This repo is a fork of
  `akitaonrails/ai-usagebar`; use `/gsd-pr-branch` to produce a clean PR branch.

- **Phase 6 excludes the GNOME, KDE and Omarchy sync surfaces** (each is an independent
  frontend contract suite). Revisit if they should be in scope.

- README.md's Sync section does not mention `sync pull` — the command a second machine's owner types is absent from the entry-point doc (5-08 was scoped to tests/ and docs/; four lines of work)
- src/sync/restore/fetch.rs:60 derives MAX_SNAPSHOTS_IN_POINTER from a 'monthly retention tail' that does not exist in the code; the value is fine, the justification is not
- Tag blocked: the fork diverged from akitaonrails/ai-usagebar. v1.2.0 is taken; both PKGBUILDs' url= resolves to upstream, so an AUR build would ship upstream's code. Maintainer must pick the release target, the version number, and whether to rebase onto upstream v1.3.1 first (6-05-SUMMARY)

## Deferred Items

| Category | Item | Status | Deferred At |
|----------|------|--------|-------------|
| *(none)* | | | |

## Session Continuity

Last session: 2026-08-20T06:47:16.129Z
Stopped at: Completed 6-05-PLAN.md — release prepared at 1.2.0, NOT tagged (fork/upstream divergence)
SUMMARY). Nothing implemented yet.
Resume file: None

## Phase 1 — closing note

**Code complete, full gate green: 1104 tests (baseline 973), 0 failures, `make test` PASS,
clippy 0, fmt clean.** 11 units executed (9 planned + `1-10`/`1-11` security remediations),
9 worktree merges with zero conflicts.

`gsd phase.complete 1` is **deliberately not run**. It requires verification status `passed`,
and the status is `human_needed` for two live-gated calibrations (CAL-1 needs a real private
repo + token; CAL-3's aarch64-Linux leg needs slow non-Apple hardware). Marking them
`uat-passed` would assert the user ran tests they have not. Both have shipped fallbacks and
neither gates the code, so the run proceeds — but the roadmap checkbox stays honest until
`1-HUMAN-UAT.md` is actually executed.

**What the phase caught that planning did not:**

- A measured defect — the *default* bundle's manifest (1558 entries, 448 KiB) could not seal
  against a 256 KiB chunk limit. Fixed in-phase (`1-09`) because `Root`'s shape is on-disk
  format and `1-07` was about to pin it.

- An AEAD **nonce-reuse flaw** introduced by the fix for an earlier plan-review blocker: the
  nonce derived from the plaintext while the sealed message was the zstd frame. Invisible to
  all 13 adversarial tests and to the first verification pass; found only by auditing from the
  threat model rather than from the suite.

- Three separate instances of *documentation describing behaviour that does not exist*, the
  last one still asserting the removed nonce rule after two remediation rounds.

## Gate definition — corrected mid-run (found by the Phase 6 planner)

The canonical end-of-phase gate is **`make test` + `cargo clippy --all-targets -D warnings` +
`cargo fmt --check`**, with these **named blind spots**:

- `make smoke` — hits real vendor APIs, needs credentials. Deferred to live UAT.
- `make qml-lint` / `qml-test` — only relevant when `kde-plasmoid/` changes.
- **`./macos/run-tests.sh` — NOT part of `make test`** (it needs `swiftc`; `make test` is cargo
  plus the three Node contract suites). Any phase touching `macos/*.swift` must run it
  explicitly. This was missing from the gate as originally defined and would have let every
  Swift change through untested.

Phase 6 is the phase this bites, and its plans already verify with the harness directly.

## Parallelism hazard found in Phase 3 — file-disjointness is necessary, not sufficient

`3-05` (docs) and `3-04` (the gate) were file-disjoint and merged without a conflict. They still
contradicted each other: `3-05` documented "the tool will warn you if it detects a token with
Administration permissions" while `3-04`, running at the same time, decided that warning must not
ship (`permissions.admin` reports the *user's* role, so it would fire on every correct install).

Neither plan was wrong when written. The defect lived in the gap between them, and no
file-overlap check can see it — the two never touched the same byte.

**What to check when running plans in parallel, beyond `files_modified`:** does any plan in the
wave *describe* behaviour that another plan in the same wave *decides*? Docs, error messages, and
`--help` text are the usual carriers, because they assert things implemented elsewhere.

This is the sixth instance in the milestone of a statement outrunning its implementation, and the
first caused by concurrency rather than by sequence. Phases 4–6 run docs plans alongside
behaviour plans (`4-07`, `6-05`), so the same shape can recur there.

## Phase 4 — the keyfile gap, found by the tracer and not by planning

`4-01` grepped all seven Phase 4 plans and found that **none of them uploads the keyfile asset
on a first push**. `Pointer.keyfile` is set from the local keyfile's content address, but only
`rekey` ever called `upload_asset` for one — so a first push published a pointer naming an asset
that does not exist, and Phase 5 could not bootstrap on a second machine. That is the milestone's
stated purpose, and it would have failed at the last step.

Assigned to `4-03` as `upload::ensure_keyfile`, idempotent by content address, called by both the
first-push path and `4-06`'s rekey. One function, two callers — rather than the rekey-only path
the plans described.

**The class, again:** seven plans each correct in isolation, with the defect in the gap between
them. This is the seventh instance in the milestone of a statement outrunning its implementation.
No file-overlap check finds it; only reading the plans against the goal does.

## Phase 4 — a guard that passed on a real violation

`4-01`'s first REPO-03 guard split each file at `#[cfg(test)]` to scan only production code. That
marker also appears **inside a doc comment** in `pairing.rs`, truncating that file's scanned region
to its first 76 lines — an injected `.post(` below it passed cleanly. The rewrite assembles needles
at runtime (`format!(".{verb}(")`), which removes the reason to skip any region at all.

A guard that cannot fail its own negative control is decoration. Both halves now have hand-run
negative controls.

## Phase 4 — audit verdict OPEN_THREATS, and all three blockers are two-machine bugs

The milestone exists so a second machine can continue where the first stopped. All three
blocking findings break exactly that, and none needs an attacker to be interesting:

- **NEW-1, unregistered by any threat model.** The snapshot counter is computed before the
  compare-and-swap and never recomputed after it. Two machines both read counter 6, both seal a
  root at 7, one wins the flip and the loser's `rebuild` re-runs — but it rebuilds the *list*,
  not the root. Both publish at 7. `anchor::accept` reads an equal counter as "already seen", so
  restoring one machine's snapshot makes the other's read as a re-read. **A backup silently
  dropped by the control that exists to protect backups.**

- **T-4-04.** The accept's justification names the anchor as the rollback defence.
  `grep -rn anchor src/sync/push/` returns three doc comments and zero reads. An authentic old
  pointer is laundered by the next honest push, and then prune deletes every pack the rollback
  orphaned — older than `PRUNE_GRACE`, so uncovered. Reversible tamper becomes irreversible
  deletion, executed by the victim, exit 0.

- **T-4-45.** `ensure_keyfile` publishes *this* machine's keyfile. A machine that has not rekeyed
  re-uploads the old wrapper the rekeying machine had verifiably destroyed, and each push resets
  its `created_at` so the grace window never expires. The password change was cosmetic. The code
  called this a "known sharp edge" for "whoever wires the rekey path". Nobody wired it.

**The guards themselves:** 11 negative controls run, 8 red, **3 green** — three guards do not
detect the violation they exist for. One is Phase 3's F-8 recurring verbatim: a hand-maintained
file list that omits the one file the threat is about. Remediation fixes the shape (recursive
walk), not the instance.

## The defect class, instance count 8

`Index::known_chunks` has zero production call sites; every reference is a test. `4-02-PLAN.md`
promised `build` would call it, the implementation needed locations instead, and the wrapper was
kept "to keep the frozen surface honest".

Running tally of *tested code nothing calls* or *text asserting absent behaviour*:
`http::actionable`, `assert_fresh`, `ensure_keyfile`, `progress::reporter`, `known_chunks`, the
anchor-on-the-push-path, plus three rounds of documentation. **A test that calls the function
directly proves the function works, never that anything uses it.** Every phase from here ends by
enumerating production call sites of what it added.

## Phase 5 — two independent plans converged on the same unbroken tie

`5-02` found that snapshot selection was `counter > best.counter`, so on a *tie* the pointer's
list order decided — the thing T-5-15 requires to be inert. Ties were assumed unreachable;
`4-08`'s NEW-1 proves they are not. Now broken on the root's sealed `created_at`, then its sealed
bytes, with a test that runs both orders and asserts they agree.

Two plans, opposite ends of the wire, same defect. Neither could have seen it alone.

## `ai-usagebar sync pull` exists, and the gate order is the safety property

`restore::run(apply:false)` → apply gate → credential gate → `restore::run(apply:true)`, which
takes the backup and only then writes. **Both gates precede `backup::take`**, so a decline leaves
neither an archive nor a write — asserted by walking the roots *and* the backups directory.

**The stdin collision, resolved deliberately.** Both gates read stdin and `cli.rs` already reads
the sync passphrase from it; piped, they interleave and each consumes the other's line. One
`is_terminal()` read now decides both owners: piped, the password owns the stream and the gates
get no reader at all (a piped run answers by flag); on a terminal, sequential reads and both
gates are offered.

## The defect class — instances 9 and 10, and the shape of the check that finds them

- `layout::to_manifest_path`: a one-line mirror of `push::packer::manifest_path` that only tests
  called — so the *drift test* compared a copy against itself. Deleted; the round-trip tests call
  the real encoder.

- `RestoreOptions::force_rehash`: written by the `Pull` dispatch, read nowhere. Deleted.

Ten instances now. Both of these were found by **enumerating production call sites of everything
the phase added** — not by any test, because each had passing tests of its own. That enumeration
is now part of every phase's exit, and it is the only check in this milestone that has ever
caught this class.

Two related refusals, both correct and both worth keeping as precedent:

- `5-07` refused to put `--force-rehash` on `sync pull`: restore hashes what is on disk and never
  consults the index, so the flag would have no reader. **A flag with no reader is the same
  defect as a printed command that does not exist.**

- `5-02` refused the plan's `MAX_RESTORE_BYTES = MAX_PACKS_PER_RESTORE * PACK_MAX`: the count
  check refuses at 513 packs, so that sum is unreachable and the check would be dead code. The
  two now bound different resources and each is reached by its own test.
