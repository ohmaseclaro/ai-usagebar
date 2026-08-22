# Phase 3 — Security Audit

**Verdict:** `OPEN_THREATS` → remediation in progress.
43/44 declared threats closed; **1 blocking** (T-3-16), plus 10 new findings — one of which
composes with a Phase-3-latent gap into a live credentials-to-public-repo path in Phase 4.

**ASVS 2**, L3 depth on the token path and the gate. `block_on: high`.

Audited from the threat model and the code. **The test suite was not treated as proof** —
F-1, F-2, F-3, F-5 and F-8 all pass every test in the phase.

---

## F-1 — BLOCKER — a remote 401 destroys a stored token that was never used

`setup.rs:362-371` (`clear_if_dead`), `cli.rs:204-208`, `token.rs:286-290`.

Two wrong predicates that compound.

**(a) The trigger is an `AppError` arm, not a 401.** `matches!(err, AppError::Credentials(_))`
also catches `mod.rs:191-194`, where `HeaderValue::from_str` fails on a token carrying an
illegal header byte — so a malformed token silently deletes the Keychain item while printing a
message about header validity that says nothing about a deletion.

**(b) The action ignores which store supplied the token.** `token::clear` deletes the Keychain
item **and** `~/.config/ai-usagebar/sync-token`, unconditionally. `source: TokenSource` is bound
and never consulted.

No attacker required, and it is the *documented* configuration:

1. macOS user runs `sync setup`; token lands in the Keychain.
2. Later a shell exports `AI_USAGEBAR_SYNC_TOKEN` — CI, an `.envrc`, or a 90-day PAT that
   expired. The project's own docs recommend both the env override and a 90-day expiry.
3. `sync status` — the command run *because* something is wrong — resolves source `Env`, gets
   401, and calls the production `token::clear`.
4. The Keychain token, never used and still valid, is gone. The env var still holds the dead
   one, so the next run 401s identically, and the 401 text sends the user to issue a *new* PAT.
   They never suspect what was destroyed.

`gh auth token` has the same shape. `a_401_clears_the_stored_token_and_nothing_else_does`
passes because it varies only the status, never the source.

**Remediation:** classify on `GithubError::Unauthorized`, not `AppError::Credentials(_)`; give
`clear_if_dead` the `TokenSource` and clear only the store that produced the value (`Env` and
`GhCli` clear nothing and say so); split `token::clear` so a `File` source cannot reach the
Keychain; make `actionable`'s "will be cleared" sentence conditional on the source.

## F-2 — HIGH (composed with F-3) — the gate is decided against a category set the flow then replaces

`setup.rs:197` vs `setup.rs:271-279`.

`credentials_in_bundle` is computed **before** the category prompt and is the only input to
`assert_pushable` that step 3 can change — and it is the one deciding D-04's public-repo
carve-out. Nothing re-gates afterwards.

Reachable with default answers: a public repo + `categories = ["config"]` takes the carve-out
and mints a clearance; the user then adds `credentials` at the prompt; the dry run enumerates
the credential files; setup stores the token, writes the pairing record at `private: false`,
and prints "This machine is paired and ready to push."

Every signal says a public repository with credentials in scope is cleared. The gate that would
have refused was answered a question it was no longer being asked. The pairing record anchored
at `private: false` also means this repo can never raise SAFE-02's private→public incident.

Bounded today only by Phase 4 re-running the gate against the *then*-current config.

**Remediation:** prompt for categories **before** the gate, so there is one evaluation and no
window. (Recomputing and re-asserting after step 3 also works, but ordering is cleaner.)

## F-3 — HIGH in Phase 4 — `assert_fresh` has zero production callers

`gate.rs:159-181`, `setup.rs:165`.

`assert_fresh` and `MAX_CLEARANCE_AGE` appear only inside `gate.rs`'s own tests.
`SetupOutcome.clearance` is minted, moved into the struct, and dropped — never read.

**This is the same class as the phase's one shipped defect** (`http::actionable` with no call
site) and it survived for the same reason: it has tests, so it reads as wired.

T-3-21's mitigation has three parts. Two hold: no cached result, and `PushClearance` is neither
`Clone` nor publicly constructible. The third — "cannot be satisfied by a stale check" — does
not follow. Non-`Clone` prevents duplication; a private field prevents forgery. **Neither
prevents holding.** A clearance minted at `sync setup` can be moved across any interval into a
`fn push(clearance: PushClearance)` that never checks freshness.

**F-2 ∧ F-3 is a complete, untyped path from a public repository to uploaded credentials.**
Neither half is visible alone; each looks like a small omission.

**Remediation:** make forgetting impossible rather than discouraged — `PushClearance::spend(self,
now) -> Result<Pushing>` consuming the capability and doing the arithmetic, with every write
verb requiring a `Pushing`. Drop `pub clearance` from `SetupOutcome`.

## F-5 — medium — a deleted pairing record re-pairs silently

`pairing.rs:168-177`, `setup.rs:227-232`.

Deleting the record makes `check_drift` return `first_contact: true` and perform **no**
`owner_id`/`repo_id` comparison — T-3-20 fully defeated, and the numeric-id check that exists
precisely for a released-and-re-registered login never runs. Worse: there is **no positive
first-contact line anywhere**, so a silently reset pairing is visually identical to a first-ever
setup.

**Remediation:** narrate first contact explicitly, naming the repo and owner ids and saying that
if this machine was already paired, the record's removal is an incident.

## F-6 · F-8 · F-9 · F-10 — low

- **F-6** the `gh` watchdog bounds the *process*, not the `read_to_string`; a credential helper
  inheriting the stdout pipe hangs the command indefinitely.
- **F-8** the passphrase-input guard's file list was not extended to
  `src/sync/github/setup.rs`, which now owns a passphrase surface. Correct today;
  regression-prevention gap.
- **F-9** "`Zeroizing<String>` end to end" is untrue at three chain boundaries. The finding is
  the *claim*, not the exposure — the recurring doc-asserts-absent-behaviour class.
- **F-10** the keyfile is written *before* the confirmation that can abort, and setup then
  refuses to re-run while it exists — stranding a user who declined, behind a passphrase they
  were shown once.

## F-4 · F-7 — residual / informational

- **F-4** a hostile body's 200-char excerpt sits undelimited at the front of the tool's own
  advice. Requires controlling TLS for `api.github.com`. Fix is one line: quote it.
- **F-7** the structural claim holds *today* — `Client` exposes only `get_json`, `Endpoints` is
  constructed at exactly one production site with no config/env/flag path, and the redirect
  vector is closed twice. **Phase 4 removes the braces**: `write.rs` adds body verbs and the
  guard scans only `mod.rs`. Extend it to pin body verbs to `write.rs` and to assert
  `reqwest::Client` is constructed nowhere else under `src/sync/`.

## Controls confirmed sound

Redirect policy compares against the chain's *original* URL (no hop-by-hop walk), belt-and-braces
with reqwest's own `Authorization` stripping. Token file is 0600 from `open(2)` via tempfile's
default — verified in tempfile 3.27's source, not assumed — with `persist` a rename and an
explicit redundant chmod; nothing lands in `/tmp`. The `security(1)`/Security.framework split
delegates to one implementation rather than re-deriving it, and fails closed on unset `$USER`.
`gh` gets nothing on argv, `stdin(null)`, and an environment stripped of all 12 project
credential vars — verified complete for this codebase. `RepoRef::parse` is a strict allow-list.
Body bounding checks `Content-Length` *and* accumulates per chunk. Gate refusal covers all six
conditions, and `#[serde(default)] private: bool` **fails closed** — deliberate, not luck.
`actionable` is wired at the only place it could be.

## Carry-forward — Phase 4 must not ship without these

1. **F-3** — `PushClearance` becomes a consuming capability, before any write verb exists.
2. **F-2 ∧ F-3** — the composed path; F-2 is fixed here, F-3 before Phase 4's writes.
3. **F-7** — extend the REPO-03 guard when `write.rs` lands.

**threats_open:** 1
