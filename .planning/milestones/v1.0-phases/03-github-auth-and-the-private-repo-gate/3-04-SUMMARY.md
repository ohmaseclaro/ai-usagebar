---
phase: 03-github-auth-and-the-private-repo-gate
plan: 04
subsystem: transport
tags: [private-repo-gate, pairing-record, safe-02, repo-03, standing-guard, hermetic-tests]

requires:
  - phase: 03-github-auth-and-the-private-repo-gate
    plan: 01
    provides: "`RepoFacts`, `fetch_facts`, the frozen `assert_pushable` signature, `PushClearance`, `RepoRef`, `SyncRoots` threading"
  - codebase
    provides: "`cache::atomic_write`, `AppError::io_at`, `sync::anchor`'s read/write/0600 shape, `SyncCategory`"
provides:
  - "`assert_pushable` complete — six refusal conditions, six distinct messages, D-04's credentials carve-out"
  - "`Pairing` + `read_from` / `write_to` / `default_path` — mode-0600 JSON in the *config* directory"
  - "`check_drift` + `DriftOutcome` — resquat detection and SAFE-02's incident"
  - "`no_repository_creating_endpoint_is_reachable_from_the_crate` — REPO-03 as a standing `#[test]`"
  - "`gate::missing_repo_message` promoted to `pub(crate)`"
affects: [3-07, phase-4-push]

tech-stack:
  added: []
  patterns:
    - "A guard that asserts an absence excludes its own source and proves the exclusion happened (`skipped == 1`) as well as that the walk found something (`scanned > 50`). Verified red for all four fragments, then reverted."
    - "`DriftOutcome` has no `proceed` flag: a refusal is an `Err`, so *holding* the value is the permission to continue — the same shape as `PushClearance`."
    - "`pairing.rs` is `anchor.rs`'s read/write/0600 shape reused verbatim rather than reinvented; both are durable state in the config directory, not the wipeable cache."

key-files:
  created: []
  modified:
    - src/sync/github/gate.rs
    - src/sync/github/pairing.rs

key-decisions:
  - "The D-03 administrative-permission warning **does not ship**. `permissions.admin` on `GET /repos/{owner}/{repo}` reports the authenticated *user's* role on the repository, not the token's granted permissions, and D-01 has the user create the repository — so a correctly-scoped `Contents: read/write` PAT would fire it on essentially every legitimate install. The field is parsed and bound to `let _` with a comment naming plan 3-06's probe."
  - "`visibility` is matched by name, not by falling through a catch-all: `\"internal\"` gets a refusal that explains it reports `private: true` and is readable by the whole enterprise. An unrecognised value gets its own refusal rather than being waved through."
  - "Check order inside `assert_pushable` is `private` → `visibility` → `owner_login` → `archived` → `fork`. The D-04 arm runs first because it is the one carrying rotation advice; the `visibility` assertion is guarded on `facts.private` so the credentials-off public carve-out is not immediately overruled by it."
  - "Owner comparison is `eq_ignore_ascii_case` — GitHub treats account names case-insensitively, and a case-differing login is the same account, not a substitution."
  - "First contact does **not** warn about a public repository. `assert_pushable` owns that message and runs on the same path; two warnings for one fact is noise."
  - "The organization fragment in the REPO-03 guard is `/orgs/`, deliberately broader than `POST /orgs/{org}/repos`. The crate calls no organization endpoint at all, so the broad form costs nothing and catches spellings a `format!` would break into pieces."

requirements-completed: [REPO-01, REPO-03, SAFE-01, SAFE-02]

coverage:
  - id: SAFE-01
    description: "Six refusal conditions, each with its own actionable message"
    verification:
      - kind: unit
        ref: "src/sync/github/gate.rs#all_six_refusals_say_six_different_things"
        status: pass
      - kind: unit
        ref: "src/sync/github/gate.rs#an_internal_repository_refuses_in_both_bundle_configurations"
        status: pass
      - kind: unit
        ref: "src/sync/github/gate.rs#a_renamed_or_transferred_owner_refuses_and_case_does_not_matter"
        status: pass
      - kind: unit
        ref: "src/sync/github/gate.rs#an_archived_repository_refuses_before_the_write_that_would_fail_later"
        status: pass
      - kind: unit
        ref: "src/sync/github/gate.rs#a_fork_refuses_because_it_shares_its_upstreams_object_network"
        status: pass
    human_judgment: false
  - id: SAFE-02
    description: "A repository private at pairing and public on re-check raises the incident, names the categories to rotate, and states the bytes cannot be un-published"
    verification:
      - kind: unit
        ref: "src/sync/github/pairing.rs#a_private_to_public_transition_is_an_incident_naming_what_to_rotate"
        status: pass
      - kind: unit
        ref: "src/sync/github/pairing.rs#the_incident_differs_from_the_gates_plain_not_private_refusal"
        status: pass
    human_judgment: false
  - id: D-04
    description: "The credentials-off carve-out survives both functions, in `setup.rs`'s call order"
    verification:
      - kind: unit
        ref: "src/sync/github/pairing.rs#with_credentials_off_a_repository_that_went_public_warns_through_both_checks"
        status: pass
      - kind: unit
        ref: "src/sync/github/gate.rs#a_public_repository_warns_and_clears_when_credentials_are_not_in_the_bundle"
        status: pass
    human_judgment: false
  - id: REPO-03
    description: "No repository-creating endpoint is reachable from the crate, checked on every `make test` rather than once"
    verification:
      - kind: unit
        ref: "src/sync/github/gate.rs#no_repository_creating_endpoint_is_reachable_from_the_crate"
        status: pass
      - kind: manual
        ref: "negative control — each of the four fragments injected into a scratch `src/*.rs` turns the test red naming that file and fragment; reverted, green"
        status: pass
    human_judgment: false
  - id: T-3-20
    description: "A delete-and-resquat of the repository name is detected by numeric id, not by login"
    verification:
      - kind: unit
        ref: "src/sync/github/pairing.rs#a_changed_owner_id_refuses_and_names_the_resquat"
        status: pass
      - kind: unit
        ref: "src/sync/github/pairing.rs#a_changed_repo_id_refuses_distinctly_from_a_changed_owner"
        status: pass
    human_judgment: false
  - id: T-3-23
    description: "The pairing record is mode 0600 in the config directory, written atomically, and a corrupt one errors"
    verification:
      - kind: unit
        ref: "src/sync/github/pairing.rs#write_then_read_round_trips_and_the_file_is_owner_only"
        status: pass
      - kind: unit
        ref: "src/sync/github/pairing.rs#a_rewrite_replaces_the_record_and_keeps_it_owner_only"
        status: pass
      - kind: unit
        ref: "src/sync/github/pairing.rs#a_corrupt_record_errors_instead_of_deserialising_into_a_passing_default"
        status: pass
    human_judgment: false
  - id: T-3-24
    description: "The administrative-permission warning is deliberately not shipped"
    verification:
      - kind: unit
        ref: "src/sync/github/gate.rs#an_administrative_permission_produces_no_warning_yet"
        status: partial
    notes: "Accepted for this phase by design, not missed. Plan 3-06's `#[ignore]`d probe measures the real field shape against a fine-grained Contents-only token; the warning ships only when it rests on that measurement. Phase 4 must not assume it exists."
    human_judgment: false

duration: 40min
completed: 2026-08-19
status: complete
---

# Phase 3 / Plan 04: The Private-Repo Gate, Complete

**The safety boundary of the milestone.** Six conditions refuse with six
different messages; a repository that turned public since pairing raises an
incident that names what to rotate; and the crate is provably unable to create a
repository by any of the four routes — checked on every future `make test`, not
once at this plan's execution instant.

## Task Commits

1. **`5614e86`** — the complete assertion set: six refusals, six messages, one carve-out
2. **`c527db5`** — the pairing record, the drift check, and SAFE-02's incident
3. **`5c06f13`** — REPO-03 as a standing test, not a one-time grep

## `assert_pushable` — signature unchanged, six conditions

Byte-identical to what plan 3-01 froze. `setup.rs` was not touched.

```rust
pub fn assert_pushable(
    facts: &RepoFacts,
    repo: &RepoRef,
    credentials_in_bundle: bool,
    now: DateTime<Utc>,
) -> Result<(PushClearance, Vec<String>)>;
```

| condition | outcome |
|---|---|
| `private == false` **and** `credentials_in_bundle` | refuse, naming rotation and the un-publishable bytes |
| `private == false` **and not** `credentials_in_bundle` | **clear**, with one warning naming the repository as public |
| `private == true` **and** `visibility == "internal"` | refuse in **both** bundle configurations |
| `private == true` **and** `visibility` is neither `"private"` nor `"internal"` | refuse — unrecognised, not waved through |
| `owner_login` ≠ `repo.owner` (ASCII case-insensitive) | refuse |
| `archived` | refuse |
| `fork` | refuse |
| 404 from `fetch_facts` | `missing_repo_message` — now `pub(crate)` |

Order is `private` → `visibility` → `owner_login` → `archived` → `fork`. The
`visibility` assertion is guarded on `facts.private`, so the credentials-off
public arm pushes its warning and continues instead of being immediately
overruled by "visibility is not private".

`PushClearance` is untouched: still no `Clone`, no `Copy`, no public
constructor, and `assert_pushable` is still its sole constructor.

## The pairing record — JSON field names, frozen

`src/sync/github/pairing.rs`. These four names are on-disk state on a user's
machine; renaming one makes an existing record unreadable, and an unreadable
record is a pairing check that degrades to first-contact trust.

```rust
pub struct Pairing {
    pub repo_id: u64,       // RepoFacts::id
    pub owner_id: u64,      // RepoFacts::owner_id — what sees a resquat
    pub private: bool,      // what it was at pairing; SAFE-02 compares against this
    pub checked_at: DateTime<Utc>,
}
impl Pairing { pub fn of(facts: &RepoFacts, now: DateTime<Utc>) -> Pairing; }
```

Serialized as flat JSON with exactly those keys (serde derive, no rename
attributes). On disk as `<config_dir>/sync-pairing.json`.

```rust
pub fn default_path(roots: &SyncRoots) -> PathBuf;   // the only production wrapper; no test calls it
pub fn read_from(path: &Path) -> Result<Option<Pairing>>;
pub fn write_to(path: &Path, pairing: &Pairing) -> Result<()>;
```

- **Config directory, not the cache.** A wiped cache must not silently reset the
  identity this record defends.
- Written through `cache::atomic_write` (tempfile in the destination's *own*
  directory, `persist`), then `set_permissions(0o600)` explicitly — the same
  belt-and-braces `sync::anchor` and the Settings overlay already apply. Mode
  survives a rewrite, and the destination directory is created if absent.
- A missing file is `Ok(None)`. A present-but-unparseable file is an **error**
  naming the path; there is deliberately no `Default`.

## `DriftOutcome` — the final shape

```rust
pub fn check_drift(
    record: Option<&Pairing>,
    facts: &RepoFacts,
    credentials_in_bundle: bool,
    now: DateTime<Utc>,
) -> Result<DriftOutcome>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriftOutcome {
    pub record: Pairing,        // persist this on success — refreshed, or the first pairing
    pub first_contact: bool,    // nothing was compared; not an error
    pub warnings: Vec<String>,
}
```

**There is no `proceed` field.** A refusal is an `Err`, so holding a
`DriftOutcome` *is* the permission to continue — the same shape as
`PushClearance`, for the same reason. Plan 3-07 renders `warnings` and persists
`record`; nothing in this module prints.

| record | facts | result |
|---|---|---|
| `None` | anything | `first_contact: true`, no warnings — `assert_pushable` owns the public warning on this same path |
| `owner_id` differs | — | `Err` — the name was released and re-registered |
| `repo_id` differs | — | `Err`, distinct — the original was deleted and something else created under the name |
| `private: true` | `private: false`, credentials **in** bundle | `Err(went_public_incident())` |
| `private: true` | `private: false`, credentials **off** | one warning, proceeds |
| `private: false` | `private: false` | clean — a repository that was never private was never trusted |

## The exact incident message (SAFE-02)

Plan 3-07 renders this verbatim and Phase 4 re-runs the check before its flip.

```
STOP — the backup repository was private when this machine paired with it, and it is public now. Nothing was uploaded by this run.

1. Make the repository private again, in its GitHub settings.
2. Rotate every credential the bundle carries. A previous push may already have landed while the repository was public, and bytes that were published cannot be un-published:
   - the saved Claude Desktop logins — each profile's `config-tokenCache`, `config-tokenCacheV2` and `desktop-state/` in the claude-acc profile store: sign out and sign in again on every account.
   - the Claude OAuth credentials in `accounts/*/.credentials.json` beside config.toml: re-run `claude login` for each account.
   - any provider API key held inline in config.toml: re-issue it at the provider.

Rotating is the only thing that undoes this. Making the repository private again does not: whoever read it while it was public still holds what they read.
```

(Rendered; the source is one string literal with `\n` line breaks in
`pairing::went_public_incident`.) It is asserted distinct from the gate's plain
not-private refusal, and the order — what happened, make it private, rotate — is
asserted by byte offset rather than by reading.

## For every later phase: two standing rules

### 1. `src/` may not contain these four path fragments

The REPO-03 guard lives in `gate.rs` as
`no_repository_creating_endpoint_is_reachable_from_the_crate`. It walks every
`.rs` file under `Path::new(env!("CARGO_MANIFEST_DIR")).join("src")` — a
compile-time constant, so it reads no `$HOME` and does not depend on the working
directory, and the AUR `check()` has the source tree in place.

```
"/user/repos"   POST /user/repos
"/orgs/"        POST /orgs/{org}/repos — deliberately broader; the crate calls no org endpoint at all
"/generate"     POST /repos/{owner}/{repo}/generate
"/forks"        POST /repos/{owner}/{repo}/forks — a fork of a public upstream is public
```

**Do not write any of these anywhere under `src/`** — not in a call, not in a
test fixture, and **not in a comment explaining that the endpoint is never
used.** To a substring guard a comment and a call site are indistinguishable.
Say what is true instead: the tool refuses and prints the
`gh repo create <owner>/<name> --private` line.

**The guard excludes exactly one file: its own** (`path.ends_with(file!())`).
The four fragments are the things it searches for, so a guard that scanned its
own source would have failed on the day it was written. That exclusion is
asserted (`skipped == 1`) so a moved file cannot silently turn the guard into a
no-op, and the walk is asserted non-vacuous (`scanned > 50`).

Negative control, run by hand: each of the four fragments written into a scratch
`.rs` file under `src/` turns the test red with that file and fragment named;
removed, green. This is the difference between a guard and a comment.

### 2. The D-03 administrative-permission warning is **outstanding**

It did not ship here, deliberately. `RepoFacts::admin_permission` is still
parsed from `permissions.admin` and is bound to `let _` with the reasoning in
place. Phase 4's planner must not assume the warning exists.

Why: for a classic token that field reports the **authenticated user's role on
the repository**, not the token's granted permissions — and D-01 and
`docs/sync-github.md` both instruct the user to create the repository
themselves, which makes them its admin. A correctly-scoped `Contents:
read/write` PAT would therefore fire the warning on essentially every legitimate
install, and a warning that always fires trains its reader to ignore it. Whether
a fine-grained PAT narrows the field is undocumented.

Plan **3-06** adds an `#[ignore]`d probe beside CAL-1 that dumps the
`permissions` object for a real fine-grained Contents-only token. If it shows
the field narrows, enabling the warning is a one-line follow-up against a
measured answer. D-03's real force is already delivered by plan 3-05's recipe,
which names exactly the two permissions to grant.

## Verification

```
cargo test --lib -- sync::github::gate sync::github::pairing   # 29 passed, 0 failed
cargo test --lib sync::                                        # 240 passed, 0 failed
cargo clippy --all-targets -- -D warnings                      # clean
cargo fmt --check                                              # clean
```

- No test in either file resolves a real config directory, reads an env var, or
  opens a socket to anything but a `mockito` base. `default_path` is the only
  production wrapper and nothing under `#[cfg(test)]` calls it; every filesystem
  test uses a `TempDir` and passes the path in.
- `Cargo.toml` unchanged. Zero new crates.
- `setup.rs`, `token.rs`, `keychain.rs` and `http.rs` were not touched —
  plans 3-02 and 3-03 own three of them and 3-07 owns the fourth.
- `setup.rs`'s existing public-repository test still refuses, because
  `SyncConfig::default()` includes the `credentials` category; the carve-out
  changes behaviour only when the user turns that category off.

## Not done here, deliberately

- **Nothing prints.** `check_drift` returns warnings and the record to persist;
  wiring it into `sync setup` — reading the record, rendering the incident,
  writing the refreshed pairing after success — is plan 3-07's, which owns
  `setup.rs` and `cli.rs`.
- **The D-03 warning**, per above.
