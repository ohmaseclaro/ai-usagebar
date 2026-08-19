---
phase: 04-push-packs-atomic-flip-gc-rekey
plan: 03
type: execute
wave: 2
depends_on: ["4-01"]
files_modified:
  - src/sync/push/upload.rs
  - src/sync/push/progress.rs
  - tests/live.rs
autonomous: true
requirements: [REPO-06, SYNC-05, UX-04]
must_haves:
  truths:
    - "A bundle of ~5,000 chunks uploads in one request per pack — under ten HTTP requests in total, asserted as a mock hit count (REPO-06)."
    - "A resumed push skips every asset already present with a matching name, size, and uploaded state, and re-uploads only what is missing (SYNC-05, D4)."
    - "An asset present but not in the uploaded state is deleted before it is retried, because GitHub creates the asset record before the body finishes."
    - "Every asset uploaded this run is verified retrievable with a matching digest before the caller is allowed to flip (D3 step 2)."
    - "No more than four uploads are in flight at once."
    - "A long push prints advancing asset-and-byte counts on a terminal, and periodic lines rather than a spinner when standard output is not a terminal (UX-04, D6)."
    - "Total bytes come from the finished packs' own lengths, never from a projection."
    - "No test sleeps, opens a socket outside the mockito base, or reads a real token; the asset-state probe lives in `tests/live.rs` behind `#[ignore]`."
  artifacts:
    - src/sync/push/upload.rs — the resume scan, the bounded-concurrency uploader, and digest verification
    - src/sync/push/progress.rs — the terminal and non-terminal `Progress` implementations behind 4-01's trait
    - "an `#[ignore]`d probe in tests/live.rs recording the real asset `state` transitions and whether GitHub populates `digest`"
  key_links:
    - "Resume is free because pack names are content addresses — the set difference is the whole mechanism, and no local record of `what I uploaded last time` is needed"
    - "`upload::run` returns counts to the orchestrator and never flips anything; the flip is 4-01's step 7 and stays the only commit point"
    - "`Progress` is 4-01's frozen trait — this plan adds implementations, never a per-chunk hook"
---

<objective>
Make the upload half real: skip what already landed, delete the zombies GitHub leaves behind when
a body is cut off, push at most four packs at a time, prove each one is retrievable before the
caller is allowed to flip, and show the user that something is happening.

Implements **SYNC-05** and **D4** (resume reuses what landed), **REPO-06** (one request per pack),
**UX-04** and **D6** (per-asset progress, non-TTY degrades to lines), and D3's verification step.

Purpose: the resume scan is what turns an interrupted 115 MB first push from a restart into a
continuation.
Output: `upload::run`, two `Progress` implementations, and one `#[ignore]`d live probe.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/04-push-packs-atomic-flip-gc-rekey/4-CONTEXT.md
@.planning/phases/04-push-packs-atomic-flip-gc-rekey/4-01-SUMMARY.md
@.planning/research/github-transport.md
@CLAUDE.md
@src/sync/github/write.rs
@src/sync/push/mod.rs
@src/vendor.rs
@tests/live.rs
</context>

<tasks>

<task type="auto" tdd="true">
  <name>Task 1: The resume scan, the bounded uploader, and digest verification</name>
  <files>src/sync/push/upload.rs</files>
  <behavior>
    - Given three packs and a mock listing one asset whose name, size, and state all match, exactly two uploads are issued.
    - Given a mock listing an asset whose name matches but whose state is not the uploaded one, a delete is issued for it and the pack is uploaded again.
    - Given a mock listing an asset whose name matches but whose size does not, a delete is issued and the pack is uploaded again.
    - Six packs against a mock that records concurrent in-flight requests never exceed four at once.
    - Each pack uploaded this run is fetched back and its content address recompared; a mock returning altered bytes makes `run` fail and the caller never reaches a flip.
    - A pack whose upload returns a rate-limited 403 is retried through the shared helper and succeeds on the second attempt without the test sleeping.
    - A pack whose upload returns 401 is not retried and fails immediately.
    - `run` reports bytes from the packs' own lengths; a test compares the reported total against the sum of `bytes.len()`.
  </behavior>
  <action>
Fill `upload::run(ctx: &PushCtx<'_>, release_id: u64, packs: &[BuiltPack], progress: &mut dyn Progress) -> Result<(usize, usize, u64)>`,
whose signature plan 4-01 froze, returning `(uploaded, skipped, bytes_uploaded)`.

**Step 1 — the resume scan.** `client.list_assets(release_id)` once. For each pack, look up
`push::pack_asset_name(&pack.id)`. Three outcomes, and all three matter:

- Present, `size` equal to `pack.bytes.len()`, and `state` equal to the uploaded state → skip. This
  is D4, and it is exact rather than heuristic because the name **is** the content address: a
  changed pack gets a different name, so there is no version of this question that needs a local
  record of what a previous run uploaded.
- Present but `state` is anything else, or `size` disagrees → `client.delete_asset` first, then
  upload. GitHub creates the asset record before the body finishes, so an interrupted upload
  leaves a zombie that would otherwise make the name collide forever. Never trust the name alone.
- Absent → upload.

Assets in the release that match no pack in this run are **left completely alone**. They belong to
other snapshots; deleting them is prune's job (plan 4-05) and only after a successful flip.

**Step 2 — the uploads, four at a time.** Use `tokio::task::JoinSet`, which the already-enabled
`rt` feature supplies — do not add a tokio feature and do not reach for a semaphore. Spawn up to
four, then `join_next` to refill, so exactly four bodies are in flight at the ceiling. The
research's documented limit is 100 concurrent, but with 32 MiB bodies the run is bandwidth-bound
long before it is request-bound and a low cap keeps a laptop's uplink usable. Each spawned task
takes an owned clone of the `Client` (`reqwest::Client` clones cheaply) and owned bytes.

Every request goes through `write::upload_asset`, which already routes through the shared retry
helper, so D7's rate-limit discipline is inherited rather than re-implemented here. Do not add a
second retry loop.

**Step 3 — verification, which D3 makes a precondition of the flip.** For each asset uploaded
*this run*, `client.download_asset` it and compare `crypto::content_address` of what comes back
against the pack's id. A mismatch fails `run`, so the orchestrator never reaches step 7 and no
pointer ever references a pack that did not verify. Skipped assets are accepted on name, size,
and state: they carry a content-addressed name and a torn upload is caught by the state check, and
re-downloading data an earlier run already verified would double the traffic of every resume.

Say plainly in the module doc what this check is and is not: a corrupt pack would in any case fail
its per-blob Poly1305 tags on read, so this catches transport and packaging bugs, not attacks. It
costs one extra download of newly-uploaded data — on a 115 MB first push, 115 MB — and that is
the price D3 sets. Record in the summary whether the live probe below found GitHub populating the
asset `digest` field, because if it does, a later phase can compare a locally computed SHA-256
instead and the extra download disappears.

**Progress.** Call `progress.start(packs_to_upload, total_bytes)` where `total_bytes` is the sum
of `pack.bytes.len()` over the packs actually being uploaded — measured, never projected. Call
`progress.asset_done` as each completes. The granularity is the asset and there is no per-chunk
hook, per D6.

Nothing in this file prints. Rendering belongs to the `Progress` implementations in task 2, which
keeps the uploader testable with `Silent`.
  </action>
  <verify>
    <automated>cargo test --lib sync::push::upload</automated>
  </verify>
  <done>`cargo test --lib sync::push::upload` is green. The three resume outcomes each have a test asserting the exact mock hits. A concurrency test proves no more than four bodies are in flight. A tampered download fails `run`. Reported bytes equal the sum of the uploaded packs' lengths. No test sleeps, and none reads a real token or `$HOME`.</done>
  <precondition>Plan 4-01 is merged: `write::{list_assets, upload_asset, delete_asset, download_asset}`, `with_retry`, `Asset`, `BuiltPack`, `PushCtx`, and the `Progress` trait exist with the signatures `4-01-SUMMARY.md` records.</precondition>
</task>

<task type="auto" tdd="true">
  <name>Task 2: Progress a user can read, on a terminal and off one</name>
  <files>src/sync/push/progress.rs, tests/live.rs</files>
  <behavior>
    - The terminal implementation rewrites one line per completed asset, carrying asset index, asset count, bytes done, and byte total.
    - The non-terminal implementation emits a plain newline-terminated line per completed asset and emits no carriage returns and no escape sequences at all.
    - Both are driven through the same `Progress` trait, so `upload::run` needs no branch.
    - Rendering is a pure function of the counters, tested against a string rather than against a terminal.
    - The chosen implementation follows an injected `is_terminal` flag, so both are testable without a tty.
  </behavior>
  <action>
Add two implementations of 4-01's `Progress` trait beside its `Silent`.

Split each into a pure renderer and a thin writer: `fn render(done: usize, total: usize, bytes_done: u64, bytes_total: u64) -> String` is what the tests assert against, and the impls write its
output. A progress reporter whose correctness can only be checked by looking at a terminal is a
progress reporter with no tests.

The terminal implementation writes to standard error with a leading carriage return and no
newline, so successive updates overwrite one line, and emits a final newline from `finish`.
Standard error, not standard output: `sync push`'s stdout is the machine-readable outcome and a
progress line in it would corrupt anything piping the command.

The non-terminal implementation writes plain lines. D6 is explicit that non-TTY output degrades
to periodic lines and never a spinner, because the macOS menu bar captures this command's output
as a subprocess and carriage returns and escape sequences make that capture unreadable. Rate-limit
it to at most one line per asset — with 32 MiB assets that is already the right cadence — and emit
a final summary line.

Choose between them from an injected `is_terminal: bool` supplied by the CLI, using
`std::io::IsTerminal` at the one production call site. Do not call `IsTerminal` inside the
constructor: an ambient environment read is exactly what makes a test non-hermetic, and the
project's convention is to inject the fact.

Byte counts render in whatever human-readable helper the crate already has; if there is none in
`src/`, write the smallest one that handles KiB/MiB/GiB and put it here rather than pulling a
crate.

**`tests/live.rs`** — add one `#[ignore]`d probe answering the two questions the research left at
MEDIUM confidence, both of which change later code if they resolve. Upload a small asset, cut the
body off mid-transfer, and record what `state` the asset reports and whether it ever leaves that
state; then upload a complete asset and record whether GitHub populates `digest` and in what
form. Follow the existing `cal1_range_on_private_release_asset` shape exactly: read its token,
repository, and asset from environment variables, **skip with a printed message when they are
absent** so it is never a hard failure, and print the findings with `--nocapture` rather than
asserting a shape we are guessing at. Print the reproduction command in the test's doc comment.
Nothing in `src/` may depend on the outcome; this probe records reality so a later phase can act
on it.
  </action>
  <verify>
    <automated>cargo test --lib sync::push::progress</automated>
  </verify>
  <done>`cargo test --lib sync::push::progress` is green. Both implementations exist behind 4-01's trait, both render through one pure function tested against strings, and the non-terminal one emits no carriage return and no escape sequence — asserted by a test over its rendered output. The live probe is present, `#[ignore]`d, and skips with a message when its environment variables are absent. `cargo test` with no arguments does not run it.</done>
  <precondition>Plan 4-01 is merged, so the `Progress` trait and `Silent` exist. `tests/live.rs` exists from Phase 1 with the `cal1_range_on_private_release_asset` probe to copy the skip-when-unset shape from.</precondition>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|---|---|
| process → uploads host | Credential-bearing ciphertext leaves the machine here, in bulk, for the first time |
| GitHub asset listing → resume decision | Attacker-controlled `name`, `size`, and `state` decide whether a pack is uploaded or trusted as present |
| downloaded asset → verification | Attacker-controlled bytes, fetched from signed storage the project does not control |
| progress output → terminal, logs, and the menu bar's subprocess capture | Anything rendered here is an exfiltration path and a parsing hazard |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|---|---|---|---|---|---|
| T-4-19 | Spoofing | a listed asset claiming to be a pack that was never uploaded | critical | mitigate | Skipping requires name **and** size **and** the uploaded state; anything else is deleted and re-uploaded, and every asset uploaded this run is re-downloaded and its content address recompared before the caller may flip |
| T-4-20 | Tampering | altered bytes served back during verification | critical | mitigate | Verification recomputes `content_address` over what actually came back; a mismatch fails `run`, so the orchestrator never reaches the flip |
| T-4-21 | Elevation of privilege | uploading before the gate | critical | mitigate | `upload::run` is called only from 4-01's step 5, after `assert_pushable` and `assert_fresh`; this module holds no gate logic and cannot be entered another way |
| T-4-22 | Denial of service | an unbounded download during verification | high | mitigate | `download_asset`'s `MAX_ASSET_BYTES` cap, checked before allocating; this module does not relax it |
| T-4-23 | Denial of service | unbounded memory from concurrent bodies | medium | mitigate | Four in flight at 48 MiB maximum each; the cap is enforced by the `JoinSet` refill and asserted by a concurrency test |
| T-4-24 | Denial of service | a retry storm on a rate limit | medium | mitigate | All requests inherit 4-01's `with_retry`; this plan adds no second retry loop, which is what would multiply the attempt count |
| T-4-25 | Information disclosure | a token, a path, or a chunk id in a progress line | high | mitigate | Progress renders counts and byte totals only; the asset name it may show is a content address, and a test asserts no rendered line contains a token substring |
| T-4-26 | Tampering | another snapshot's asset deleted during the resume scan | critical | mitigate | Only assets whose names match a pack in **this** run are ever deleted; assets matching nothing are untouched, and deletion of unreferenced assets belongs to prune, after the flip |
| T-4-27 | Repudiation | a progress line mistaken for machine output | low | mitigate | Progress goes to standard error; the outcome goes to standard output |
| T-4-SC | Tampering | dependency surface | low | accept | Zero new crates and zero new features — `JoinSet` comes from the already-enabled tokio `rt`. `Cargo.toml` is not in `files_modified`; an executor that believes it needs an edit there should stop, because that is the `stream`-feature decision 4-01 recorded, not a step to take |
</threat_model>

<verification>
- `cargo test --lib sync::push::upload` and `cargo test --lib sync::push::progress` are green.
- `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` are clean.
- `cargo test` with no arguments does not execute the live probe.
- No test in this plan sleeps, spawns a process, opens a socket outside the mockito base, or reads
  a real `$HOME` or a real token.
- `Cargo.toml` is unchanged by this plan.
</verification>

<success_criteria>
A first push of many packs issues one request per pack, four at a time, and shows advancing
counts. A push killed partway and re-run uploads only what is missing, deletes the zombie the
kill left behind, and reaches the same end state. Every pack this run uploaded is proven
retrievable and byte-identical before the caller is allowed to flip.
</success_criteria>

<output>
Create `.planning/phases/04-push-packs-atomic-flip-gc-rekey/4-03-SUMMARY.md` when done.

Record the exact `upload::run` return shape and the two `Progress` implementations' names, and
state whether the live probe was run. If it was, record what asset `state` a torn upload reports
and whether `digest` is populated — the second answer is what would let a later phase drop
verification's extra download.
</output>
