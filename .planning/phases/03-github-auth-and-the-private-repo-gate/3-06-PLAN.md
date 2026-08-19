---
phase: 03-github-auth-and-the-private-repo-gate
plan: 06
type: execute
wave: 3
depends_on: [3-01]
files_modified:
  - tests/live.rs
  - docs/sync-format.md
  - src/sync/pack.rs
autonomous: false
requirements: [REPO-06]
user_setup:
  - service: github
    why: "CAL-1 only — probing whether a private-repo release asset honours a Range request after the redirect to signed storage. Optional; the phase completes on the documented 32 MiB fallback if it is not run."
    env_vars:
      - name: GSD_CAL1_TOKEN
        source: "A fine-grained PAT scoped to one throwaway private repo, Contents: Read"
      - name: GSD_CAL1_REPO
        source: "The throwaway repository, as owner/name"
      - name: GSD_CAL1_ASSET
        source: "The file name of a release asset over 1 MiB on that repository's latest release"
    dashboard_config:
      - task: "Create a throwaway private repository, cut one release, and attach an asset over 1 MiB"
        location: "github.com -> New repository (private) -> Releases -> Draft a new release"
must_haves:
  truths:
    - "CAL-1 is either measured, with the answer recorded, or explicitly declined, with the fallback recorded as still standing."
    - "Neither outcome blocks the phase — declining is a first-class answer, not a failure."
    - "`sync::pack::PACK_TARGET` and its doc comment agree with whatever `docs/sync-format.md` now records."
    - "Phase 4 starts from a recorded answer rather than re-deriving the question."
  artifacts:
    - "docs/sync-format.md §CAL-1 replaced with a measured result or an explicit decline"
    - "tests/live.rs — the existing probe, unchanged unless the run showed it to be wrong"
  key_links:
    - "PACK_TARGET's doc comment cites CAL-1 by name; changing one without the other leaves the constant justified by a stale claim"
---

<objective>
Answer CAL-1, or record that it was declined.

Phase 1 wrote the `#[ignore]`d probe `cal1_range_on_private_release_asset` in `tests/live.rs`
and could not run it: Phase 1 was offline by construction and had no GitHub credential. This
phase has both an HTTP client and, by the time this plan runs, a user who has just been through
`sync setup`. So it is the first place the probe can actually be executed.

It decides Phase 4's pack sizing. If a ranged read against a private-repo release asset is
honoured after the redirect to signed storage, one chunk can be fetched out of a large pack and
packs may grow. If it is not, fetching one chunk means fetching its whole pack, and the 32 MiB
fallback already recorded in `docs/sync-format.md` and shipped as `sync::pack::PACK_TARGET`
stands unchanged.

**This must not block the phase.** The fallback already ships and is already documented. A
decline is a complete, correct outcome for this plan.

Purpose: give Phase 4 a measured number instead of an assumption.
Output: an updated `docs/sync-format.md` §CAL-1, and `PACK_TARGET` reconciled with it.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/03-github-auth-and-the-private-repo-gate/3-CONTEXT.md
@.planning/phases/01-encrypted-bundle-core/1-08-SUMMARY.md
@.planning/research/github-transport.md
@docs/sync-format.md
@tests/live.rs
@src/sync/pack.rs
@CLAUDE.md
</context>

<tasks>

<task type="checkpoint:human-verify" gate="blocking">
  <name>Task 1: Run CAL-1, or decline it</name>
  <what-built>`cal1_range_on_private_release_asset` in `tests/live.rs`, written in Phase 1 plan 1-08. It looks up the latest release on a private repository, finds a named asset, issues a request for the asset's API URL carrying a `Range` header for the first kilobyte, follows the redirect to signed storage by hand — deliberately, so the GitHub token is never replayed to a storage host that neither needs nor should see it — and prints the resulting status, any `Content-Range`, and the number of bytes actually received.</what-built>
  <how-to-verify>
This probe needs credentials the phase's automated tests deliberately do not have, and an
asset this phase cannot upload, because Phase 3 uploads nothing.

**Setup** — a throwaway private repository with one release carrying an asset a little over
1 MiB (large enough that a whole-body 200 is unmistakable, small enough to finish inside the
timeout), and a fine-grained token scoped to it with `Contents: Read`. Use a throwaway
repository rather than the one you just paired: this probe reads an asset, and there is no
reason to point a hand-rolled probe at your real backup target.

**Run it:**

```bash
GSD_CAL1_TOKEN=<throwaway token> \
GSD_CAL1_REPO=<owner>/<throwaway-repo> \
GSD_CAL1_ASSET=<asset file name> \
  cargo test --test live -- --ignored --nocapture cal1_range_on_private_release_asset
```

**Read the last line it prints.** It says either that `Range` is honoured — a 206 with a
`Content-Range` header — or that it is not, with the byte count showing the whole asset came
back. Report that line, plus the status and `Content-Range` values above it.

**Declining is a valid answer.** Reply `skip` and the recorded 32 MiB fallback stands
unchanged; Phase 4 proceeds on it exactly as planned. Do not create a repository or issue a
token you do not want to, and do not leave one lying around afterwards: delete the repository
and revoke the token when you are done.

If the probe fails with a 401 or a 404, that is a broken setup, not an answer about `Range` —
fix the token scope or the repository name and re-run, or reply `skip`.
  </how-to-verify>
  <resume-signal>Paste the probe's final lines, or reply "skip" to keep the documented fallback</resume-signal>
</task>

<task type="auto">
  <name>Task 2: Record the answer, and reconcile the constant with it</name>
  <files>docs/sync-format.md, src/sync/pack.rs, tests/live.rs</files>
  <action>
Replace the `### CAL-1` section in `docs/sync-format.md`, which currently reads as not
measured, with whichever of these the checkpoint produced.

**Measured, and ranged reads are honoured.** Record the date, the status code, the
`Content-Range` value, and the bytes received, and state that a partial restore is therefore
possible. Then decide `PACK_TARGET` on the evidence rather than reflexively raising it: a
larger pack is strictly better for upload-request count, and with ranged reads working it costs
nothing on restore. If you raise it, update `sync::pack::PACK_TARGET` **and** `PACK_MAX`
together with the doc comment that currently cites CAL-1's fallback as their justification —
leaving that comment in place beside a changed number is worse than not changing it, because
the next reader will trust the stale reason. Re-run the pack tests. If you leave it at 32 MiB,
say why in one sentence, so the next reader knows it was a decision and not an oversight.

**Measured, and ranged reads are not honoured.** Record the same evidence and state that the
32 MiB target stands because it does, now with a measurement behind it rather than an
assumption. Update `PACK_TARGET`'s doc comment to cite the measurement rather than the
fallback. The value does not change.

**Declined.** Say so plainly: CAL-1 was offered in Phase 3 and not run, the assumption remains
that ranged reads are not honoured, and 32 MiB stands. Name Phase 5 as the next place the
question can be answered, since that is the first phase that actually fetches a pack. Do not
write it as if it had been measured.

In every case, update the cross-reference in `docs/sync-format.md`'s calibration index so the
document does not contradict itself in two places, and leave `tests/live.rs` unchanged unless
the run showed the probe itself to be wrong — if it was, fix it and say what was wrong, because
a probe that does not work is worse than one that was never run.
  </action>
  <verify>
    <automated>cargo test --lib sync::pack && [ "$(cargo test --test live -- --ignored --list 2>/dev/null | grep -c 'cal1_range_on_private_release_asset')" = 1 ] && grep -qi 'CAL-1' docs/sync-format.md</automated>
  </verify>
  <done>`docs/sync-format.md` §CAL-1 records a measured answer with its evidence, or an explicit decline naming Phase 5 as the next opportunity. `PACK_TARGET` and its doc comment agree with what the document now says. The probe still exists and still compiles. `cargo test --lib sync::pack` is green.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|---|---|
| operator token → the probe | A real GitHub credential enters a test process |
| GitHub → signed storage redirect | The redirect target is a third-party host that must never receive the GitHub token |
| terminal output → the transcript | A signed storage URL carries its credential in the query string |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|---|---|---|---|---|---|
| T-3-30 | Information disclosure | the redirect to signed storage | high | mitigate | The probe sets an explicit no-redirect policy and re-issues the second request by hand without the `Authorization` header, so the GitHub token is never replayed to a storage host |
| T-3-31 | Information disclosure | the probe's printed output | high | mitigate | Only the redirect target's **host** is printed, never the full signed URL, whose query string is itself a credential |
| T-3-32 | Information disclosure | the operator's token in a shell history or transcript | medium | mitigate | A throwaway token on a throwaway repository, revoked and deleted afterwards, and never the token paired in `sync setup`; the checkpoint instructs both |
| T-3-33 | Repudiation | a declined calibration recorded as a measurement | high | mitigate | The decline wording is prescribed explicitly and differs from the measured wording; the document must not read as though a number were obtained |
| T-3-34 | Tampering | a constant changed without its justification | medium | mitigate | `PACK_TARGET` and its CAL-1-citing doc comment are changed together or not at all — a stale reason beside a new number misleads every later reader |
</threat_model>

<verification>
- `cargo test --lib sync::pack` is green, and the default `cargo test` set still makes no
  network call.
- `docs/sync-format.md` states one CAL-1 outcome, not two, and its calibration index agrees
  with its CAL-1 section.
- `cargo clippy --all-targets -- -D warnings` is clean if `PACK_TARGET` changed.
</verification>

<success_criteria>
Phase 4 begins with CAL-1 either answered and evidenced, or explicitly and visibly unanswered
with the fallback restated. The 32 MiB target and the sentence that justifies it say the same
thing.
</success_criteria>

<output>
Create `.planning/phases/03-github-auth-and-the-private-repo-gate/3-06-SUMMARY.md` when done.
State the outcome in its first line — measured and honoured, measured and not honoured, or
declined — so Phase 4's planner does not have to read further to size a pack.
</output>
