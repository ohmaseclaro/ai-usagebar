---
phase: 03-github-auth-and-the-private-repo-gate
plan: 04
type: execute
wave: 2
depends_on: [3-01]
files_modified:
  - src/sync/github/gate.rs
  - src/sync/github/pairing.rs
autonomous: true
requirements: [REPO-01, REPO-03, SAFE-01, SAFE-02]
must_haves:
  truths:
    - "A repository reporting `private: false`, `visibility: \"internal\"`, `archived: true`, `fork: true`, a changed `owner.id`, or 404 is refused in each case with its own message and no clearance (D-04)."
    - "A repository that was private at pairing and is public on re-check produces the incident message, naming the credentials to rotate and stating that published bytes cannot be un-published (SAFE-02)."
    - "A token that carries administrative permission on the repository warns and continues; it never refuses (D-03)."
    - "The pairing record exists at mode 0600 in the config directory and is written atomically."
    - "The crate contains no reference to the repository-creation endpoint, in code or in a comment (REPO-03)."
  artifacts:
    - src/sync/github/gate.rs with the complete assertion set and the SAFE-02 incident path
    - src/sync/github/pairing.rs with the mode-0600 record and the drift check
  key_links:
    - "`assert_pushable` is the only constructor of `PushClearance`, and Phase 4's upload will take one by value — that is what makes the check structurally prior to the first byte"
    - "`owner_id` in the pairing record is what makes a delete-and-resquat of the repository name detectable; `owner.login` alone would not"
---

<objective>
Complete the gate. Plan 3-01 proved one assertion end to end; this plan adds the other five,
the pairing record they are checked against, the drift check, and the SAFE-02 incident path.

The assertion set is not a checklist of paranoia. Each item closes a specific hole:
`visibility` catches `"internal"`, which `private` reports as true and which is not private
enough for credential-bearing data. `owner.id` catches a delete-and-resquat of the repository
name, which `owner.login` cannot see. `archived` catches a repository that will reject the
write later, at a worse moment. `fork` catches a repository whose upstream relationship makes
its contents reachable in ways the owner did not intend.

Implements **D-03** (warn on excess permission, never refuse), **D-04** (checked immediately
before every push, never cached), and the structural half of **REPO-03**.

Purpose: this is the phase's reason to exist.
Output: a complete `gate.rs` and `pairing.rs`.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/03-github-auth-and-the-private-repo-gate/3-CONTEXT.md
@.planning/phases/03-github-auth-and-the-private-repo-gate/3-01-SUMMARY.md
@.planning/research/github-transport.md
@.planning/REQUIREMENTS.md
@CLAUDE.md
@src/cache.rs
</context>

<tasks>

<task type="auto" tdd="true">
  <name>Task 1: The complete assertion set, and the excess-permission warning</name>
  <files>src/sync/github/gate.rs</files>
  <behavior>
    - `private: false` refuses with a message naming the repository and stating it must be private.
    - `private: true` with `visibility: "internal"` refuses, and its message says internal is not private enough for credentials.
    - `archived: true` refuses with a message naming archiving as the reason.
    - `fork: true` refuses with a message naming the fork relationship as the reason.
    - `owner.login` differing from the configured owner refuses.
    - A fully-valid response yields a `PushClearance` whose `checked_at` equals the injected `now`.
    - Each of the six refusals produces a message distinct from the other five.
    - A valid response whose `permissions.admin` is true yields a clearance **and** a warning; it never refuses.
    - A response missing the `permissions` object yields a clearance and no warning.
  </behavior>
  <action>
Fill `assert_pushable` in `src/sync/github/gate.rs`, which plan 3-01 left asserting `private`
alone. Do not change its signature or `PushClearance`'s shape; plan 3-07 calls it and Phase 4
will take a `PushClearance` by value.

Assert all of: `private` is true; `visibility` equals the literal for private, rejecting the
internal value explicitly rather than by falling through a match, so the reason can be named;
`owner_login` equals `repo.owner` case-insensitively, since GitHub treats owner names that
way; `archived` is false; `fork` is false. Each failure carries its own message stating the
condition that failed and what to change. Six conditions, six messages, no shared "gate
failed" string — the phase's first success criterion is that each refuses distinctly.

Return `PushClearance { checked_at: now }` only when all of them hold. `assert_pushable` stays
the sole constructor.

Add the D-03 warning. `RepoFacts::admin_permission` comes from the response's
`permissions.admin`. When it is true, emit a warning saying the token appears to carry
administrative permission on this repository, that the design's guarantee comes from
*withholding* that permission, and that the user should re-issue a token with
`Contents: Read and write` and `Metadata: Read` only. It is a warning and never a refusal,
exactly as D-03 reasons: the effective permissions of a token cannot be reliably enumerated
without extra calls, and a false refusal is worse than an unheeded warning. Carry it out of
the function as a `Vec<String>` of warnings on `PushClearance`, or a separate returned value —
not by printing from inside, which would make it untestable.

Widen the 404 handling plan 3-01 wrote so its message is reachable from every entry point in
this module, and add a test asserting the message contains both possibilities and the create
command with the configured owner and name substituted.

**REPO-03 is enforced structurally, and this plan must not weaken it.** There is no request in
this crate to the endpoint that creates a repository under a user namespace. Do not write that
endpoint's path as a string literal anywhere under `src/` — not in a call, not in a test
fixture, and not in a comment explaining that it is never called. A gate in this plan's verify
step greps the tree for it, and a comment naming it is indistinguishable from a call site to
`grep`. Say what is true instead: the tool refuses and prints the command the user should run.

Every test constructs `RepoFacts` directly or serves one from `mockito`. No test reaches
`api.github.com`.
  </action>
  <verify>
    <automated>cargo test --lib sync::github::gate && [ "$(grep -rho 'user/repos' src/ --include='*.rs' | wc -l | tr -d ' ')" = 0 ]</automated>
  </verify>
  <done>Six refusals with six distinct messages, one clearance on a fully-valid response, and an administrative-permission warning that does not refuse. The grep gate finds no reference to the repository-creation endpoint anywhere under `src/`. `cargo test --lib sync::github::gate` is green.</done>
  <reversibility rating="one-way">Withholding repository-creation capability is the milestone's strongest structural guarantee: it is what makes creating a *public* repository impossible rather than merely disallowed. Adding that endpoint later would silently downgrade REPO-03 from a structural property to a runtime check, and the documented token permissions would no longer match what the code can do.</reversibility>
</task>

<task type="auto" tdd="true">
  <name>Task 2: The pairing record, the drift check, and the SAFE-02 incident</name>
  <files>src/sync/github/pairing.rs</files>
  <behavior>
    - `write_to` then `read_from` round-trips `{repo_id, owner_id, private, checked_at}`.
    - The written file is mode 0600, created in its destination directory, and replaces any previous record atomically.
    - A record absent from disk is `Ok(None)` — first pairing is not an error.
    - A corrupt or truncated record is reported as such and does not deserialize into a default that would silently pass the drift check.
    - Facts whose `owner_id` differs from the record's refuse, with a message naming the resquat possibility.
    - Facts whose `id` differs from the record's refuse.
    - A record saying `private: true` against facts saying public produces the incident message, distinct from the plain not-private refusal.
    - The incident message names the credential categories in the bundle and states that already-published bytes cannot be un-published.
    - With the credentials category off, a public repository warns and is allowed, rather than raising an incident.
  </behavior>
  <action>
Fill `src/sync/github/pairing.rs`, which plan 3-01 created with its doc comment only.

`pub struct Pairing { pub repo_id: u64, pub owner_id: u64, pub private: bool, pub checked_at: DateTime<Utc> }`,
serialized as JSON. `pub fn read_from(path: &Path) -> Result<Option<Pairing>>` and
`pub fn write_to(path: &Path, pairing: &Pairing) -> Result<()>` take the path explicitly —
that injected seam is what keeps every test off a real config directory, exactly as
`Cache::at` and `creds::read_from` already do. A thin `pub fn default_path(roots: &SyncRoots) -> PathBuf`
resolving it inside the *config* directory is the only production wrapper, and no test calls
it. The config directory, not the cache: a wiped cache must not silently reset the identity
this record defends.

Write atomically at mode 0600 — `tempfile::NamedTempFile::new_in` the destination's own
directory, `persist()`, then an explicit `set_permissions`. Never `/tmp`. This is the
convention `cache.rs` and the Settings overlay already use, and the reasons are in `CLAUDE.md`.

A missing file is `Ok(None)`: first contact establishes the pairing and is a normal path, not
a failure. A file that exists but does not parse is an error naming the file, never a
silently-defaulted record — a `Default` here would let a corrupted record wave through exactly
the substitution the record exists to detect.

`pub fn check_drift(record: Option<&Pairing>, facts: &RepoFacts, credentials_in_bundle: bool, now: DateTime<Utc>) -> Result<DriftOutcome>`
is the plan's core. With no record, return the outcome that says to record a fresh pairing.
With a record: a differing `owner_id` or `repo_id` refuses, naming that the repository at this
name is not the one this machine paired with and that a name can be released and re-registered
by someone else. `owner_id` is what makes that detectable; a login can be given up and taken.

Then SAFE-02. When the record says the repository was private and the facts say it is public
now, this is an incident, not a configuration error, and its message must differ from the
plain not-private refusal — a repository that was *never* private was never trusted with
anything. The incident message says three things, in this order: the backup repository is now
public; make it private again; and rotate every credential the bundle carries, because a
previous push may have landed while it was public and published bytes cannot be un-published.
Name the credential categories rather than gesturing at "your credentials". Rotation advice is
the only correct response here, not politeness — say it plainly and do not soften it.

When `credentials_in_bundle` is false, a public repository is a warning rather than an
incident and the outcome allows continuing, per D-04's closing paragraph: chat indexes and
config are still personal data, so it is warned about, but there is no credential to rotate.

`DriftOutcome` carries enough for the caller to act: whether to proceed, the warnings, and the
pairing to persist on success. Plan 3-07 wires it to `sync setup`; nothing here prints.

Every timestamp comparison takes the injected `now`.
  </action>
  <verify>
    <automated>cargo test --lib sync::github::pairing</automated>
  </verify>
  <done>The record round-trips at mode 0600, a missing one is `Ok(None)`, a corrupt one errors. A changed `owner_id` or `repo_id` refuses. A private-to-public transition with credentials in the bundle produces the incident message naming the categories to rotate and the un-publishable bytes, distinct from the plain refusal; with credentials off it warns and proceeds. `cargo test --lib sync::github::pairing` is green and no test resolves a real config directory.</done>
  <reversibility rating="costly">The pairing record's JSON shape is on-disk state on a user's machine. A later field rename makes an existing record unreadable, and a record that cannot be read is a pairing check that silently degrades to first-contact trust — the exact resquat window it exists to close.</reversibility>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|---|---|
| GitHub response → gate decision | Attacker-controlled JSON deciding whether credentials may be uploaded |
| pairing record on disk → gate decision | Local state an attacker with filesystem access could edit to wave a substitution through |
| repository identity over time | The same `owner/name` can point at a different repository after a delete and re-registration |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|---|---|---|---|---|---|
| T-3-19 | Spoofing | `assert_pushable` | critical | mitigate | Both `private` and `visibility` are asserted, so an `"internal"` repository — which reports `private: true` — is rejected explicitly and by name |
| T-3-20 | Spoofing | repository re-registration | high | mitigate | `owner_id` and `repo_id` are compared against the pairing record, so releasing and re-taking a name is detected; a login comparison alone would not see it |
| T-3-21 | Information disclosure | a repository flipped public between syncs | critical | mitigate | The gate is a call with no cached result and `PushClearance` is neither `Clone` nor publicly constructible, so D-04's "immediately before every push" cannot be satisfied by a stale check |
| T-3-22 | Information disclosure | credentials already pushed while public | critical | transfer | Cannot be mitigated in software — published bytes are published. The incident message names the categories to rotate and states plainly that this is not undoable (SAFE-02) |
| T-3-23 | Tampering | the pairing record on disk | high | mitigate | Written atomically at mode 0600 in the config directory, not the wipeable cache; a corrupt record errors rather than defaulting into a passing check |
| T-3-24 | Elevation of privilege | a token carrying administrative permission | medium | mitigate | Warned about by name, with the two permissions to re-issue with. Not a refusal, per D-03: effective token permissions cannot be reliably enumerated and a false refusal is worse than an unheeded warning |
| T-3-25 | Elevation of privilege | repository creation | critical | mitigate | No request to that endpoint exists anywhere in the crate, enforced by a grep gate in this plan's verify step; the documented token withholds the permission that would allow it (REPO-03) |
</threat_model>

<verification>
- `cargo test --lib sync::github::gate sync::github::pairing` is green.
- `grep -rho 'user/repos' src/ --include='*.rs'` produces no output.
- `PushClearance` still derives neither `Clone` nor `Copy` and still has no public
  constructor outside `assert_pushable`.
- No test in either file resolves a real config directory or reaches the network.
</verification>

<success_criteria>
Every one of the six refusal conditions from `github-transport.md` §3.2 is asserted and refused
by its own message; a valid private repository yields a clearance; a repository that turned
public since pairing raises the incident rather than a generic error; and the crate is
provably incapable of creating a repository.
</success_criteria>

<output>
Create `.planning/phases/03-github-auth-and-the-private-repo-gate/3-04-SUMMARY.md` when done.
Record the final `DriftOutcome` shape, the pairing record's JSON field names, and the exact
incident message — plan 3-07 renders it and Phase 4 re-runs this gate before its flip.
</output>
