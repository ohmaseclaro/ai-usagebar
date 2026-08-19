---
phase: 03-github-auth-and-the-private-repo-gate
plan: 03
subsystem: transport
tags: [github, failure-taxonomy, retry, backoff, d-06, hermetic-tests, no-token-in-output]

requires:
  - phase: 03-github-auth-and-the-private-repo-gate
    plan: 01
    provides: "`GithubError`'s seven frozen variants, `classify`/`from_transport`/`actionable` signatures, `Client::get_json`"
  - codebase
    provides: "`display::sanitize_untrusted_field`, `AppError`, `getrandom` 0.4"
provides:
  - "`classify(status, &HeaderMap, &[u8], now)` — the complete 401 / 403 / 404 / 409 / 429 / catch-all taxonomy"
  - "`retry_delay(&HeaderMap, attempt, now) -> Duration` — the three-rung ladder, clamped to [`MIN_RETRY_DELAY`, `MAX_RETRY_DELAY`]"
  - "`MIN_RETRY_DELAY = 60s`, `MAX_RETRY_DELAY = 3600s` (both `pub`)"
  - "`actionable(&GithubError) -> String` — seven arms, each naming a next step"
  - "`is_retryable(&GithubError) -> bool` — true only for `RateLimited` and `Transport`"
  - "`from_transport(&reqwest::Error)` — timeout / connect / never-sent / other"
affects: [3-07, phase-4-push]

tech-stack:
  added: []
  patterns:
    - "A distinctness claim is asserted as a *set*: collect all six messages into a `BTreeSet` and assert `len() == 6`. A per-message assertion passes happily on six copies of one paragraph."
    - "The untrusted-body boundary is crossed exactly once, in `message_of`, so both the `Display` impl and every `actionable` arm inherit the sanitize + truncate for free."
    - "A header that is *present but garbled* still counts as a limit signal. Parse failure changes the delay, never the classification — otherwise a broken remote could turn a rate limit into 'your token lacks permission'."

key-files:
  created: []
  modified:
    - src/sync/github/http.rs

key-decisions:
  - "**429 is always `RateLimited`, with or without headers.** The plan's classify prose reads 'decide between RateLimited and Forbidden by whether the response carries a limit signal' for 403 *or* 429, but its own behaviour line says 429 is treated identically to a rate-limited 403, and the success criterion lists 429 as one of six *distinct* outcomes. A bare 429 rendered as 'your token lacks a permission' is exactly the misclassification this plan exists to prevent, so the signal test gates only 403. A bare 429 falls to the backoff rung and gets the 60s floor."
  - "**422 stays in `Unexpected`, carrying 422.** Mapping it onto `Conflict` here would relabel every malformed request as a conflict. Only Phase 4's Contents `PUT` call site knows it omitted a `sha`; it can match `Unexpected { status: 422, .. }` and route it onto the conflict path without a new variant."
  - "**`MAX_RETRY_DELAY = 1 hour`,** because the primary rate limit is a one-hour window — an hour is the longest wait that can still be true. A `retry-after: 999999999` is eleven days of parked process otherwise (T-3-14)."
  - "**No TLS-specific transport arm.** reqwest reports a failed TLS handshake as a connect error and exposes no predicate to separate them; the only alternative was sniffing the error chain for the substring 'certificate', which is brittle and untestable hermetically. The connect arm names all three causes honestly: 'no connection could be opened — DNS, TLS, or a blocked port'."
  - "**`actionable` takes no clock,** because its signature is frozen. `RateLimited` therefore names the wait relatively ('Wait 90 seconds', 'Wait about 10 minutes') rather than as an absolute instant. Whole units only — a wait rendered as `312.4074s` reads like a bug."
  - "**The token-silence test does not cover a body that echoes the token back.** `classify` has no token in scope and the frozen signature cannot give it one, so it could not redact such an echo; asserting otherwise would be asserting a falsehood. GitHub does not echo credentials, and a remote that does already has the token — printing it to the user's own terminal leaks it no further."

patterns-established:
  - "Time-dependent arithmetic takes `now`; jitter comes from `getrandom::fill`, which is the only nondeterminism in the module and is confined to the third rung — so every header-derived delay is asserted with `assert_eq!`, not a range."

requirements-completed: [REPO-05]

coverage:
  - id: REPO-05
    description: "401, 403-with-limit-headers, 403-without, 429, 404, and a connection reset are six distinct outcomes with six distinct actionable messages, none reported as success"
    verification:
      - kind: unit
        ref: "src/sync/github/http.rs#the_six_outcomes_produce_six_distinct_messages_that_each_name_a_fix"
        status: pass
      - kind: unit
        ref: "src/sync/github/http.rs#the_wire_produces_the_same_six_classifications"
        status: pass
      - kind: unit
        ref: "src/sync/github/http.rs#a_dead_port_becomes_a_transport_failure_with_retry_guidance"
        status: pass
    human_judgment: false
  - id: D-06
    description: "Every failure path names the fix; the 401 and 403 arms say opposite things about the stored token"
    verification:
      - kind: unit
        ref: "src/sync/github/http.rs#a_403_is_a_wait_only_when_the_response_says_so"
        status: pass
      - kind: unit
        ref: "src/sync/github/http.rs#a_429_is_always_a_wait_and_uses_the_same_delay_source"
        status: pass
    human_judgment: false
  - id: T-3-14
    description: "A hostile or garbled header cannot park the process: every rung is clamped to [60s, 1h], and a reset in the past floors rather than going negative"
    verification:
      - kind: unit
        ref: "src/sync/github/http.rs#the_delay_ladder_runs_in_order_and_is_clamped_at_both_ends"
        status: pass
      - kind: unit
        ref: "src/sync/github/http.rs#backoff_grows_with_the_attempt_and_stays_inside_the_ceiling"
        status: pass
    human_judgment: false
  - id: T-3-15
    description: "No rendering of any variant — `actionable`, `Display`, `Debug`, the converted `AppError`, or the `Client` holding the token — contains the token or its first eight characters"
    verification:
      - kind: unit
        ref: "src/sync/github/http.rs#no_rendering_of_any_failure_contains_the_token_or_a_prefix_of_it"
        status: pass
    human_judgment: false
  - id: T-3-17
    description: "The response body is sanitized, length-bounded, lossy-UTF-8 decoded, and interpolated as a value"
    verification:
      - kind: unit
        ref: "src/sync/github/http.rs#a_body_is_bounded_sanitized_and_never_a_format_string"
        status: pass
    human_judgment: false
  - id: T-3-18
    description: "All seven variants convert into a failure `AppError`; none lands on a success-shaped value"
    verification:
      - kind: unit
        ref: "src/sync/github/http.rs#every_variant_converts_into_a_failure_app_error"
        status: pass
    human_judgment: false

duration: 40min
completed: 2026-08-19
status: complete
---

# Phase 3 / Plan 03: The Failure Taxonomy

**A 401 and a 403 now say opposite things about the stored token, and neither of
them guesses.** That is the whole plan: 401 is the one status where the token is
provably dead, so its message says the token will be cleared; a 403 that is a
missing grant says *keep the token* and fix the permission, because clearing it
would destroy a working credential over a rate limit.

## Task Commits

1. **`8b224ff`** — the classifier, the backoff ladder, the message table, and the
   two guard tests (tasks 1 and 2; one file, one diff)

## The classifier

| Status | Variant | Discriminator |
|---|---|---|
| 401 | `Unauthorized` | — |
| 403 **with** `retry-after`, or `x-ratelimit-remaining: 0` | `RateLimited` | `has_limit_signal(headers)` |
| 403 **without** | `Forbidden` | — |
| 404 | `NotFound` | — |
| 409 | `Conflict` | unreached in Phase 3 |
| 429 | `RateLimited` | **always** — see the decision note |
| anything else non-2xx | `Unexpected { status }` | includes 422 |
| no status at all | `Transport` | `from_transport` |

`has_limit_signal` treats a *present but garbled* `retry-after` as a signal. A
parse failure changes the delay (it falls through to backoff), never the
classification — otherwise a hostile remote could relabel a rate limit as a
missing permission and send the user off to re-issue a working token.

## `retry_delay` — the ladder and the ceiling

```rust
pub const MIN_RETRY_DELAY: Duration = Duration::from_secs(60);
pub const MAX_RETRY_DELAY: Duration = Duration::from_secs(3600);   // one hour

pub fn retry_delay(headers: &HeaderMap, attempt: u32, now: DateTime<Utc>) -> Duration
```

1. `retry-after`, parsed as whole seconds.
2. else, when `x-ratelimit-remaining == 0`, `x-ratelimit-reset - now.timestamp()`,
   saturating at zero.
3. else `60 << attempt.min(6)` seconds — 60, 120, 240 … 3840 — plus up to 25%
   jitter from `getrandom::fill`.

`.clamp(MIN_RETRY_DELAY, MAX_RETRY_DELAY)` applies to **whichever rung produced
the value**. So a reset already in the past yields 60s rather than 0, a
`retry-after: 999999999` yields one hour rather than eleven days, and rung 3
plateaus at one hour from attempt 6 on.

`classify` calls `retry_delay(headers, 0, now)` — a freshly classified rate limit
is attempt 0. **Phase 4's upload loop passes its own attempt counter.**

Jitter is the only nondeterminism in the module and it lives on rung 3 alone, so
every header-derived delay is asserted with `assert_eq!` rather than a range.

## `is_retryable` — Phase 4's contract

```rust
pub fn is_retryable(err: &GithubError) -> bool   // RateLimited | Transport, nothing else
```

True only where re-running the *same* request unchanged can succeed. Everything
else is a decision the user has to make: a dead token, a missing grant, a wrong
repository name, a diverged remote, an unexplained status. A loop that retried
`Forbidden` would spin forever.

## The exact `actionable` text — plan 3-07 prints these

`{message}` is GitHub's own `"message"` field when the body carries one, else the
body as lossy text; either way sanitized through
`display::sanitize_untrusted_field` and truncated to 200 characters with an
ellipsis. `PERMISSIONS` = `Contents: read/write and Metadata: read`.
`NEW_TOKEN_URL` = `https://github.com/settings/personal-access-tokens`.

**`Unauthorized`**
> GitHub rejected the sync token (401): {message}. The stored token is dead and
> will be cleared. Issue a replacement at
> https://github.com/settings/personal-access-tokens/new — a fine-grained PAT
> scoped to the single sync repository, with Contents: read/write and Metadata:
> read — then supply it in AI_USAGEBAR_SYNC_TOKEN or
> ~/.config/ai-usagebar/sync-token and re-run \`ai-usagebar sync setup\`.

**`Forbidden`** — says nothing about clearing or re-issuing (T-3-16)
> GitHub refused this request (403): {message}. The token is valid — keep it —
> but it lacks a permission on this repository. Edit it at
> https://github.com/settings/personal-access-tokens, confirm the repository is
> in its selected list, and grant Contents: read/write and Metadata: read. Then
> re-run the same command.

**`RateLimited`** — `humanize`: `"{n} seconds"` under two minutes, else
`"about {n} minutes"` (ceiling division)
> GitHub rate-limited this token: {message}. Nothing is wrong with the token or
> the repository. Wait {90 seconds | about 10 minutes} and re-run the same
> command; the limit clears on its own.

**`NotFound`** — both causes, no offer to create anything
> GitHub returned 404 for this repository: {message}. That is two different
> problems wearing one status, and GitHub will not say which: either the
> repository does not exist under that name, or the token is not scoped to it —
> GitHub answers 404 for a private repository a token cannot see. Check \`repo\`
> under [sync] in config.toml, then check the token's repository access at
> https://github.com/settings/personal-access-tokens. ai-usagebar never creates a
> repository.

**`Conflict`** — unreached in Phase 3
> GitHub reported a conflicting remote state (409): {message}. Another machine
> wrote to this repository after this run read it. Re-run the same command — it
> re-reads the remote state and re-plans. Do not force it: the other machine's
> state may reference data this run is about to remove.

**`Transport`** — `{message}` here is `from_transport`'s own text: one of "GitHub
did not answer before the request timed out", "no connection could be opened —
DNS, TLS, or a blocked port", "the request was never sent", or "the request did
not complete", each followed by a truncated reqwest detail in parentheses
> Could not reach GitHub: {message}. Nothing was uploaded. Check the network, any
> proxy or VPN, and https://www.githubstatus.com, then re-run the same command —
> every sync command is safe to re-run.

**`Unexpected`**
> GitHub returned an unexpected HTTP {status}: {message}. Re-run the same command
> once; if HTTP {status} repeats, check https://www.githubstatus.com, and if
> GitHub is healthy report this status and message at
> https://github.com/akitaonrails/ai-usagebar/issues.

## Note for Phase 4 — the 422 that is really a 409

A Contents `PUT` that omits `sha` against an existing file answers **422, not
409**. 422 is not special-cased here: it is also GitHub's generic validation
failure, so mapping it onto `Conflict` in the classifier would relabel every
malformed request as a conflict. It arrives as `Unexpected { status: 422, .. }`,
and the one call site that knows it omitted a `sha` can match on that and route
it onto the conflict path — no new variant, no change to `classify`.

## Verification

```
cargo test --lib sync::github::http     # 12 passed
cargo test --lib -- sync::             # 228 passed, 0 failed
cargo clippy --all-targets -- -D warnings   # clean
cargo fmt --check                           # clean
```

- `grep -n 'Utc::now\|thread::sleep\|time::sleep' src/sync/github/http.rs` — one
  hit, in the doc comment explaining why `now` is a parameter. No test sleeps.
- Seven variants, unchanged. No other file touched: `3-02` (`token.rs`,
  `keychain.rs`) and `3-04` (`gate.rs`, `pairing.rs`) are untouched, and
  `gate.rs`'s existing `classify` call still compiles and passes.
- `Cargo.toml` unchanged. Zero new crates; the jitter comes from the `getrandom`
  already used by `sync::crypto` and `sync::passphrase`.
- Every test drives constructed `HeaderMap`s, a pinned `now`
  (`1_700_000_000`), a `mockito` base, or a closed loopback port. No real
  network, no real token, no `$HOME`.

## Not done here, deliberately

- **Nothing calls `retry_delay` yet.** Phase 3 makes exactly one request and does
  not retry it; the ladder exists for Phase 4's upload loop, which is also the
  only consumer of `is_retryable`.
- **`Conflict` is still unreached** — Phase 4's compare-and-swap pointer write is
  its first producer.
- **The 401 arm says the token "will be cleared"; it does not clear it.**
  `token::clear` is plan 3-02's, and the call site that invokes it on a 401 is
  3-07's. If either slips, that sentence becomes a lie — it is the one line in
  this file that depends on another plan landing.
