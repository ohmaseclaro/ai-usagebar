---
phase: 04-push-packs-atomic-flip-gc-rekey
plan: 03
subsystem: transport
tags: [upload, resume, content-addressing, bounded-concurrency, verification, progress, keyfile, hermetic-tests]

requires:
  - phase: 04-push-packs-atomic-flip-gc-rekey
    plan: 01
    provides: "`write::{list_assets, upload_asset, delete_asset, download_asset}`, `Asset`, `ASSET_STATE_UPLOADED`, `with_retry`, `BuiltPack`, `PushCtx`, the `Progress` trait and `Silent`"
  - phase: 03-github-auth-and-the-private-repo-gate
    plan: 08
    provides: "`gate::Pushing` — taken by reference by every write verb, which is what rules out spawning"
  - phase: 01-encrypted-bundle-core
    provides: "`content_address`, `Keyfile`, `PACK_MAX`"
provides:
  - "`upload::run` — the resume scan, four bodies at a time, and D3's verifying download"
  - "`upload::ensure_keyfile` — the keyfile asset a first push must publish (4-01 deviation 9), frozen for 4-06"
  - "`progress::{render, Terminal, Plain, reporter}` — two implementations behind 4-01's trait"
  - "CAL-5 in `tests/live.rs` — the torn-upload asset `state` and whether `digest` is populated"
affects: [4-04, 4-05, 4-06, 4-07, phase-5-restore]

tech-stack:
  added: []
  patterns:
    - "Bounded concurrency without `'static`: a hand-polled window of boxed futures, refilled as each completes. `JoinSet` is unavailable to any code that borrows a capability or a `!Sync` context."
    - "A concurrency assertion pins the literal, never the constant it is bounding — asserted against the constant the test passes at every cap."
    - "A mock's counting predicate does its own matching: mockito evaluates every candidate's `match_request`, so a side effect beside a separate matcher counts requests the mock never answered."

key-files:
  created: []
  modified:
    - src/sync/push/upload.rs
    - src/sync/push/progress.rs
    - src/sync/cli.rs
    - tests/live.rs

requirements-completed: [REPO-06, SYNC-05, UX-04]

duration: 2h
completed: 2026-08-19
status: complete
---

# Phase 4 / Plan 03: Uploads That Resume, Verify, and Say So

**A push now lists before it uploads, skips every pack already landed at a
matching name, size and state, deletes the zombie an interrupted upload leaves
behind, puts at most four bodies on the wire at once, and proves every asset it
uploaded is retrievable and byte-identical before the caller is allowed to
flip.** Progress has two implementations behind 4-01's trait, both driven by one
pure renderer. The keyfile gap that blocked Phase 5 is closed.

---

## READ THIS FIRST — two functions this plan delivers have no production call site

Both call sites are in files this plan was told not to edit, and both are one
line. Neither is wired, so both ship as library API that nothing invokes:

1. **`upload::ensure_keyfile` is never called.** It belongs in
   `src/sync/push/mod.rs`'s `run`, between step 4 (`ensure_release`) and step 5
   (`upload::run`) — it needs the same `permit` and the same `release_id`:
   ```rust
   upload::ensure_keyfile(&ctx, release_id, &permit).await?;
   ```
   **Until that line exists, a first push still publishes a pointer naming an
   asset that does not exist and Phase 5 still cannot bootstrap.** The gap 4-01
   recorded as deviation 9 is *solved* here, not *closed*. `push/mod.rs` is
   being rewritten by 4-04 (the 409 arm) and 4-05 (prune) right now, which is
   why this plan did not reach into it.

2. **`progress::reporter` is never called.** `src/sync/cli.rs:566` still passes
   `&mut Silent`, with a comment naming this plan. The change is:
   ```rust
   let mut progress = push::progress::reporter(std::io::stderr().is_terminal());
   match rt.block_on(push::run(ctx, progress.as_mut())) {
   ```
   plus `use std::io::IsTerminal;`. Until then **UX-04 is code, not behaviour**:
   a long push still prints nothing. `IsTerminal` is read at that call site and
   nowhere else, which is the whole point of the injected flag.

**The `ensure_keyfile` signature is exactly as specified and was not changed.**
4-06 can build against it as told.

---

## Task commits

1. `9ca3577` — test: the resume scan's three outcomes, the four-body ceiling, verification
2. `6f5eeb6` — feat: `upload::run`
3. `e13eb30` — test: the keyfile asset a first push must publish
4. `d6e978d` — feat: `ensure_keyfile`
5. `a906354` — test: progress on a terminal and off one
6. `f6dca92` — feat: `Terminal`, `Plain`, `render`, `reporter`
7. `39018a4` — test: CAL-5, the torn-upload state and the `digest` question
8. `104dcad` — fix: 4-01's push fixtures predate the resume scan's listing

## The shapes later plans build against

```rust
// src/sync/push/upload.rs
pub async fn run(ctx: &PushCtx<'_>, release_id: u64, packs: &[BuiltPack],
                 permit: &gate::Pushing, progress: &mut dyn Progress)
    -> Result<(usize, usize, u64)>;            // (uploaded, skipped, bytes_uploaded)

pub async fn ensure_keyfile(ctx: &PushCtx<'_>, release_id: u64,
                            permit: &gate::Pushing) -> Result<()>;

// src/sync/push/progress.rs
pub fn render(done: usize, total: usize, bytes_done: u64, bytes_total: u64) -> String;
pub struct Terminal<W = std::io::Stderr>;      // rewrites one line on stderr
pub struct Plain<W = std::io::Stderr>;         // one plain line per asset
pub fn reporter(is_terminal: bool) -> Box<dyn Progress>;
```

`bytes_uploaded` is the sum of `pack.bytes.len()` over the packs **actually
uploaded** — measured, never projected, and it excludes what was skipped. A test
compares the returned value against that sum directly.

## What `ensure_keyfile` does, precisely

- **Idempotent by content address.** The asset name is
  `keyfile_asset_name(content_address(canonical))`; a listing already showing
  that name in `ASSET_STATE_UPLOADED` ends the call without a request body. That
  is what makes it safe on every push rather than only the first.
- **It uploads the canonical serialization**, `serde_json::to_vec` of the
  keyfile as it sits on disk — not the file's literal bytes. Setup writes the
  keyfile *pretty-printed* (`setup::write_keyfile` uses `to_vec_pretty`) while
  `cli::keyfile_asset_for` addresses the compact form, so hashing the file as it
  sits would publish an asset whose name addresses different bytes than it
  holds. Nothing here re-wraps, re-derives, re-encrypts, or reads a password.
- **A torn keyfile asset is deleted first**, exactly as a torn pack is: GitHub
  creates the asset record before the body finishes, and the name is otherwise
  held forever — which for the keyfile means a pointer naming an unreadable
  asset.

**Known sharp edge, recorded rather than guessed at.** It publishes whatever
keyfile is on *this* machine's disk. If another machine has rekeyed and this one
still holds the superseded wrapper, calling this re-uploads a wrapper D5
destroyed on purpose. The pointer is unaffected — its `keyfile` comes from the
arriving pointer, per `push::run`'s rule 3 — so the bundle stays readable, but
the old wrapper returns as an orphan asset that `plan_deletions` never collects
(its rules only make *pack*-shaped names deletable). A guard against this was
written and then removed: every formulation expressible from `PushCtx` alone
also blocks the legitimate rekey case, because "my local keyfile is stale" and
"my local keyfile is the new one this run is publishing" are the same state as
seen from `ctx.previous`. **4-06 must write the new keyfile to disk before
calling this**, and is the right place to fix the resurrection — it already
deletes by name.

## The plan said `JoinSet`; there is no `JoinSet`

The plan's step 2 specifies `tokio::task::JoinSet`, spawned to four and refilled
with `join_next`. Two things that merged after it was written make that
**unimplementable**, not merely unattractive:

- Every write verb takes `gate::Pushing` **by reference**, and `Pushing` is not
  `Clone`. A spawned task must be `'static`, so it cannot carry the borrow that
  proves the gate was earned. Minting a second permit inside `upload.rs` would
  put gate logic in this module — precisely what T-4-21 says must not exist.
- `PushCtx` holds an `&Index`, whose `rusqlite::Connection` is `Send` but not
  `Sync`. A future borrowing the context is therefore not `Send` and cannot be
  spawned at all, permit or no permit.

What replaces it has the same semantics — up to four uploads outstanding,
refilled as each completes — as a window of boxed futures polled by hand with
`std::future::poll_fn`, which is `join_next` without the `'static` bound. Zero
crates added, and the borrow survives. The ceiling is asserted by a test that
counts uploads whose verifying download has not yet arrived.

## Deviations from the plan

**1. [Contract, forced] No `JoinSet`.** See above. The must-have — "no more than
four uploads in flight at once" — is met and tested; the named mechanism is not.

**2. [Test dropped, and why] There is no rate-limit-retry test at the
`upload::run` level.** The plan asks for "a rate-limited 403 retried through the
shared helper, succeeding on the second attempt without the test sleeping".
Production `upload_asset` routes through `retried()`, which passes
`tokio::time::sleep`, and `http::retry_delay` clamps every delay up to
`MIN_RETRY_DELAY` = **60 seconds** — a `Retry-After: 0` does not escape the
clamp. Making that test not sleep needs `tokio`'s `test-util` feature for
`start_paused`, which is a `Cargo.toml` change this plan forbids. The arm is
already covered by `write.rs`'s own `with_retry` tests (4-01). The **401** half
costs nothing and is tested here: one attempt, then failure.

**3. [Rule 3, blocking] `src/sync/cli.rs` was edited — test code only.** The
resume scan adds one `list_assets` before the first upload, which 4-01's four
end-to-end CLI tests answered with a 501. Each fixture gains an empty listing;
they are all first pushes. `mock_upload_path`'s takes `expect(1)` deliberately,
so the mid-push-public test's second listing (from the incident path) still
reaches that test's own mock — mockito prefers a matching mock still missing
hits. **No production line in `cli.rs` was touched.** This is the only edit
outside the plan's three files, and it is the mechanical consequence of the
request the plan required be added.

**4. [Additive, load-bearing] `ensure_keyfile`.** Not in the plan; closes 4-01's
deviation 9. See above, including the unwired-call-site warning.

**5. [Reuse over addition] Byte counts render through
`report::human_bytes`.** The plan allowed writing a KiB/MiB/GiB helper "if there
is none in `src/`". There is one, `pub(crate)`, with its own unit test.

## Two test defects found and fixed while writing them

Both were found by running negative controls rather than by reading, and both
had already gone green once while proving nothing:

- **The concurrency assertion was self-referential.** It read
  `assert_eq!(max, MAX_IN_FLIGHT)`, so raising the cap to 6 raised the
  expectation with it and the test passed at every value. It now pins the
  literal `4`; the control is red at 6 and green at 4, three runs each.
- **The counting mock counted requests it never answered.** The first version
  incremented an in-flight counter inside `match_request` while relying on a
  separate `match_query` to select the mock. mockito evaluates *every* candidate
  mock's `match_request` while looking for a match, so one incoming POST bumped
  the counter once per registered mock and the observed maximum was noise —
  it read 6 under a cap of 4. The predicate now does its own matching and the
  side effect happens only on a real hit.

## Was the live probe run?

**No.** CAL-5 is present, `#[ignore]`d, and skips with a printed message when
`GSD_CAL5_TOKEN` / `GSD_CAL5_REPO` are unset. Running it needs a throwaway
**private** repository and a `Contents: write` PAT, because unlike CAL-1 it
writes and deletes release assets. It cleans up after itself and removes its own
leftovers first, so it is re-runnable.

**So the two questions are still open, and the code is written for the
conservative answer to both:**

- **Asset `state` after a torn upload — unmeasured.** The resume scan skips only
  on the exact `"uploaded"` literal and deletes on everything else, so every
  unrecognised value fails in the safe direction. **The size check stays
  regardless of what the probe reports**: `state` is not authoritative, and a
  future reader must not drop a check believing that it is.
- **Whether GitHub populates `digest` — unmeasured.** D3's verifying download
  therefore stays exactly as implemented, at the cost of one extra download of
  newly-uploaded data (115 MB on a first push). If the probe finds `digest`
  populated *and* equal to a plain SHA-256 of the body — the probe computes and
  compares both — a later phase can verify against a locally computed hash and
  that download disappears. That is the single largest saving available on the
  first-push path.

The tear is a dropped future rather than a truncated body: reqwest's `stream`
feature is the dependency decision 4-01 recorded as closed, and a connection cut
at 250 ms of a 32 MiB body is the same thing from GitHub's side. The probe says
so loudly if the upload completes inside the window instead.

## Security properties, and how each is enforced

| Property | Enforcement |
|---|---|
| T-4-19 — a listed asset claiming to be a pack never uploaded | Skipping requires name **and** size **and** the uploaded state; anything else is deleted and re-uploaded. Three tests, one per outcome. |
| T-4-20 — altered bytes served back | `upload_one` recomputes `content_address` over what actually came back and fails `run` on a mismatch, so the orchestrator never reaches the flip. Tested with a mock serving different bytes; 4-01's `a_pack_that_does_not_verify_never_reaches_the_pointer_put` asserts `expect(0)` on the `PUT`. |
| T-4-21 — uploading before the gate | This module holds no gate logic and mints no permit; it takes `&gate::Pushing`, which cannot exist without a fresh `spend`. |
| T-4-22 — an unbounded verification download | `download_asset`'s `MAX_ASSET_BYTES` cap, untouched here. |
| T-4-23 — unbounded memory from concurrent bodies | Four in flight, enforced by the refill and asserted against the **literal** 4 with a working negative control. |
| T-4-24 — a retry storm | Every request inherits `with_retry` through the write verbs; there is no second retry loop in this file. |
| T-4-25 — a token or a chunk id in a progress line | `render` takes four integers. The asset name reaches `asset_done` and stops there; a test drives a token-shaped string through it and asserts the output carries neither it nor its prefix. |
| T-4-26 — another snapshot's asset deleted | Only names produced by *this run's* packs are ever matched, and only a matching name in a bad state is deleted. A test lists a stranger asset and asserts `expect(0)` on `DELETE`. |
| T-4-27 — a progress line mistaken for machine output | Both implementations write to standard error; the outcome stays on standard output. |
| T-4-SC — dependency surface | Zero new crates, zero new features. `Cargo.toml` and `Cargo.lock` are byte-identical to the branch point. |

## Verification

```
cargo test --lib sync::                     333 passed, 0 failed
cargo test --lib                           1315 passed, 0 failed   (baseline 1300)
cargo test (all targets)                   all green; 16 live tests ignored
cargo clippy --all-targets -- -D warnings  clean
cargo fmt --check                          clean
Cargo.toml / Cargo.lock                    unchanged vs 688beb3 (branch point)
```

- `cargo test` with no arguments does **not** execute CAL-5 — confirmed in the
  ignored list.
- `grep -n 'Utc::now' src/sync/push/{upload,progress}.rs` — **no hits.**
- No test added here sleeps, spawns a process, opens a socket outside the
  mockito base, reads a real `$HOME` or a real token, or uses production KDF
  parameters: the fixture wraps at `m_kib = 8`, and every path is under a
  `TempDir`.
- `IsTerminal` appears in `src/` only inside two doc comments explaining why it
  is *not* called here.
- None of `/user/repos`, `/orgs/`, `/generate`, `/forks` appears in any file this
  plan touched — the REPO-03 guard is still green.

## Not done here

- **Neither `ensure_keyfile` nor `reporter` is wired.** Both call sites are named
  at the top of this document, with the exact lines.
- CAL-5 is unrun, so `state` and `digest` stay at MEDIUM confidence and
  `docs/sync-format.md` §10 records neither.
- Skipped assets are not re-verified, by design: they carry a content-addressed
  name, a torn upload is caught by the state check, and re-downloading data an
  earlier run already verified would double the traffic of every resume.

## Self-Check: PASSED

- `src/sync/push/upload.rs`, `src/sync/push/progress.rs`, `src/sync/cli.rs`,
  `tests/live.rs` — all present and modified.
- Commits `9ca3577`, `6f5eeb6`, `e13eb30`, `d6e978d`, `a906354`, `f6dca92`,
  `39018a4`, `104dcad` — all present in `git log`.
