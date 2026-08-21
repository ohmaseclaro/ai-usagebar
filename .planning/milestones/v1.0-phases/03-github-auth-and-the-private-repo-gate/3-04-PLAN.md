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
    - "With the credentials category off, a public repository warns and is allowed — and `assert_pushable` and `check_drift` agree about that rather than one overruling the other (D-04)."
    - "The pairing record exists at mode 0600 in the config directory and is written atomically."
    - "No repository-creating endpoint is reachable from the crate, and the check that says so runs on every future `make test`, not once at this plan's execution instant (REPO-03)."
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

Implements **D-04** (checked immediately before every push, never cached, and public-with-
credentials aborts while public-without-credentials warns) and the structural half of
**REPO-03**. **D-03**'s runtime warning is deliberately *not* shipped here — see Task 1; its
enforcement in this phase is the token recipe plan 3-05 writes, and plan 3-06 probes the field
the warning would need.

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
    - A fully-valid response yields a `PushClearance` whose `checked_at` equals the injected `now`, paired with an empty warning list.
    - Each of the six refusals produces a message distinct from the other five.
    - `private: false` with `credentials_in_bundle: false` yields a clearance **and** a warning naming the repository as public; it does not refuse.
    - `private: false` with `credentials_in_bundle: true` refuses, and its message differs from the credentials-off warning.
    - `visibility: "internal"` refuses in **both** bundle configurations — internal is not a public-repository carve-out, it is a repository whose visibility we do not accept at all.
  </behavior>
  <action>
Fill `assert_pushable` in `src/sync/github/gate.rs`, which plan 3-01 left asserting `private`
alone. **Do not touch its signature.** Plan 3-01 froze it as
`assert_pushable(facts, repo, credentials_in_bundle, now) -> Result<(PushClearance, Vec<String>)>`
precisely so this plan can add both assertions and warnings without one: your worktree contains
3-01's `setup.rs`, which calls it and which you do not own, so a signature change means your
branch does not compile. Warnings go in the tuple's `Vec<String>`, which 3-01 left empty and
`setup.rs` already destructures and renders.

Assert: `visibility` equals the literal for private, rejecting the internal value explicitly
rather than by falling through a match, so the reason can be named; `owner_login` equals
`repo.owner` case-insensitively, since GitHub treats owner names that way; `archived` is false;
`fork` is false. Each failure carries its own message stating the condition that failed and
what to change — no shared "gate failed" string, since the phase's first success criterion is
that each refuses distinctly.

**The `private` arm is where this function and `check_drift` have to agree**, and
`credentials_in_bundle` is what lets them. Read D-04 exactly as written: a public repository
aborts *when credentials are in the bundle*, and is allowed-with-a-warning when the credentials
category is off, because chat indexes and config are still personal data but there is nothing
to rotate. So `private == false` refuses when `credentials_in_bundle`, and otherwise returns a
clearance plus a warning naming the repository as public. Without this the two functions
contradict each other and Task 2's credentials-off carve-out is dead code: `setup.rs` calls
`check_drift` then `assert_pushable`, and an unconditional refusal here would kill it.

The internal-visibility arm takes no such carve-out and refuses in both configurations. It is
not a public repository we are tolerating; it is a visibility we do not accept.

Return `PushClearance { checked_at: now }` with the warning list only when the refusing
conditions all pass. `assert_pushable` stays its sole constructor.

**The D-03 administrative-permission warning does not ship in this plan**, and this is a
deliberate reading of D-03 rather than an omission. `RepoFacts::admin_permission` comes from
`permissions.admin` on `GET /repos/{owner}/{repo}` — but for a classic token that field
reports the **authenticated user's role on the repository**, not the token's granted
permissions. D-01 and `docs/sync-github.md` both instruct the user to create the repository
themselves, which makes them its admin, so a correctly-scoped `Contents: read/write` token
would still see `admin: true` and the warning would fire on essentially every correct install.
Whether a fine-grained PAT narrows this field is undocumented. A warning that always fires
trains its reader to ignore it, which is worse than no warning — the same reasoning D-03 itself
uses to prefer an unheeded warning over a false refusal, applied one step further.

So: keep parsing `admin_permission` (it is data, and free), emit no warning from it, and leave
a `ponytail:`-style comment naming the open question and pointing at the probe. Plan 3-06 adds
an `#[ignore]`d probe beside CAL-1 that dumps the `permissions` object for a real fine-grained
Contents-only token. If it shows the field narrows, enabling the warning is a one-line
follow-up against a measured answer. D-03's real force is in the recipe, which plan 3-05
already delivers: the token is scoped correctly because the document says exactly which two
permissions to grant.

Widen the 404 handling plan 3-01 wrote so its message is reachable from every entry point in
this module, and add a test asserting the message contains both possibilities and the create
command with the configured owner and name substituted.

Every test constructs `RepoFacts` directly or serves one from `mockito`. No test reaches
`api.github.com`.
  </action>
  <verify>
    <automated>cargo test --lib sync::github::gate</automated>
  </verify>
  <done>`assert_pushable`'s signature is byte-identical to 3-01's. Six refusal conditions with six distinct messages; a fully-valid response yields a clearance and an empty warning list; `private: false` refuses with credentials in the bundle and warns-and-clears without them; internal visibility refuses in both. No warning is emitted from `admin_permission`, and the open question is recorded in a comment naming plan 3-06's probe. `cargo test --lib sync::github::gate` is green.</done>
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

<task type="auto" tdd="true">
  <name>Task 3: REPO-03 as a standing test, not a one-time grep</name>
  <files>src/sync/github/gate.rs</files>
  <behavior>
    - The guard walks every `.rs` file under `src/`, excluding only the file it lives in, and passes on the tree as it stands.
    - Injecting any one of the four forbidden path fragments into a scratch string the guard is pointed at makes it fail, with a message naming the file and the fragment.
    - The guard reads its root from `CARGO_MANIFEST_DIR`, not from a working directory or a home directory.
  </behavior>
  <action>
The previous draft enforced REPO-03 with a shell `grep` in this plan's verify step. That has
two holes, and the second is the serious one.

**It ran once.** At this plan's execution instant, and never again. Nothing would stop Phase 4,
5, or 6 from reintroducing a creation path — and REPO-03 is a property of the shipped crate,
not of one afternoon. Promote it to a `#[test]` in this file so `make test` carries it forward
every phase, forever, at the cost of one directory walk.

**It matched one path out of four.** Creating a repository under a user namespace is only the
first way. There are three more: creating under an *organization* namespace — which D-01
explicitly permits as an owner, so this is a live route, not a hypothetical; generating a
repository from a template; and forking one. **Forking is the dangerous one:** a fork of a
public upstream is public, which is precisely the outcome REPO-03 exists to make impossible.
A guard that catches only the first path is a guard that would not have caught the worst case.

Write the test as a plain walk, no new crate and no regex. Root it at
`Path::new(env!("CARGO_MANIFEST_DIR")).join("src")` — a compile-time constant, so this reads no
`$HOME` and does not depend on the working directory, and the AUR `check()` has the source tree
in place. Recurse with `std::fs::read_dir`, take every `.rs` file, and reject any whose contents
contain any of the four forbidden fragments, using `str::contains` four times. Fail with the
offending file path and which fragment matched.

**Exclude this file from the walk.** The test's own source necessarily contains all four
fragments as the things it searches for, so a guard that scanned itself would fail on the day
it was written. Skip by comparing against `file!()`, and say in a comment that the exclusion is
deliberate and is the reason the fragments may appear here and nowhere else in `src/`.

That exclusion is also the rule for everyone else: do not write any of those four path
fragments anywhere under `src/` — not in a call, not in a test fixture, and not in a comment
explaining that the endpoint is never called. To this guard a comment is indistinguishable from
a call site. Say what is true instead: the tool refuses and prints the command the user should
run.

The structural half of REPO-03 is plan 3-01's guard that `Client` exposes no method able to
carry a request body — none of these four endpoints is reachable without one. This test is the
belt to that's braces, and cheap enough that having both is right.
  </action>
  <verify>
    <automated>cargo test --lib sync::github::gate</automated>
  </verify>
  <done>A `#[test]` in `gate.rs` walks `src/` from `CARGO_MANIFEST_DIR`, excludes only itself, checks all four repository-creating path fragments, and passes. Temporarily adding any one of them to another file under `src/` makes it fail with that file named — verify this by hand once and record it in the summary. The test appears in `cargo test --lib` and therefore in `make test`, so every later phase inherits it.</done>
  <reversibility rating="one-way">This test is what makes REPO-03 durable rather than momentary. Deleting or weakening it later would silently downgrade "the tool cannot create a repository" from a property of the crate to a claim in a document, and the documented token permissions would no longer match what the code is able to do.</reversibility>
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
| T-3-24 | Elevation of privilege | a token carrying administrative permission | medium | accept (this phase) | The runtime signal is unreliable: `permissions.admin` reports the *user's* role on the repository for a classic token, and D-01 has the user create the repository, so a correct token would trigger it on nearly every install and train its reader to ignore it. Mitigated instead by the recipe in `docs/sync-github.md` (plan 3-05), which is what actually determines the token's scope. Plan 3-06 probes the real field shape; the warning ships when it rests on a measurement |
| T-3-25 | Elevation of privilege | repository creation | critical | mitigate | All four creating paths — user namespace, organization namespace, template generation, and **forking**, which produces a public repository — are blocked by a standing `#[test]` that walks `src/` on every `make test`, not by a one-time grep. Backed structurally by plan 3-01's guard that `Client` exposes no request-body method, without which none of the four is reachable (REPO-03) |
</threat_model>

<verification>
- `cargo test --lib -- sync::github::gate sync::github::pairing` is green. (The multi-filter
  form needs the `--` separator; `cargo test --lib a b` takes one positional and errors.)
- The REPO-03 guard test is part of `cargo test --lib`, so `make test` runs it in every later
  phase.
- `assert_pushable`'s signature is unchanged from plan 3-01's summary, and `PushClearance`
  still derives neither `Clone` nor `Copy` and still has no public constructor outside it.
- `assert_pushable` and `check_drift` agree on the credentials-off carve-out: a test drives
  `check_drift` then `assert_pushable` in `setup.rs`'s order over a public repository with the
  credentials category off and asserts the run is allowed.
- No test in either file resolves a real config directory or reaches the network.
</verification>

<success_criteria>
Every one of the six refusal conditions from `github-transport.md` §3.2 is asserted and refused
by its own message; a valid private repository yields a clearance; the credentials-off carve-out
survives both functions rather than being overruled by the second; a repository that turned
public since pairing raises the incident rather than a generic error; and the crate is provably
incapable of creating a repository by any of the four routes, on every future test run.
</success_criteria>

<output>
Create `.planning/phases/03-github-auth-and-the-private-repo-gate/3-04-SUMMARY.md` when done.
Record the final `DriftOutcome` shape, the pairing record's JSON field names, and the exact
incident message — plan 3-07 renders it and Phase 4 re-runs this gate before its flip.

Record two things for later phases: that `src/` may not contain any of the four
repository-creating path fragments and why the guard test excludes its own file, and that the
D-03 administrative-permission warning is outstanding pending plan 3-06's probe, so Phase 4's
planner does not assume it shipped.
</output>
