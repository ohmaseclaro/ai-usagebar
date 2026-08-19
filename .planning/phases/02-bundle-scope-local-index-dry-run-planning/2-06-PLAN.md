---
phase: 2-bundle-scope-local-index-dry-run-planning
plan: 06
type: execute
wave: 3
depends_on: ["2-02"]
files_modified:
  - tests/live.rs
  - docs/sync-calibration.md
  - docs/sync-format.md
autonomous: false
requires_phase_1_merged: true
requirements: [SCOPE-03]
user_setup:
  - service: claude-desktop
    why: "CAL-2 measures whether the Claude Desktop app's LevelDB compaction rewrites the 24 MB profile wholesale, which can only be observed across two real app restarts."
    dashboard_config:
      - task: "Quit and relaunch the Claude Desktop app between the two CAL-2 runs"
        location: "macOS — the Claude Desktop app itself"

must_haves:
  truths:
    - "CAL-2 reports the fraction of desktop-state chunks that survive a Claude Desktop restart unchanged, measured rather than assumed."
    - "CAL-4 reports the real zstd-compressed size of this machine's default bundle, measured rather than quoted from the research estimate."
    - "Both numbers are written down with the date and the machine they were measured on."
    - "Neither calibration runs during a plain `cargo test`."
    - "If either is unmeasurable, its named fallback is recorded instead, and the phase is not blocked."
  artifacts:
    - docs/sync-calibration.md
  key_links:
    - "CAL-2 uses the already-present `sha2`, not Phase 1's keyed BLAKE3, because it measures chunk *stability* across restarts — a property of the fixed 256 KiB boundaries, independent of which hash names them."
    - "CAL-4 runs Phase 1's zstd over the categories plan 2-02 collects, so the figure is the real default bundle and not a hand-picked sample."
---

<objective>
Answer the two measurements this phase owes: CAL-2, whether Claude Desktop's LevelDB
compaction rewrites the 24 MB profile wholesale between app restarts; and CAL-4, the real
zstd-compressed size of the default bundle on this machine.

Purpose: both are currently estimates. CAL-2 decides whether the credentials category
dominates daily sync cost, which the user should be told in `sync status` if it does. CAL-4 is
the number SCOPE-03 shows before the first push — the research's ~33 MB is derived from an
assumed 4-5x ratio, not from this machine's bytes.
Output: `docs/sync-calibration.md` with two dated numbers, and two `#[ignore]`d tests that can
be re-run when either answer goes stale.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/02-bundle-scope-local-index-dry-run-planning/2-CONTEXT.md
@.planning/ROADMAP.md
@.planning/research/chunking-storage.md
@CLAUDE.md
@tests/live.rs
</context>

<tasks>

<task type="auto">
  <name>Task 1: the CAL-2 chunk-stability probe</name>
  <files>tests/live.rs</files>
  <precondition>A macOS machine with the Claude Desktop app installed and at least one populated profile under `~/.claude-acc/profiles/<label>/desktop-state/`. Without it the probe has nothing to measure and must report that, not fabricate a number.</precondition>
  <read_first>tests/live.rs lines 1-40 (the `#[ignore]` convention, the `make smoke` entry point, and the "what gets tested" doc block this must be added to).</read_first>
  <action>
Add `#[ignore] #[test] fn cal2_desktop_state_chunk_stability()` to tests/live.rs, following the
file's existing convention: ignored by default so a plain `cargo test` — including the AUR
`check()` — never runs it, and documented in the header block alongside the vendor smoke tests.

The probe walks every file under a profile's `desktop-state/` directory, splits each at fixed
256 KiB offsets, and hashes each window with `sha2` — already a direct dependency. Phase 1's
keyed BLAKE3 is deliberately not used: the question is whether the *same offsets hold the same
bytes* across a restart, which is a property of the boundaries, not of the naming function. Using
sha2 keeps this probe independent of Phase 1 landing and adds no crate.

Take the profile root and the snapshot output path from env vars (`AI_USAGEBAR_CAL2_PROFILE`,
`AI_USAGEBAR_CAL2_SNAPSHOT`) rather than resolving a real home path in test code — the same
injected-root discipline the rest of the suite follows, and it lets the two runs write to two
files. With either var unset, the test reports what it needs and returns without asserting.

Behaviour: if the snapshot path holds no prior run, write `{path, window_index, hex_digest,
len}` for every window and print the file and byte totals. If it does, load it, re-hash, and
print: total windows, windows unchanged, windows changed, bytes changed, and files that
disappeared or appeared. Print no file contents and no path outside the profile root.
  </action>
  <verify>
    <automated>cargo test --test live cal2 -- --list</automated>
  </verify>
  <done>`cargo test --test live cal2 -- --list` shows the test as ignored. Running it with both env vars set against a seeded temp directory (no Claude Desktop needed) writes a snapshot on the first invocation and reports 100% unchanged on an immediate second invocation.</done>
</task>

<task type="checkpoint:human-verify" gate="blocking">
  <what-built>The CAL-2 probe. It needs two runs separated by a real Claude Desktop quit-and-relaunch, which no automated step can perform.</what-built>
  <how-to-verify>
1. Pick a populated profile: `ls ~/.claude-acc/profiles/`
2. First run:
   `AI_USAGEBAR_CAL2_PROFILE=~/.claude-acc/profiles/<label>/desktop-state AI_USAGEBAR_CAL2_SNAPSHOT=/tmp/cal2-a.json cargo test --test live cal2 -- --ignored --nocapture`
3. Fully quit the Claude Desktop app, relaunch it, sign in, let it settle for a minute, then quit it again.
4. Second run, same profile, same snapshot path:
   `AI_USAGEBAR_CAL2_PROFILE=~/.claude-acc/profiles/<label>/desktop-state AI_USAGEBAR_CAL2_SNAPSHOT=/tmp/cal2-a.json cargo test --test live cal2 -- --ignored --nocapture`
5. Paste the second run's summary line — total windows, unchanged, changed, bytes changed.

If the machine has no Claude Desktop install or no populated profile, say so. The named
fallback then applies: do not block the phase, and record instead that the category's churn
will be reported in `sync status` from real index data after a week of use.
  </how-to-verify>
  <resume-signal>Paste the summary numbers, or type "unmeasurable" to take the recorded fallback</resume-signal>
</task>

<task type="auto">
  <name>Task 2: the CAL-4 real compressed-size measurement, and both numbers written down</name>
  <files>tests/live.rs, docs/sync-calibration.md, docs/sync-format.md</files>
  <precondition>Phase 1 has merged, so `zstd` is a dependency of this crate. Without it CAL-4 cannot be measured; check `Cargo.toml` before starting and halt with that reason if it is absent.</precondition>
  <action>
Add `#[ignore] #[test] fn cal4_default_bundle_compressed_size()` to tests/live.rs. It resolves
`SyncRoots` for the real machine, collects the four default categories through
`sync::scope::collect` — the same collectors the product uses, so the figure is the real
default bundle and not a hand-picked sample — reads each file in 256 KiB windows, runs
`zstd` at level 3 over each window per Phase 1's compress-before-encrypt order, and sums the
compressed lengths. It prints, per category and in total: file count, raw bytes, compressed
bytes, and the achieved ratio. Reading real user data is why this is `#[ignore]`d and lives in
tests/live.rs, alongside the other tests that touch a real machine.

Add the +40 bytes of per-chunk AEAD overhead (24-byte nonce, 16-byte tag) per window so the
printed figure is the size that would actually be stored, not just the zstd output.

Write `docs/sync-calibration.md` with a short section per calibration: what was measured, the
exact command, the numbers, the date, and the machine (chip and OS version — no hostname, no
account identifier). For CAL-2, state plainly what the churn figure means for daily sync cost
and whether the credentials category needs its default revisited before Phase 4 ships a push.
For CAL-4, state the measured ratio against the research's estimated 4-5x, and note that
`sync push --dry-run` re-measures at runtime so no static number is ever quoted at a user.

If `docs/sync-format.md` exists (Phase 1's deliverable), append one line linking to
`docs/sync-calibration.md`. Do not restructure that file — Phase 1 owns it.

Take whichever fallback the checkpoint resolved for CAL-2, and record it as the outcome rather
than leaving the section empty.
  </action>
  <verify>
    <automated>cargo test --test live cal -- --list</automated>
  </verify>
  <done>Both calibration tests appear as ignored in the list. `docs/sync-calibration.md` carries a dated CAL-2 outcome (a churn number or the recorded fallback) and a dated CAL-4 measured compressed size with its ratio. A plain `cargo test` runs neither.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| real user profile store → test process | The probe reads live OAuth-bearing profile data on the developer's own machine. |
| measurement → committed docs | Numbers derived from real account data cross into a tracked file. |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|-----------|----------|-----------|----------|-------------|-----------------|
| T-2-25 | Information disclosure | CAL-2 probe output | critical | mitigate | The probe prints window digests, offsets and byte counts only. No file body, no path outside the injected profile root, and no account UUID or email is printed or written to the snapshot. |
| T-2-26 | Information disclosure | docs/sync-calibration.md | high | mitigate | Only aggregate counts, ratios, chip model and OS version are recorded. No hostname, no account label, no path under the user's home. |
| T-2-27 | Tampering | AUR `check()` | high | mitigate | Both tests are `#[ignore]`d and live in tests/live.rs, so `makepkg` running `cargo test` on a user's machine never reads their profiles. Asserted by the `-- --list` gate showing them as ignored. |
| T-2-28 | Repudiation | undated numbers | medium | mitigate | Each figure is recorded with its date, command and machine, so a later reader can tell a stale measurement from a current one — the research's undated estimate is exactly what this plan is replacing. |
| T-2-SC | Tampering | npm/pip/cargo installs | high | accept | This plan adds no dependency. CAL-2 uses the already-present `sha2`; CAL-4 uses Phase 1's `zstd`. `cargo machete` runs in the phase-end gate. |
</threat_model>

<verification>
`cargo test --test live cal -- --list` lists both tests as ignored. A plain `cargo test` does
not execute either. `docs/sync-calibration.md` holds two dated outcomes.
</verification>

<success_criteria>
CAL-2 and CAL-4 are answered with measured numbers, or with their named fallback explicitly
recorded — and in neither case did the measurement block the phase.
</success_criteria>

<output>
Create `.planning/phases/02-bundle-scope-local-index-dry-run-planning/2-06-SUMMARY.md` when done.
State whether CAL-2 changed the recommendation for the credentials category's default, since
that decision gates Phase 4's push.
</output>
