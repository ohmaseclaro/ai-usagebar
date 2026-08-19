---
phase: 03-github-auth-and-the-private-repo-gate
plan: 05
type: execute
wave: 1
depends_on: []
files_modified:
  - docs/sync-github.md
  - docs/configuration.md
  - README.md
autonomous: true
requirements: [REPO-01, REPO-03, REPO-04]
must_haves:
  truths:
    - "A user can create the token by following the document without guessing a single permission (D-03)."
    - "The document states that the tool never creates a repository, and gives the command that does (D-01, REPO-03)."
    - "All four token sources are documented in the order they are tried, including the headless-over-SSH case (D-02)."
    - "The reason `keyring`/`secret-service` was rejected is recorded, so it is not re-litigated by the next reader."
    - "The document says plainly that this phase uploads nothing, so a reader knows what `sync setup` does and does not do (D-05)."
  artifacts:
    - docs/sync-github.md — the pairing and token document
    - a sync section in README.md linking to it
    - "`[sync] repo` documented in docs/configuration.md"
  key_links:
    - "docs/sync-github.md is the only place the exact permission set is written down; getting it wrong hands the user an over-privileged token"
---

<objective>
Write the fine-grained PAT recipe and the pairing document.

This plan has no code dependency and touches no file any other plan in the phase touches, so
it runs in wave 1 alongside the tracer. Everything it documents is already locked in
`3-CONTEXT.md`: the repository is named by the user, the permission set is exactly two
entries, the token resolution order is fixed, and this phase uploads nothing. It documents
those decisions, not an implementation.

Implements **D-01** (the create command the tool prints instead of acting), **D-02** (the
resolution order and the rejected alternative), **D-03** (the exact permission set),
**D-05** (nothing is uploaded here).

Purpose: an over-privileged token is the failure this whole phase is arranged to prevent, and
a vague document is the most likely way to get one.
Output: `docs/sync-github.md`, a README section, a `[sync] repo` entry in the config doc.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/03-github-auth-and-the-private-repo-gate/3-CONTEXT.md
@.planning/research/github-transport.md
@.planning/REQUIREMENTS.md
@CLAUDE.md
@docs/configuration.md
@docs/sync-format.md
@README.md
</context>

<tasks>

<task type="auto">
  <name>Task 1: `docs/sync-github.md` — the PAT recipe and the pairing rules</name>
  <files>docs/sync-github.md</files>
  <action>
Write `docs/sync-github.md`, following the house style of `docs/sync-format.md` and
`docs/configuration.md`: prose that explains the reason before the step, no marketing, and
every claim either true today or explicitly marked as belonging to a later phase.

**Create the repository yourself.** State first, not last, that the tool never creates a
repository, and give both routes: the `gh repo create <owner>/<name> --private` command and
the GitHub web form with the private option preselected. Explain why in one sentence — the
token deliberately holds no permission that could create one, which is what makes creating a
*public* repository structurally impossible rather than merely disallowed. A reader who
understands that will not file a bug asking for auto-creation.

**The token recipe**, step by step, precise enough to follow without judgement calls:
GitHub → Settings → Developer settings → Personal access tokens → Fine-grained tokens →
Generate new token; **Repository access → Only select repositories** → the one backup
repository; **Repository permissions → Contents → Read and write**; **Metadata → Read-only**,
which GitHub selects automatically and cannot be removed. Nothing else. State explicitly that
the administrative permission tier must **not** be granted, and that the tool warns if it
detects one on the repository — a warning, not a refusal, because a token's effective
permissions cannot be reliably enumerated and a false refusal would be worse than an unheeded
warning. Also state that the token is scoped to one repository so a leak is not a skeleton key
across everything the user owns; this application sits next to their AI provider credentials.

**Where the token is stored**, in the order it is looked for: the `AI_USAGEBAR_SYNC_TOKEN`
environment variable; the macOS Keychain item; a mode-0600 file under the config directory on
Linux and elsewhere; and `gh auth token` if the GitHub CLI happens to be installed. Say what
each is for — the environment variable is the override for CI and for restoring over SSH, and
`gh` is convenience that is never required. Say that the token is never written into
`config.toml`, and why: the inline-API-key convention there is a deliberate choice for
read-only provider keys, and a token that can write to a repository is a different class of
secret.

Record the rejected alternative and its reason: a D-Bus secret service needs a live session
and unlocked keyring, so it fails headless and over SSH — which is precisely the restore case
this feature exists to serve. One paragraph, so the next reader does not re-open it.

**What is checked, and when.** List the conditions a repository must satisfy: private, with a
visibility that is private rather than internal; owned by the configured owner, whose numeric
id matches the one recorded at first pairing; not archived; not a fork. Say that the check runs
immediately before every push and is never cached, because a repository can be made public from
the web interface at any moment. Say what happens if it is found public after having been
private: the operation aborts, and the user is told to make it private again and to rotate the
credentials in the bundle, because a previous push may have landed while it was public and
published bytes cannot be un-published.

**What `sync setup` does not do.** It authenticates, resolves the repository, and verifies
visibility. It uploads nothing. Pushing arrives in a later release; say so rather than leaving
a reader to infer it from silence.

Close with the operational caveat from the research: GitHub's acceptable-use policy reserves
the right to throttle or suspend accounts for bandwidth use significantly out of line with
comparable users, and a frequently-rewritten multi-gigabyte bundle fits that profile. There is
no rule against backing up to a private repository, but the user should know the shape of the
risk rather than discover it.
  </action>
  <verify>
    <automated>test -f docs/sync-github.md && grep -qi 'only select repositories' docs/sync-github.md && grep -qi 'read and write' docs/sync-github.md && grep -q 'AI_USAGEBAR_SYNC_TOKEN' docs/sync-github.md && grep -q 'gh repo create' docs/sync-github.md && grep -qi 'rotate' docs/sync-github.md</automated>
  </verify>
  <done>`docs/sync-github.md` exists and a reader can produce a correctly-scoped token from it without guessing. It names the two permissions and no others, gives the repository-creation command, lists all four token sources in order, records the rejected alternative with its reason, states the full check set and that it re-runs before every push, gives the rotation instruction for the public-repository case, and states that nothing is uploaded in this release.</done>
</task>

<task type="auto">
  <name>Task 2: `[sync] repo` in the config reference, and the README entry point</name>
  <files>docs/configuration.md, README.md</files>
  <action>
In `docs/configuration.md`, document the `[sync]` section's `repo` key beside the sections
already there, matching their format exactly. Value shape `owner/name`, no default, and a
statement that a missing value is an error rather than something the tool resolves for the
user, with the reason: the tool holds no permission that could create the repository, so a
guessed name could only ever produce a confusing not-found error. Naming it is a one-time
explicit act. If plan 2-01's `[sync]` keys are already documented there, extend that entry
rather than starting a second one.

Cross-reference `docs/sync-github.md` for the token, and state that the token is deliberately
*not* a config key.

In `README.md`, add a short sync section in the style of the existing ones — a few sentences
on what the feature is for, the two commands that exist today, and links to
`docs/sync-github.md` and `docs/sync-format.md`. Keep it short; the README is an index, not a
manual. Do not claim a capability that does not ship yet: describe pairing and status, not
pushing.
  </action>
  <verify>
    <automated>grep -q 'sync-github.md' README.md && grep -q 'sync-github.md' docs/configuration.md && grep -q 'repo' docs/configuration.md</automated>
  </verify>
  <done>`docs/configuration.md` documents `[sync] repo` with its shape, its absence of a default, and the reason there is none, and points at `docs/sync-github.md` for the token. `README.md` carries a sync section linking to both documents and claims no capability beyond pairing and status.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|---|---|
| documentation → the token the user creates | The document is the only control over how much authority the issued token carries |
| documentation → the user's expectations | An overstated capability leads a user to believe data is backed up when it is not |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|---|---|---|---|---|---|
| T-3-26 | Elevation of privilege | the PAT recipe | high | mitigate | The permission set is enumerated exactly and the administrative tier is called out as one that must not be granted; a vague recipe is the most likely route to an over-privileged token |
| T-3-27 | Elevation of privilege | repository access scope | high | mitigate | "Only select repositories" is named as a required step, so a leaked token's blast radius is one repository rather than everything the user owns |
| T-3-28 | Repudiation | a document claiming a capability that does not ship | medium | mitigate | The document states that this release uploads nothing, so a user cannot believe their data is backed up when only pairing has happened |
| T-3-29 | Information disclosure | a documented example carrying a real token | high | mitigate | No example anywhere in the document contains a token-shaped literal; placeholders are named as placeholders |
</threat_model>

<verification>
- Both automated checks above pass.
- `grep -rniE 'ghp_[A-Za-z0-9]|github_pat_[A-Za-z0-9]' docs/sync-github.md README.md
  docs/configuration.md` finds no token-shaped literal.
- The document contains no instruction that would result in a permission beyond the two
  named.
</verification>

<success_criteria>
A user who has never used a fine-grained token can follow `docs/sync-github.md` and end up
with a token scoped to exactly one repository carrying exactly two permissions, a private
repository they created themselves, and an accurate understanding that this release pairs and
verifies but does not upload.
</success_criteria>

<output>
Create `.planning/phases/03-github-auth-and-the-private-repo-gate/3-05-SUMMARY.md` when done.
Note any place the document had to describe behaviour plans 3-01 through 3-04 had not merged
yet, so plan 3-07's verification can reconcile the command name and config key against what
actually shipped.
</output>
