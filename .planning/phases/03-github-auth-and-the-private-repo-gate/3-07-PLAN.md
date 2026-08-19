---
phase: 03-github-auth-and-the-private-repo-gate
plan: 07
type: execute
wave: 3
depends_on: [3-01, 3-02, 3-03, 3-04]
files_modified:
  - src/sync/github/setup.rs
  - src/sync/report.rs
  - src/sync/cli.rs
autonomous: true
requirements: [UX-03, REPO-02, REPO-05, SAFE-01, SAFE-02]
must_haves:
  truths:
    - "`sync setup` against a mock private repo walks repo → password → categories → size → ready, stores the token, and exits zero (UX-03)."
    - "The same command against each refusal case exits non-zero with that case's message and never offers to create a repository."
    - "The token appears in no rendered line, no config file, and no process argument at any verbosity (REPO-02)."
    - "`sync status` reports the repository, its visibility, the token's source, and when the pairing was last verified."
    - "A repository that has turned public since pairing is reported by `sync status` as an incident, not a generic error (SAFE-02)."
  artifacts:
    - src/sync/github/setup.rs — the guided flow behind an injected prompt seam
    - src/sync/report.rs extended with the repository, visibility, token-source, and drift lines
    - src/sync/cli.rs wiring both surfaces
  key_links:
    - "The flow reaches the password step only after the gate has cleared — a user must not be asked to set a passphrase for a repository we are about to refuse"
    - "The size shown is Phase 2's plan-builder total, not a second estimate computed here"
---

<objective>
Make `sync setup` the guided command UX-03 asks for, and extend `sync status` with what this
phase now knows.

Everything under it exists by the time this plan runs: plan 3-01's client and CLI surface,
3-02's token chain and storage, 3-03's actionable failures, 3-04's full gate and pairing
record, Phase 1's passphrase generation and keyfile, and Phase 2's category selection and
plan-builder byte totals. This plan is the wiring and the ordering — and the ordering is the
substance. The gate runs before the user is asked for anything, because asking someone to
choose a passphrase for a repository that is about to be refused wastes their time and teaches
them the refusal is negotiable.

Implements **D-01**, **D-04**, **D-05** (the flow ends at "ready to push", and pushes nothing),
and **D-06** at every exit.

Purpose: the first-run experience is where an over-privileged token or a public repository
would actually get accepted.
Output: a complete `setup.rs`, an extended `report.rs`, both wired through `cli.rs`.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/03-github-auth-and-the-private-repo-gate/3-CONTEXT.md
@.planning/phases/03-github-auth-and-the-private-repo-gate/3-01-SUMMARY.md
@.planning/phases/03-github-auth-and-the-private-repo-gate/3-02-SUMMARY.md
@.planning/phases/03-github-auth-and-the-private-repo-gate/3-03-SUMMARY.md
@.planning/phases/03-github-auth-and-the-private-repo-gate/3-04-SUMMARY.md
@.planning/phases/01-encrypted-bundle-core/1-01-SUMMARY.md
@.planning/phases/01-encrypted-bundle-core/1-05-SUMMARY.md
@.planning/phases/02-bundle-scope-local-index-dry-run-planning/2-05-SUMMARY.md
@.planning/phases/02-bundle-scope-local-index-dry-run-planning/2-07-SUMMARY.md
@CLAUDE.md
@src/tui/settings.rs
@src/sync/passphrase.rs
</context>

<tasks>

<task type="auto" tdd="true">
  <name>Task 1: The guided flow, behind an injected prompt seam</name>
  <files>src/sync/github/setup.rs</files>
  <behavior>
    - A scripted run against a mock private repo reaches "ready to push" and returns a success outcome.
    - The same script against a mock repo reporting `private: false` stops before the password prompt is ever called — asserted by a prompt double that records which prompts were reached.
    - A 404 stops at the same point and its message carries the create command.
    - A run in which the user accepts the generated passphrase writes the keyfile at mode 0600 in the injected config directory, and the passphrase is displayed exactly once with the no-recovery warning.
    - A run in which the user supplies a passphrase under the minimum length is refused by Phase 1's strength gate and re-prompted, not accepted.
    - Toggling a category off in the flow lands in the injected `config.toml` and is readable back by `Config::load_from`.
    - The confirmed size shown equals the plan builder's total for the chosen categories, not a separately computed number.
    - The rendered outcome contains no substring of the token and no substring of the passphrase.
    - Re-running setup on an already-paired machine reuses the existing pairing rather than issuing a second one, and says so.
  </behavior>
  <action>
Expand `github::setup::run` behind the entry point plan 3-01 froze, into UX-03's five steps.
Keep the signature, adding one parameter: an injected prompt seam.

Define `pub trait SetupPrompt` with the few methods the flow needs — confirm a yes/no with a
default, choose a passphrase (accept the generated one or supply your own), toggle the category
set, and confirm the computed size. A `TtyPrompt` implements it over the terminal for
production; every test uses a scripted double that records which methods were reached, in
order. That recording is what lets the ordering itself be asserted, which is the point of this
plan. Do not read from stdin anywhere outside `TtyPrompt`.

**Step 1 — the repository, and the gate.** Read `cfg.repo`; absent is the D-01 error naming the
config key and the create command. Parse it, resolve the token through `TokenChain`, build the
`Client`, `fetch_facts`, read the pairing record, `check_drift`, then `assert_pushable`. Every
refusal exits here, before any prompt is shown, with the message plan 3-03 or 3-04 produced —
do not re-word them at the call site, or the phase acquires two copies of every message and one
of them rots. Render `assert_pushable`'s warnings, including the administrative-permission one,
before continuing.

**Step 2 — the passphrase.** Only reachable once the gate has cleared. Use Phase 1's
`sync::passphrase` generation as the default path and its strength floor for a supplied one;
do not re-implement either, and do not lower the floor. Display the generated passphrase once,
with Phase 1's plain-language explanation of the offline attack and the statement that there is
no recovery. Never accept a passphrase from a command-line argument or an environment variable —
Phase 1's rule, and it is not relaxed here. Then create the keyfile through the entry point
`sync::crypto` exposes and write it into the injected config directory: atomically, via
`NamedTempFile::new_in` that directory, `persist()`, then an explicit mode 0600. It stays local;
Phase 4 uploads it. If a keyfile already exists, do not silently overwrite it — an overwritten
keyfile is an unrecoverable bundle. Say what exists and stop.

**Step 3 — the categories.** Present Phase 2's category set with its defaults, let the user
toggle, and write the result back into `config.toml` with `toml_edit`, preserving comments and
key order the way `src/tui/settings.rs` already does. The credentials category is shown
explicitly rather than buried, because syncing credentials is the deliberate override recorded
in the research summary and the user should see it being turned on.

**Step 4 — the size.** Call Phase 2's plan builder over the chosen categories and show its
per-category file and byte totals plus the overall figure. Use that number; do not compute a
second estimate here, or the two will disagree and the user will not know which to believe.
Then confirm.

**Step 5 — ready.** Store the token through `token::store`, persist the pairing record through
`pairing::write_to`, and print that the machine is paired and ready to push, together with what
would be sent. Say explicitly that nothing has been uploaded — D-05, and a user who has just
been through a five-step wizard will otherwise assume something was.

Re-running on a paired machine reuses the existing pairing when the identity still matches, and
says so rather than re-issuing one.

Nothing in this file prints a token, a passphrase, or a keyfile byte. Assert that with a test
over the rendered outcome, checking for both the value and its eight-character prefix.
  </action>
  <verify>
    <automated>cargo test --lib sync::github::setup</automated>
  </verify>
  <done>The five steps run in order against a scripted prompt double and a mockito private repository, ending at "ready to push" with nothing uploaded. Every refusal case stops before the password prompt is reached, asserted by the double's recording. The keyfile and the pairing record are written at mode 0600 in the injected directory. No rendered line contains the token or the passphrase or an eight-character prefix of either.</done>
  <precondition>Plans 3-02, 3-03, and 3-04 are merged: this flow calls `token::store`, `http::actionable`, `gate::assert_pushable`, and `pairing::check_drift`, none of which is complete at the tracer.</precondition>
</task>

<task type="auto" tdd="true">
  <name>Task 2: `sync status` learns about the repository</name>
  <files>src/sync/report.rs, src/sync/cli.rs</files>
  <behavior>
    - With no `[sync] repo` configured, status prints an unconfigured line naming the config key and still renders every category line Phase 2 produced.
    - With a repo configured and a mock reporting private, status prints the repository, `private`, the token's source, and when the pairing was last verified.
    - With a mock reporting public against a pairing record that says private, status prints the incident line and exits non-zero.
    - With no token resolvable, status prints that no token was found and names the four places one is looked for, and exits non-zero.
    - Status never prints the token, only its source label.
    - Status makes exactly one request to the repository endpoint per invocation — asserted by the mock's call count, so the gate is not accidentally run twice per command.
  </behavior>
  <action>
Extend `src/sync/report.rs`'s `StatusReport` from plan 2-01 with a repository section: the
configured `owner/name` or an unconfigured marker, the visibility as reported, the token's
`TokenSource` label, the pairing record's `checked_at`, and any drift outcome. Keep the existing
split the file already uses — a pure model built by a `build_*` function, and a renderer over
it — so every one of these lines is assertable without a terminal. Add the repository fields to
the existing builder rather than introducing a parallel report type.

The repository section is best-effort with respect to the *category* lines: a network failure
or a missing token must still leave Phase 2's category listing rendered, with the repository
section reporting why it could not be filled. A user whose token expired should still be able to
see what would be sent. It is not best-effort with respect to the **exit code**: a failure in
the repository section exits non-zero, per D-06 and REPO-05.

A drift outcome that is the SAFE-02 incident renders as the incident, using plan 3-04's message
verbatim — not a status-flavoured paraphrase of it. A repository going public is the same event
whichever command noticed it.

In `src/sync/cli.rs`, extend the `Status` arm to build the client and drive the repository
section when `[sync] repo` is set, and to skip it silently when it is not — an unconfigured
machine reporting a network error would be a lie. Return non-zero for the failure cases above.
Do not run the gate twice in one invocation; fetch the facts once and pass them to both the
drift check and the report.

Reconcile against `docs/sync-github.md` from plan 3-05, which was written in wave 1 before any
of this merged: confirm the command name, the config key, the token source labels, and the
environment variable name in that document match what actually shipped, and fix the document if
they diverged. Do not change the shipped names to match the document — the document is the one
that was written ahead of the code.
  </action>
  <verify>
    <automated>cargo test --lib sync::report sync::cli</automated>
  </verify>
  <done>`sync status` renders the repository, visibility, token source, and last-verified time; reports the incident distinctly when the repository turned public; keeps the category listing visible when the repository section cannot be filled; exits non-zero on any repository-section failure; and issues exactly one repository request per invocation. `docs/sync-github.md` agrees with the shipped command name, config key, and token source labels.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|---|---|
| user input at the prompt → config and keyfile | A passphrase and a category set enter durable local state |
| gate result → the rest of the flow | Everything after step 1 assumes the repository was verified private |
| rendered outcome → terminal and shell scrollback | Two secrets are in scope during the flow: the token and the passphrase |
| `config.toml` write-back → an existing file | A user's hand-edited config is being rewritten |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|---|---|---|---|---|---|
| T-3-35 | Elevation of privilege | flow ordering | critical | mitigate | The gate runs before any prompt; a refusal exits before a passphrase is chosen, asserted by a prompt double that records which prompts were reached |
| T-3-36 | Information disclosure | rendered outcome | critical | mitigate | Tests assert neither the token nor the passphrase, nor an eight-character prefix of either, appears in any rendered line |
| T-3-37 | Information disclosure | passphrase entry | critical | mitigate | Accepted only from the prompt, stdin, or a mode-0600 file, per Phase 1's rule — never a command-line argument, never an environment variable |
| T-3-38 | Denial of service | overwriting an existing keyfile | critical | mitigate | An existing keyfile stops the flow with what exists and why; overwriting one makes the corresponding bundle permanently unreadable, and there is no recovery by design |
| T-3-39 | Tampering | `config.toml` write-back | medium | mitigate | `toml_edit` in place, preserving comments and key order, exactly as the Settings overlay already does; the file's mode protection in `Config::load_from` is unchanged |
| T-3-40 | Repudiation | a user believing data was uploaded | high | mitigate | The closing line states explicitly that nothing has been uploaded (D-05); a five-step wizard otherwise implies it |
| T-3-41 | Repudiation | a failure reported as success | critical | mitigate | Every repository-section failure exits non-zero even though the category listing still renders; the two concerns are deliberately separated |
</threat_model>

<verification>
- `cargo test --lib sync::github::setup sync::report sync::cli` is green.
- No test drives a real terminal, reads a real `$HOME`, or reaches the network.
- The rendered success output of both commands is asserted free of the token and the
  passphrase.
- `docs/sync-github.md` and the shipped command name, config key, and source labels agree.
</verification>

<success_criteria>
`ai-usagebar sync setup` takes a user from an unconfigured machine to a paired one — repository
chosen and verified private, passphrase set with its keyfile written locally at mode 0600,
categories chosen, size confirmed against Phase 2's own number, token stored where only they can
read it — and says plainly that nothing was uploaded. Every refusal case stops before the
passphrase step with its own actionable message and a non-zero exit. `sync status` then reports
the pairing, and reports it as an incident if the repository has since become public.
</success_criteria>

<output>
Create `.planning/phases/03-github-auth-and-the-private-repo-gate/3-07-SUMMARY.md` when done.
Record the `SetupPrompt` trait, the `SetupOutcome` shape, and the local keyfile path — Phase 4
uploads that keyfile and re-runs this gate before its flip.
</output>
