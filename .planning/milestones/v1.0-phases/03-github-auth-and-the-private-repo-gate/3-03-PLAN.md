---
phase: 03-github-auth-and-the-private-repo-gate
plan: 03
type: execute
wave: 2
depends_on: [3-01]
files_modified:
  - src/sync/github/http.rs
autonomous: true
requirements: [REPO-05]
must_haves:
  truths:
    - "401, 403 carrying rate-limit headers, 403 without them, 404, 429, and a connection reset are six distinct outcomes with six distinct messages (D-06)."
    - "Every one of them names what the user should do next, and none is reported as success."
    - "A retry delay is computed from headers, not from a hard-coded guess, and never from the wall clock inside the function."
    - "A 401 says to clear and re-issue the token; a 403 that is a missing permission does not."
    - "No message, at any verbosity, contains the token or any prefix of it."
  artifacts:
    - src/sync/github/http.rs with the complete classifier, the backoff schedule, and the actionable message table
  key_links:
    - "`classify` is the only place a status code becomes a `GithubError`; `gate` and `setup` both route through it"
    - "`retry_delay` takes `now: DateTime<Utc>` so the reset-header arithmetic is testable without sleeping"
---

<objective>
Fill the failure taxonomy plan 3-01 froze: turn a status code plus headers into the right
`GithubError`, compute a retry delay from what GitHub actually told us, and give every arm a
message that names the fix.

This is where D-06 becomes real. A 401 and a 403 need opposite responses — one means the
token is dead and should be cleared, the other means the token is alive but under-permissioned
or rate-limited and clearing it would destroy a working credential. Reporting both as
"authentication failed" is the bug this plan exists to prevent.

Implements **D-06** (every failure path prints what to do) and satisfies **REPO-05**.

Purpose: a failure that does not name its fix costs the user a support round trip.
Output: a complete `src/sync/github/http.rs`.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/03-github-auth-and-the-private-repo-gate/3-CONTEXT.md
@.planning/phases/03-github-auth-and-the-private-repo-gate/3-01-SUMMARY.md
@.planning/research/github-transport.md
@CLAUDE.md
@src/error.rs
@src/vendor.rs
</context>

<tasks>

<task type="auto" tdd="true">
  <name>Task 1: Classification — six outcomes, six messages</name>
  <files>src/sync/github/http.rs</files>
  <behavior>
    - 401 with any body yields `Unauthorized`, and its actionable text says to clear the stored token and issue a new one.
    - 403 carrying `retry-after` yields `RateLimited` with that many seconds.
    - 403 carrying `x-ratelimit-remaining: 0` and an `x-ratelimit-reset` epoch yields `RateLimited` with the delay from `now` to that instant.
    - 403 carrying neither yields `Forbidden`, and its text points at the token's permissions, not at re-issuing it.
    - 429 is treated identically to a rate-limited 403 — same variant, same delay source.
    - 404 yields `NotFound`, and its text states both possibilities: the repository does not exist, or the token is not scoped to it.
    - An `x-ratelimit-reset` already in the past clamps to the 60-second floor rather than yielding a zero or negative delay.
    - A body that is not JSON, is empty, or is a megabyte of unrelated text still produces a bounded message.
  </behavior>
  <action>
Complete `classify`, `from_transport`, and `actionable` in `src/sync/github/http.rs` behind the
signatures plan 3-01 froze. Do **not** add, rename, or remove a `GithubError` variant: plans
3-02 and 3-04 are matching on them in parallel worktrees, and Phase 4 will match on `Conflict`
without adding one of its own.

**`classify(status, headers, body, now)`**, following `github-transport.md` §4.3, which treats
403 and 429 as the same backoff signal rather than as different problems:

- 401 → `Unauthorized`. This is the one status where the stored token is provably useless.
- 403 or 429 → decide between `RateLimited` and `Forbidden` by whether the response carries a
  limit signal. A `retry-after` header, or `x-ratelimit-remaining` at zero, means rate limited;
  neither means a missing permission. This discrimination is the plan's core: it is what stops
  a rate limit from being reported as "your token lacks permission" and a missing permission
  from being reported as "wait and retry", which would loop forever.
- 404 → `NotFound`.
- 409 → `Conflict`, unreached in this phase and present so Phase 4's compare-and-swap pointer
  write adds no variant.
- anything else non-2xx → `Unexpected`, carrying the numeric status.

Extract a short, bounded excerpt of the response body into each message — take the JSON
`message` field when the body parses as a GitHub error object, otherwise the first couple of
hundred characters — the same shape `anthropic::oauth::parse_error_body` already uses. The
body is attacker-controlled when the remote is hostile, so it is truncated, never formatted
as a format string, and never trusted to be UTF-8.

**`pub fn retry_delay(headers: &HeaderMap, attempt: u32, now: DateTime<Utc>) -> Duration`** —
the three-step ladder, in order: `retry-after` in seconds if present and parseable; else, when
`x-ratelimit-remaining` is zero, the interval from `now` to the `x-ratelimit-reset` UTC epoch
second; else exponential backoff over `attempt` with jitter. Floor every result at 60 seconds
and cap it at a stated ceiling so a hostile or garbled header cannot park the process for a
day. `now` is a parameter — the whole point is that the reset arithmetic is testable without
sleeping, mirroring `antigravity::parse_cache_at`. Derive the jitter from the existing
`getrandom` dependency; do not add a random-number crate.

**`from_transport(e)`** → `Transport`, with text that distinguishes the recognisable cases
reqwest exposes — timeout, connect failure, TLS — and otherwise says the request never
reached GitHub. Retry guidance, per D-06.

**`actionable(err)`** returns the user-facing line for each variant. Concretely: `Unauthorized`
says the token was rejected and names re-issuing a fine-grained token with
`Contents: read/write` and `Metadata: read`, plus that the stored one will be cleared;
`Forbidden` says the token is valid but lacks a permission on this repository and names the
same two permissions; `RateLimited` names the wait in whole seconds and when it clears;
`NotFound` states both possibilities and carries no offer to create anything; `Transport`
gives retry guidance; `Unexpected` reports the numeric status and the excerpt. Every line
ends in something the user can do. None of them interpolates a token, a header value that
could carry one, or a full response body.

Add `pub fn is_retryable(err: &GithubError) -> bool`, true only for `RateLimited` and
`Transport` — Phase 4's upload loop needs this and it belongs with the taxonomy, not with the
uploader.

Tests are table-driven over constructed `HeaderMap`s and a fixed `now`, plus one `mockito`
test per status through `Client::get_json` proving the real path produces the same
classification, plus one test pointing an `Endpoints` at a closed local port to exercise the
transport arm. Assert on the *distinctness* of the six messages, not just their presence: a
test that collects all six and asserts the set has six elements catches the copy-paste that a
per-message assertion misses.
  </action>
  <verify>
    <automated>cargo test --lib sync::github::http</automated>
  </verify>
  <done>Six outcomes, six distinct messages, each naming a next step. `retry_delay` returns the header-derived value for `retry-after`, the reset-derived value when remaining is zero, and a jittered exponential otherwise — never below 60 seconds, never above the ceiling, never negative for a reset in the past. `cargo test --lib sync::github::http` is green and no test sleeps.</done>
</task>

<task type="auto">
  <name>Task 2: Prove the exit code and the silence</name>
  <files>src/sync/github/http.rs</files>
  <behavior>
    - Converting each `GithubError` into `AppError` lands on an arm the CLI renders as a failure; none maps onto a success-shaped value.
    - Rendering every variant's `actionable` text with a token in scope produces no output containing that token.
  </behavior>
  <action>
Add the two guard tests success criterion 5 of the phase turns on.

First, exit-code correctness: build one of each `GithubError`, convert through the
`From<GithubError> for AppError` that plan 3-01 defined, and assert each lands on a failure
arm. The phase's requirement is that none of the four named failures is reported as success,
and the conversion is where that could silently go wrong.

Second, silence: construct a token-shaped fixture string, put it through a `Client` and a
classified failure for each variant, and assert the rendered `actionable` text contains
neither the fixture nor its first eight characters. A prefix is still a secret — the codebase's
rule is that presence and source are reportable and the value never is. Assert on the prefix
explicitly, because "we only log the first few characters" is the exact shortcut this test
exists to prevent.

Keep both tests in this file. They assert properties of this module, and putting them in an
integration test would put them in a file another plan owns.
  </action>
  <verify>
    <automated>cargo test --lib sync::github::http</automated>
  </verify>
  <done>Both guard tests exist and pass. Every `GithubError` maps to a failure `AppError`; no rendered message contains the token fixture or any eight-character prefix of it.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|---|---|
| GitHub response headers → process | Attacker-controlled when the remote is hostile or the host is spoofed; they drive a sleep duration |
| GitHub response body → user-facing message | Attacker-controlled bytes rendered to a terminal |
| process → terminal | Every error line is a potential exfiltration path for the token |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|---|---|---|---|---|---|
| T-3-14 | Denial of service | `retry_delay` | high | mitigate | A hostile `retry-after` or a far-future `x-ratelimit-reset` is clamped to a stated ceiling, so a header cannot park the process indefinitely; a past reset clamps up to the 60-second floor rather than yielding a negative delay |
| T-3-15 | Information disclosure | `actionable` output | critical | mitigate | No variant interpolates the token, a header value, or a whole body; a test asserts no message contains the token fixture or its eight-character prefix |
| T-3-16 | Spoofing | 401 vs 403 confusion | high | mitigate | 401 alone clears the stored token; a 403 that is a missing permission never does, so a rate limit cannot destroy a working credential |
| T-3-17 | Tampering | response body rendered to a terminal | medium | mitigate | The excerpt is length-bounded, lossy-UTF-8 decoded, and never used as a format string |
| T-3-18 | Repudiation | a failure reported as success | critical | mitigate | Every `GithubError` converts to a failure `AppError` arm, asserted by a test over all seven variants |
</threat_model>

<verification>
- `cargo test --lib sync::github::http` is green.
- No test in the file calls `tokio::time::sleep`, `std::thread::sleep`, or `Utc::now`.
- `GithubError` still carries exactly the seven variants plan 3-01 froze.
</verification>

<success_criteria>
401, a 403 carrying `retry-after`, a 403 without limit headers, 429, 404, and a connection
reset each produce a distinct, actionable message and a failure result. The retry delay comes
from GitHub's own headers where it offers them, is bounded at both ends where it does not, and
is computed against an injected clock.
</success_criteria>

<output>
Create `.planning/phases/03-github-auth-and-the-private-repo-gate/3-03-SUMMARY.md` when done.
Record the final `retry_delay` ceiling and the exact text of each `actionable` arm — plan 3-07
prints them and Phase 4's upload loop reuses `is_retryable`.
</output>
