---
gsd_state_version: 1.0
milestone: v1.0
milestone_name: milestone
current_phase: 1
current_phase_name: encrypted-bundle-core
status: executing
stopped_at: Milestone artifacts written (PROJECT, REQUIREMENTS, ROADMAP, STATE, research×3 +
last_updated: "2026-08-19T16:50:50.537Z"
last_activity: 2026-08-19
last_activity_desc: Phase 1 execution started
progress:
  total_phases: 6
  completed_phases: 0
  total_plans: 15
  completed_plans: 0
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-08-17)

**Core value:** Answer "how much quota do I have left, and on which account?" instantly and
correctly — without the user opening a browser, and without ever mis-reporting one account's
usage as another's.
**Current focus:** Phase 1 — encrypted-bundle-core

## Current Position

Phase: 1 (encrypted-bundle-core) — CODE COMPLETE, gate green; formal completion awaits 2 live UATs
Plan: 11 of 11 (9 planned + 2 security remediations)
Status: Phase 1 done pending user UAT; Phase 2 starting
Last activity: 2026-08-19 — Phase 1 execution started
and reconciled, REQUIREMENTS.md (37 v1) and ROADMAP.md (6 phases) written

Progress: [█░░░░░░░░░] ~17% (1 of 6 phases)

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

### Pending Todos

None yet.

### Blockers/Concerns

- **Four assumptions are unverified and scheduled as calibrations**, not guesses: whether
  private-repo release assets honour `Range:` (CAL-1, Phase 1), Claude Desktop LevelDB
  compaction behaviour (CAL-2, Phase 2), Argon2id timings on slow aarch64 Linux (CAL-3,
  Phase 1), and the real compressed size of the 115 MB default bundle (CAL-4, Phase 2).
  Each has a named fallback so none can block its phase.

- **`.planning/` must not reach an upstream PR.** This repo is a fork of
  `akitaonrails/ai-usagebar`; use `/gsd-pr-branch` to produce a clean PR branch.

- **Phase 6 excludes the GNOME, KDE and Omarchy sync surfaces** (each is an independent
  frontend contract suite). Revisit if they should be in scope.

## Deferred Items

| Category | Item | Status | Deferred At |
|----------|------|--------|-------------|
| *(none)* | | | |

## Session Continuity

Last session: 2026-08-17
Stopped at: Milestone artifacts written (PROJECT, REQUIREMENTS, ROADMAP, STATE, research×3 +
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
