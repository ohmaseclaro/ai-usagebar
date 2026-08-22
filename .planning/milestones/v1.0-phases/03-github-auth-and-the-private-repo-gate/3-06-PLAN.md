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
requirements: [REPO-01]
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
  - service: github
    why: "The permissions probe — measuring what `permissions` reports for a correctly-scoped fine-grained token, so D-03's warning can rest on an observation. Read-only, creates nothing, and runs against the repository already paired in `sync setup`. Optional."
    env_vars:
      - name: GSD_PERM_TOKEN
        source: "The sync PAT already created per docs/sync-github.md — Contents: Read and write, Metadata: Read, nothing else"
      - name: GSD_PERM_REPO
        source: "The paired repository, as owner/name"
must_haves:
  truths:
    - "CAL-1 is either measured, with the answer recorded, or explicitly declined, with the fallback recorded as still standing."
    - "Neither outcome blocks the phase — declining is a first-class answer, not a failure."
    - "`sync::pack::PACK_TARGET` and its doc comment agree with whatever `docs/sync-format.md` now records."
    - "Phase 4 starts from a recorded answer rather than re-deriving the question."
    - "What `permissions` actually reports for a correctly-scoped fine-grained token is measured, so D-03's warning can later rest on an observation rather than an assumption."
  artifacts:
    - "docs/sync-format.md §CAL-1 replaced with a measured result or an explicit decline"
    - "tests/live.rs — the existing CAL-1 probe plus a new `#[ignore]`d permissions probe"
  key_links:
    - "PACK_TARGET's doc comment cites CAL-1 by name; changing one without the other leaves the constant justified by a stale claim"
    - "plan 3-04 parses `admin_permission` and deliberately warns on nothing; this probe is what decides whether that warning can ever be correct"
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

This plan also carries a second, smaller measurement for the same reason — it is the only place
in the milestone with both a client and a live credential. Plan 3-04 withheld D-03's
administrative-permission warning because `permissions.admin` most likely reports the *user's*
role on the repository rather than the *token's* grant, which would make the warning fire on
every correctly-configured install. That is a guess. Tasks 3 through 5 measure it.

**Neither measurement may block the phase.** Both fallbacks already ship and are already
documented. A decline is a complete, correct outcome for this plan.

Purpose: give Phase 4 a measured number instead of an assumption, twice.
Output: an updated `docs/sync-format.md` — §CAL-1 with `PACK_TARGET` reconciled, plus the
permissions answer and what it settles about D-03.
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

<task type="auto">
  <name>Task 3: Measure what `permissions` actually reports, so D-03's warning can be built on it</name>
  <files>tests/live.rs</files>
  <action>
Plan 3-04 parses `RepoFacts::admin_permission` from `permissions.admin` and deliberately emits
no warning from it, because the field's meaning is not established. For a classic token it
reports the **authenticated user's role on the repository**, not the token's granted
permissions — and D-01 has the user create the repository themselves, which makes them its
admin. So a correctly-scoped `Contents: read/write` token would very likely still read
`admin: true`, and a warning built on that fires on every correct install and teaches its
reader to ignore warnings. Whether a fine-grained PAT narrows the field is undocumented.

This is the one phase that has both an HTTP client and a live credential, so this is where the
question gets answered rather than guessed.

Add `#[ignore]`d `permissions_shape_for_a_fine_grained_contents_token` to `tests/live.rs`,
beside CAL-1 and following the same shape: read a token and an `owner/name` from two
environment variables, skip with a printed message when either is absent, and do a single
`GET` of the repository. Print the whole `permissions` object verbatim, plus `visibility`,
`private`, `archived`, and `fork` — those four are what plan 3-04 asserts on, and seeing them
against a real repository is free once the request is made. **Print nothing else from the
response**: it carries owner and repository metadata that has no business in a terminal
transcript, and the token must not appear at any point.

Assert only that the request succeeded, so a wrong scope fails loudly as a broken probe rather
than being recorded as an answer. The probe's output is the deliverable, not its assertions.

Document in the doc comment exactly which token to run it with — a fine-grained PAT with
`Contents: Read and write` and `Metadata: Read` and nothing else, on a repository the user
owns, which is the shape `docs/sync-github.md` tells every user to create. Running it with any
other token answers a different question.

Reuse CAL-1's existing `non_empty_var` helper and its user-agent constant rather than adding
new ones.
  </action>
  <verify>
    <automated>[ "$(cargo test --test live -- --ignored --list 2>/dev/null | grep -c 'permissions_shape_for_a_fine_grained_contents_token')" = 1 ]</automated>
  </verify>
  <done>The probe exists, is `#[ignore]`d, compiles, and skips cleanly with a printed message when its variables are unset. Its doc comment names the exact token shape to run it with and why any other token answers a different question.</done>
</task>

<task type="checkpoint:human-verify" gate="blocking">
  <name>Task 4: Run the permissions probe, or decline it</name>
  <what-built>`permissions_shape_for_a_fine_grained_contents_token` in `tests/live.rs`, from Task 3. It fetches one repository with a correctly-scoped fine-grained token and prints the `permissions` object plus the four fields plan 3-04's gate asserts on.</what-built>
  <how-to-verify>
You can run this with the token and repository you already paired in `sync setup` — unlike
CAL-1, it needs no throwaway anything and creates nothing. It is one read-only `GET`.

```bash
GSD_PERM_TOKEN=<your sync PAT> \
GSD_PERM_REPO=<owner>/<name> \
  cargo test --test live -- --ignored --nocapture \
    permissions_shape_for_a_fine_grained_contents_token
```

Report the `permissions` object it prints. The question it answers is narrow: **is `admin`
true for a token that was granted only `Contents: Read and write` and `Metadata: Read`?** If
it is true, the field reflects your role on the repository and cannot be used to detect an
over-permissioned token — plan 3-04's decision to ship no warning was right, and D-03's
warning is not implementable this way. If it is false, the field does narrow to the token's
grant and the warning becomes a one-line follow-up.

Reply `skip` to decline. The consequence is recorded, not hidden: D-03's runtime warning stays
unshipped and the token recipe in `docs/sync-github.md` remains its sole enforcement, which is
where its force actually lies.
  </how-to-verify>
  <resume-signal>Paste the printed `permissions` object, or reply "skip"</resume-signal>
</task>

<task type="auto">
  <name>Task 5: Record the permissions answer where the next phase will find it</name>
  <files>docs/sync-format.md</files>
  <action>
Add a short subsection under the calibration section recording what Task 4 produced, in the
same style as the CAL-1 entry beside it.

**Measured, `admin` is true.** Record it, and state the consequence plainly: `permissions.admin`
reflects the authenticated user's role on the repository, not the token's grant, so it cannot
detect an over-permissioned token. D-03's runtime warning is not implementable from this
endpoint and is closed as such — not deferred, closed, with the reason. Note that the token
recipe in `docs/sync-github.md` remains D-03's enforcement, which is the part that actually
determines the token's scope.

**Measured, `admin` is false.** Record it and state that the field does narrow to the token's
grant, so the warning plan 3-04 held back is implementable as a one-line addition to
`assert_pushable`'s warning list. Name that as a follow-up rather than doing it here: this plan
owns no source file under `src/sync/github/`, and reaching into one another plan owns is how a
wave breaks.

**Declined.** Say so, and say what stands as a result: no runtime warning, the recipe as sole
enforcement, and the question still open for whoever wants it.

In every case, write which token shape produced the answer. An answer from the wrong token
shape is not an answer to this question, and six months from now nobody will remember which
was used.
  </action>
  <verify>
    <automated>grep -qi 'permissions' docs/sync-format.md</automated>
  </verify>
  <done>`docs/sync-format.md` records the permissions answer, the token shape that produced it, and the resulting status of D-03's runtime warning — implementable and named as a follow-up, closed with its reason, or open because the probe was declined.</done>
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
| T-3-33b | Information disclosure | the permissions probe's output | medium | mitigate | It prints the `permissions` object and four named fields only, never the whole response body and never the token; it is run against the user's own already-paired repository, so it creates and exposes nothing new |
| T-3-33c | Repudiation | a warning shipped on an assumed API shape | high | mitigate | D-03's runtime warning is withheld until this probe measures the field. A warning that fires on every correct install trains its reader to ignore warnings, which is a worse security outcome than no warning — the probe exists so the choice rests on an observation |
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
thing. D-03's warning is either implementable and named as a follow-up, closed with a measured
reason, or open with the decline recorded — never quietly assumed either way.
</success_criteria>

<output>
Create `.planning/phases/03-github-auth-and-the-private-repo-gate/3-06-SUMMARY.md` when done.
State both outcomes in its first two lines — CAL-1 as measured-and-honoured,
measured-and-not-honoured, or declined; and the permissions answer as `admin` true, false, or
declined — so Phase 4's planner does not have to read further to size a pack, and whoever
picks up D-03 does not have to re-run the probe.
</output>
