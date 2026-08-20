---
phase: 6
plan: 11
subsystem: sync
tags: [onboarding, github, keystore, status, setup]
requires:
  - 6-05 (the empty-repository 422 message)
  - 6-10 (the keystore module the Claude Code login travels in)
provides:
  - "sync setup: an empty repository is offered its first commit"
  - "sync status: the credentials count includes the machine-bound login"
affects:
  - src/sync/github/setup.rs
  - src/sync/github/write.rs
  - src/sync/report.rs
  - src/sync/keystore.rs
  - src/anthropic/keychain.rs
  - src/sync/cli.rs
tech-stack:
  added: []
  patterns:
    - "existence probe without a value read, so a Keychain ACL prompt is unreachable"
    - "fail-safe remote probe returning bool, not Result"
key-files:
  created: []
  modified:
    - src/anthropic/keychain.rs
    - src/sync/keystore.rs
    - src/sync/report.rs
    - src/sync/cli.rs
    - src/sync/github/setup.rs
    - src/sync/github/write.rs
decisions:
  - "The empty-repository offer sits inside step 2, after the gate — the private-repo refusal stays the first thing that touches the repository."
  - "A decline continues setup rather than aborting it: the pairing, password and scope are all valid without the commit."
  - "sync status counts stores by existence, never by value — a value read is what raises the macOS Keychain ACL prompt on a menu open."
metrics:
  duration: ~1h
  completed: 2026-08-20
status: complete
---

# Phase 6 Plan 11: Two onboarding fixes found by a real first run — Summary

`sync setup` now offers an empty repository its first commit instead of
printing a command to paste, and `sync status` counts the Claude Code login it
had been silently omitting.

## Signature changes

Stated first, as asked.

| Symbol | Change |
|---|---|
| `github::setup::SetupOutcome` | **new field** `pub initialised: bool` — every construction site outside `setup::run` breaks |
| `sync::report::WARNINGS` | `[&str; 1]` → **`[&str; 2]`** — an array length in the type, so any consumer matching on it breaks |
| `Client::repo_has_no_commits(&self, &RepoRef) -> bool` | new read verb, no `Pushing` |
| `Client::init_first_commit(&self, &RepoRef, &Pushing, DateTime<Utc>) -> Result<()>` | new write verb |
| `write::first_commit_command(&RepoRef) -> String` | new, `pub(crate)` |
| `keystore::Stores::has(&self, Store) -> Result<bool>` | new |
| `keystore::Fixture::set_unreadable(&mut self, bool)` | new test seam |
| `anthropic::keychain::has_raw() -> Result<bool>` | new, macOS-only |
| `report::WARN_KEYSTORE_UNAVAILABLE` | new constant |

Nothing was removed and no existing signature changed shape. `Cargo.toml` and
`Cargo.lock` are byte-identical — no crate added.

## 1. `sync setup` gives an empty repository its first commit

**The condition.** A repository with no commits cannot be tagged, so GitHub
answers the release `POST` with a bare 422 `"Validation Failed"` — and an empty
repository is exactly what `gh repo create --private` leaves behind, so this is
the ordinary first-run state. Plan 6-05 named it and printed a `gh api …` line;
the user still had to leave setup, paste a command, and come back.

**The shape.** Three pieces, all in the file that is reviewed *as* the outbound
path:

- `Client::repo_has_no_commits` — `GET …/commits?per_page=1`. GitHub's own
  409 `Git Repository is empty.` is the only answer that means empty. It
  returns a `bool` rather than a `Result`, and that is deliberate: **fail-safe,
  not fail-closed.** Anything else — a 500, a dead socket — reads as "not
  empty", because being wrong that way costs a message the push path already
  prints, while being wrong the other way offers to write a commit into a
  repository that did not need one.
- `Client::init_first_commit` — one `Contents` `PUT` of a `README.md` saying
  the repository holds ciphertext managed by `ai-usagebar sync` and should not
  be edited by hand. **Created, never replaced**: no `sha` is sent, so a file
  that appeared between the probe and the write is a refusal.
- `write::first_commit_command` — one function behind both the push refusal's
  `gh api` line and the setup decline's. They had already drifted once; a
  command a user pastes is exactly the text that must not.

**Where it goes.** Inside step 2, **after** `assert_pushable`. Step 2 is the
private-repo gate and stays the first thing that touches the repository — this
is the first byte the tool would ever put in it, so the refusal has to have run
against it first. `a_public_repository_is_refused_before_the_offer_is_ever_reached`
asserts that the emptiness probe was never even issued for a public repository;
moving the probe above the gate fails it.

**Asked, never assumed.** Through the existing `SetupPrompt::confirm`. A
decline leaves the repository untouched, prints the command, and lets setup
finish — the pairing, the password and the scope are all valid without the
commit, and the eventual push says the same thing in the same words. Aborting
would throw away a whole guided flow over something the user can still fix.

**No repository creation.** The token holds no `Administration: write`, which
is what makes bringing a *public* repository into existence structurally
impossible rather than merely disallowed (REPO-03). A `Contents` write does not
touch that, and REPO-03's standing guard over `src/` is unchanged and still
green. A `[sync] repo` naming a repository that does not exist still gets
`gate::missing_repo_message` and the `gh repo create --private` line, and no
API call.

**The clearance.** `gate::assert_pushable`'s `PushClearance` used to be minted
and dropped here, because setup uploaded nothing. It is now kept as far as the
offer and **spent** on it — this is the byte the private-repo check was run
for, a handful of round trips after it. If the repository is not empty it is
dropped unspent, which uploads nothing. Nothing is carried into a push; a push
still mints and spends its own (F-3).

**What stopped being true, and was corrected rather than left standing.** Setup
no longer "uploads nothing" unconditionally, so `SetupOutcome::initialised`
carries the one write and both closing messages branch on it — the success line
and the size-confirmation decline. Neither may claim "the repository was not
touched" over a README the user approved.

**F-10 is untouched.** Its rule is that nothing *local* persists until the last
thing that can abort has passed, because a keyfile written early survived a
decline and then refused the re-run, stranding the user behind a passphrase
shown once. A README strands nothing: a re-run finds the repository no longer
empty and makes no offer.

## 2. `sync status` counts what a push would carry

**The defect.** `sync status` said `credentials 107 files` while
`sync push --dry-run` planned 108. The extra one is the Claude Code login
captured from the macOS Keychain by plan 6-10's `keystore` module. `status`
walks the filesystem, and a keystore entry is not a file — so the item it
omitted was the single most sensitive one in the bundle.

**The constraint that shaped the fix.** `sync status --json` is the macOS menu
bar's whole read of sync and runs on every menu open, so it must not ask for a
password, dial out, or open a file body. Reading the store's *value* would do
none of those three — but it would run `security find-generic-password -w`,
and `-w` is the flag that asks for the secret and therefore the only reason
macOS consults the item's ACL. On an item the user granted with "Allow" rather
than "Always Allow", that is a Keychain prompt per menu open.

So the probe asks a smaller question:

- `keychain::has_raw` — `find-generic-password` **without `-w`**. Attributes,
  never data: no ACL consultation, no prompt, no credential in this process.
  `read_raw_service` and `has_raw` now share one `find_generic_password`, so
  the item selector and the locked-Keychain message have one copy.
- `Stores::has` — the seam, injected exactly like `read`, and the
  `SyncRoots::at`/`Stores::Machine` hermeticity rule is unchanged. The
  `the_machine_store_is_constructed_in_exactly_one_place` guard still passes.
- `report::count_stores` — guarded by `cfg.includes(Credentials)`, mirroring
  the planner's third pass exactly, and added on the **walked path only**: a
  plan has already counted its own stores, and adding them again would double
  them.

**The third answer.** An unreadable store (a locked Keychain, a denied ACL) is
not "no credential here". It becomes `WARN_KEYSTORE_UNAVAILABLE` — a new entry
in `WARNINGS`, so the machine-readable document may carry it and the
"every string is a label, a path, a timestamp or a known warning" guard still
holds — and the row keeps only what the walk could see. Never a quiet zero.

**A guard rather than a promise:**
`the_status_path_asks_whether_a_store_has_an_entry_and_never_what_it_holds`
scans `report.rs`'s production code for `stores.read(` and fails on it.

## Ceilings, marked in the source

- `add_stores` moves the **count** and not the byte total. A store's size is its
  value's length, and reading a value is the thing this path may not do; the
  shortfall is a few hundred bytes against a total rendered in megabytes. If a
  store ever holds something visible, the size has to travel out of the planner.
- `has_raw` answers `true` for an item that exists but holds an empty value,
  which the planner (which reads values) skips. Establishing that difference
  costs the value, and therefore the prompt.

## Production call sites of every function added

Asked for explicitly. Every one has at least one, except the two test seams,
which are named as such:

| Added | Production call sites |
|---|---|
| `keychain::has_raw` | 1 — `keystore::machine_has` (macOS) |
| `keychain::find_generic_password` | 2 — `read_raw_service`, `has_raw` |
| `keystore::Stores::has` | 1 — `report::count_stores` |
| `keystore::machine_has` | 1 — `Stores::has` (both cfg arms) |
| `keystore::Fixture::fail` | 2 — `Stores::has`, `Stores::read` |
| `keystore::Fixture::set_unreadable` | **0 — test seam**, exactly like the existing `Fixture::set` / `set_safe_key` |
| `report::count_stores` | 1 — `build_status` |
| `report::add_stores` | 1 — `build_status` |
| `report::WARN_KEYSTORE_UNAVAILABLE` | 2 — `build_status`, `WARNINGS` |
| `Client::repo_has_no_commits` | 1 — `setup::run` |
| `Client::init_first_commit` | 1 — `setup::offer_first_commit` |
| `write::first_commit_command` | 2 — `ensure_release`'s 422 message, `setup::offer_first_commit` |
| `setup::offer_first_commit` | 1 — `setup::run` |
| `SetupOutcome::initialised` | 2 — `cli::render_setup`, `setup::run`'s decline message |
| `INIT_PATH` / `INIT_COMMIT_MESSAGE` / `INIT_README` | 1 each — `init_first_commit` |
| `Script::decline_matching` | **0 — test double field** |

## Negative controls

Run **after** committing, per the milestone's standing rule.

| Mutation | Failed |
|---|---|
| `count_stores` returns `Ok(0)` | `status_counts_the_keystore_the_way_a_push_would`, `an_unreadable_store_is_a_warning_rather_than_a_quietly_short_count` |
| `repo_has_no_commits` always `false` | `a_409_on_the_commits_endpoint_is_an_empty_repository`, plus all three setup offer tests |
| emptiness probed **before** `assert_pushable` | `a_public_repository_is_refused_before_the_offer_is_ever_reached` |

## Hermeticity

No test reads a real `$HOME`/`$XDG`, opens a socket, reads the wall clock, or
touches the real login Keychain. The keystore tests go through
`Stores::fixture` (which `SyncRoots::at` yields), the setup tests through
mockito and the scripted `SetupPrompt` double, and every `security(1)` call
stays behind `Stores::Machine`, which no test can construct.

## Verification

| Gate | Result |
|---|---|
| `cargo test` | **1800 passed, 0 failed**, 16 ignored — baseline was 1787/0. The lib suite went 1724 → 1737 (4 report, 4 setup, 5 write); every other binary is unchanged. |
| `cargo clippy --all-targets -- -D warnings` | clean |
| `cargo fmt --check` | clean |
| `make test` | green, including the GNOME, KDE and Omarchy contract suites |
| `Cargo.toml` / `Cargo.lock` | byte-identical |

## Commits

| Commit | What |
|---|---|
| `e58a976` | `fix(6-11)`: sync status counts the login the walk cannot see |
| `5325f03` | `feat(6-11)`: sync setup offers an empty repository its first commit |
| `83f7f6f` | `test(6-11)`: the closing line reports the README it approved |

## Self-Check: PASSED
