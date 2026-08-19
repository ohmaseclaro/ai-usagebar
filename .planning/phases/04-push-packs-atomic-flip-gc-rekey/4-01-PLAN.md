---
phase: 04-push-packs-atomic-flip-gc-rekey
plan: 01
type: execute
wave: 1
depends_on: []
files_modified:
  - src/sync/mod.rs
  - src/sync/github/mod.rs
  - src/sync/github/write.rs
  - src/sync/push/mod.rs
  - src/sync/push/packer.rs
  - src/sync/push/pointer.rs
  - src/sync/push/upload.rs
  - src/sync/push/progress.rs
  - src/sync/push/prune.rs
  - src/sync/push/rekey.rs
  - src/config.rs
  - src/widget/cli.rs
  - src/sync/cli.rs
  - docs/sync-format.md
autonomous: true
requirements: [REPO-06, REPO-07, SYNC-04]
must_haves:
  truths:
    - "`ai-usagebar sync push` against a mockito server moves one file into one pack, uploads it as **one** release asset, and publishes the pointer with a `sha` precondition — end to end (REPO-06, REPO-07, D3)."
    - "Killing the run after the upload and before the pointer `PUT` leaves the remote pointer byte-identical to what it was; nothing a reader resolves through has changed (SYNC-04, D3)."
    - "The push re-runs `fetch_facts` + `assert_pushable` itself and calls `PushClearance::assert_fresh` before the first byte — a clearance from `sync setup` is never carried in."
    - "A repo that reads private on the first gate and public on the re-gate aborts before the flip, deletes the assets this run uploaded, and names the credentials to rotate."
    - "Every remote write routes through one retry helper that honours `Retry-After` and `x-ratelimit-reset` and never retries a 4xx that is not a rate-limited 403 (D7)."
    - "No test opens a socket to a host that is not an injected `Endpoints` base, spawns a process, or reads a real `$HOME`."
    - "Nothing in the phase seals a **new kind of object** under `chunk_key`, so the deferred AAD object-type separator stays untriggered."
  artifacts:
    - src/sync/github/write.rs — the six write verbs and the D7 retry helper, the only place in the crate that sends a request body
    - src/sync/push/mod.rs — the frozen remote-layout types (`Pointer`, `SnapshotRecord`, `RemoteIndexEntry`, `BuiltPack`, `PushBundle`, `PushCtx`, `PushOutcome`) and the orchestrator
    - src/sync/push/pointer.rs — `load` and the CAS `commit`, the single linearization point
    - "`[sync] keep_snapshots` on `SyncConfig`, defaulting to 10 (D1)"
    - "`SyncAction::{Push, Prune, Rekey}` dispatched from `sync::cli`"
    - docs/sync-format.md §10 — the remote layout, so a re-implementer can write a reader
  key_links:
    - "`src/sync/mod.rs` declares `pub mod push;` and `github/mod.rs` declares `pub mod write;` — without both, nothing in the phase compiles"
    - "**Every cross-module type lives in `push/mod.rs`, never in a sibling.** Five wave-2 plans each own one file; a type declared in `packer.rs` and consumed by `upload.rs` would make two worktrees uncompilable"
    - "`write.rs` is created and *filled* here, not stubbed: `list_assets` and `delete_asset` are needed by 4-03 and 4-05, which are in the same wave and cannot edit each other's files"
    - "`pointer::commit` takes a rebuild closure, so 4-04 adds the 409 arm inside `pointer.rs` without touching the orchestrator that supplies the closure"
    - "`prune` receives the pointer that actually **landed**, so it structurally cannot delete a pack a competing pointer references"
---

<objective>
The tracer for Phase 4: one file, one pack, one asset, one compare-and-swap — the whole
outbound path wired end to end against `mockito`, production quality, with the gate re-earned
inside the push.

It also fixes the remote layout and every cross-file signature for the rest of the phase.
Five wave-2 plans each fill one file this plan creates; none of them edits another's.

Implements **D3** (the flip is the only commit point), **D7** (rate-limit discipline), and the
config half of **D1** (`keep_snapshots`).

Purpose: prove the architecture on one request before five plans build out from it — an
architectural dead end found now costs one commit, not ten.
Output: `src/sync/push/`, `src/sync/github/write.rs`, `ai-usagebar sync push`,
`docs/sync-format.md` §10.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/04-push-packs-atomic-flip-gc-rekey/4-CONTEXT.md
@.planning/research/github-transport.md
@docs/sync-format.md
@CLAUDE.md
@src/sync/mod.rs
@src/sync/pack.rs
@src/sync/model.rs
@src/sync/plan.rs
@.planning/phases/03-github-auth-and-the-private-repo-gate/3-01-SUMMARY.md
@.planning/phases/02-bundle-scope-local-index-dry-run-planning/2-05-SUMMARY.md
</context>

<source_audit>
Phase-wide coverage audit. Every source item maps to a plan.

| Source | Item | Covered by |
|---|---|---|
| GOAL | The bundle reaches the private repo; an interrupted push never leaves a readable half-snapshot; the remote does not grow without bound | 4-01 … 4-07 |
| REQ | REPO-06 bulk data as a small number of large objects, inside 80/min and 500/hour | 4-01 (one asset per pack, end to end), 4-02 (packing), 4-03 (one request per pack, concurrency 4) |
| REQ | REPO-07 pointer published with a compare-and-swap precondition | 4-01 (the `sha`-preconditioned `PUT`), 4-04 (the 409 arm) |
| REQ | SYNC-04 an interrupted push leaves the previous consistent state | 4-01 (flip is the only commit point), 4-04, 4-07 |
| REQ | SYNC-05 a resumed push reuses what already uploaded | 4-03, 4-07 |
| REQ | SYNC-07 remote storage does not grow without bound | 4-05, 4-07 |
| REQ | CRYPTO-04 change the password without re-uploading the bundle | 4-06 |
| REQ | UX-04 progress is visible for a long first push | 4-03 |
| RESEARCH | Release assets for bulk data; Contents API `PUT` with `sha` for the pointer | 4-01 |
| RESEARCH | Resume by name + size + `state == "uploaded"`; `DELETE` a torn asset before retry | 4-03 |
| RESEARCH | Concurrency capped at 4 | 4-03 |
| RESEARCH | `retry-after` → `x-ratelimit-reset` → jittered exponential; 401 ≠ 403 | 4-01 (`with_retry`, over Phase 3's `classify`/`retry_delay`) |
| RESEARCH | Re-assert visibility after the upload, before the flip; a public read is an incident | 4-01 |
| RESEARCH | Asset `state` transitions are MEDIUM confidence — verify live | 4-03 (`tests/live.rs`, `#[ignore]`d) |
| RESEARCH | GC only after a successful flip, only assets the current pointer does not reference | 4-05 |
| RESEARCH | AUP §9 excessive bandwidth is a real risk worth telling the user about | 4-07 |
| CONTEXT | D1 `[sync] keep_snapshots = 10`, config not constant | 4-01 (the key), 4-05 (the retention) |
| CONTEXT | D2 prune is automatic after a successful push, is a warning on failure, and deletes the snapshot record before any pack | 4-01 (warning wiring), 4-05 |
| CONTEXT | D3 upload → verify → flip; the flip is the only commit point | 4-01, 4-03 (verify), 4-04 |
| CONTEXT | D4 resume skips packs already present with a matching digest | 4-03 |
| CONTEXT | D5 rekey re-wraps and verifiably deletes the old keyfile asset | 4-06 |
| CONTEXT | D6 per-asset progress; non-TTY degrades to periodic lines | 4-03 |
| CONTEXT | D7 honour `Retry-After` and `x-ratelimit-*`; never retry a 4xx that is not a rate-limited 403 | 4-01 |
| CONTEXT | Risk: the pack header is single-chunk and its slack tracks `PACK_TARGET` | 4-02 (the ceiling assertion, and `PACK_TARGET` left at 32 MiB) |
| CARRIED | Phase 3: `PushClearance` must be re-earned inside the push | 4-01 |
| CARRIED | Phase 2: the `chunk` table has no writer | 4-02 |
| CARRIED | Phase 1: the AAD object-type separator is deferred, and a new object kind under `chunk_key` triggers it | 4-01 (the layout adds no such object — stated, and asserted in 4-07) |

**Two reconciliations with ROADMAP §Phase 4, both deliberate, neither a scope reduction.**

*The draft release is dropped.* ROADMAP steps 3 and 7 create the release as a draft and publish
it before the flip. A draft release **has no git tag until it is published**, so
`GET /releases/tags/{tag}` returns 404 for it — the resume scan (SYNC-05, locked as D4) would
need a full release listing plus name matching to find its own crashed predecessor. That is real
machinery bought to reduce exposure of *ciphertext* during a repo-goes-public window that the
re-gate's incident path already handles by deleting the assets. One published release, one fixed
tag, created once. Atomicity is unaffected: it comes from the flip (D3), not from draft state.

*The `stream` feature is not added to `reqwest`.* ROADMAP asks for it "so a large pack is not
buffered in RAM", reasoning from the research's ~1.9 GiB packs. CAL-1 was not run
(`docs/sync-format.md` §7), so its recorded fallback stands and `PACK_TARGET` is **32 MiB** with
`PACK_MAX` 48 MiB. `PackWriter::finish` already returns a `Vec<u8>`, so a `Vec` body needs no new
feature, no tempfile, and no `cargo machete` churn; peak upload memory is 4 × 48 MiB. **The
trigger is named:** if a future phase runs CAL-1 positive and raises `PACK_TARGET` past ~256 MiB,
add the `stream` feature, write packs to a `NamedTempFile`, and re-check `pack.rs`'s single-chunk
header ceiling at the same time.
</source_audit>

<tasks>

<task type="tracer" tdd="true">
  <name>Task 1: The six write verbs, and the retry discipline they all route through</name>
  <files>src/sync/github/mod.rs, src/sync/github/write.rs</files>
  <behavior>
    - `ensure_release` against a mock returning 200 for the tag yields that release id and issues no `POST`.
    - `ensure_release` against a mock returning 404 for the tag issues one `POST /releases` and yields the created id.
    - `list_assets` parses `id`, `name`, `size`, `state`, and an optional `digest`, and tolerates the field being absent.
    - `upload_asset` sends its body to the **uploads** base, not the api base, with `Content-Type: application/octet-stream`.
    - `upload_asset` against a mock returning 422 with an `already_exists` error re-lists and returns the existing asset rather than failing.
    - `download_asset` follows the redirect to signed storage and the second request carries **no** `Authorization` header.
    - `download_asset` refuses a body larger than the asset cap without allocating it.
    - `get_contents` on 404 yields `None`; on 200 it yields the blob `sha` and the decoded bytes; a body past the pointer cap is refused.
    - `put_contents` with `Some(sha)` sends that `sha`; with `None` it omits the field entirely.
    - `with_retry` retries a `RateLimited` up to its attempt cap, sleeping the delay `http::retry_delay` computed, and returns the last error rather than looping.
    - `with_retry` returns immediately on `Unauthorized`, `Forbidden`, `NotFound`, and `Conflict` — no second attempt is made, asserted by a mock expecting exactly one hit.
  </behavior>
  <action>
Add `pub mod write;` to `src/sync/github/mod.rs`. That is the **only** edit this plan makes to
that file — plan 3-01's guard test reads `include_str!("mod.rs")` and fails if a request-body
call site appears in it, and that guard keeps doing its job precisely because Phase 4's writes
land in their own module. Rust allows an inherent `impl Client` block in a sibling module of the
same crate, so `write.rs` extends `Client` without reopening `mod.rs`.

Create `src/sync/github/write.rs`. It is the only file in the crate that sends a request body,
and it says so in its module doc.

`pub struct Asset { pub id: u64, pub name: String, pub size: u64, pub state: String, pub digest: Option<String> }`
deserialized from GitHub's release-asset JSON. `state` is a `String` rather than an enum: the
research rates its transitions MEDIUM confidence, and an enum that panics or errors on an
undocumented value would turn a surprise into an outage. Callers compare it against the literal
for the uploaded state. `digest` is `Option` because GitHub does not populate it on every asset;
nothing in this phase depends on it, and plan 4-03 records live whether it arrives.

Six methods on `Client`, each taking `now: DateTime<Utc>` as its last parameter — never reading
the clock inside — and each mapping a non-2xx through Phase 3's `http::classify`:

`ensure_release(&self, repo, tag, now) -> Result<u64>` — `GET /repos/{o}/{r}/releases/tags/{tag}`;
on `NotFound`, `POST /repos/{o}/{r}/releases` with `tag_name` set to `tag`, `name` the same, and
`body` a single line saying the release holds encrypted ai-usagebar sync data and is written by
the tool. Not a draft, per the reconciliation in the source audit. Returns the release id.

`list_assets(&self, repo, release_id, now) -> Result<Vec<Asset>>` — paginated
`GET /repos/{o}/{r}/releases/{id}/assets?per_page=100`, following `page` until a short page.
Stop at a hard page cap derived from the documented 1000-assets-per-release ceiling and error
if it is reached, naming the ceiling: an unbounded pagination loop against a hostile remote is a
denial of service.

`upload_asset(&self, repo, release_id, name, body: Vec<u8>, now) -> Result<Asset>` —
`POST {uploads_base}/repos/{o}/{r}/releases/{id}/assets?name={name}`, `Content-Type:
application/octet-stream`, the body as `reqwest::Body::from(Vec<u8>)`. `name` is
percent-safe by construction (our asset names are the literal prefixes plus 64 hex characters);
assert that with a debug check rather than reaching for a percent-encoding crate. A `422` whose
body names an `already_exists` error is **not** a failure: re-list and return the matching asset,
because a retried upload after a response was lost is the ordinary case, not an anomaly.

`delete_asset(&self, repo, asset_id, now) -> Result<()>` —
`DELETE /repos/{o}/{r}/releases/assets/{id}`. A 404 is success: the asset is gone, which is the
outcome the caller asked for. This is the one destructive verb in the crate; say so in its doc
comment and name its two callers (prune and rekey).

`download_asset(&self, repo, asset_id, now) -> Result<Vec<u8>>` —
`GET /repos/{o}/{r}/releases/assets/{id}` with `Accept: application/octet-stream`. GitHub answers
with a 302 to signed storage. `Client`'s reqwest instance carries
`vendor::same_origin_redirect_policy()`, so that hop is **refused**, and this method must not
work around it by relaxing the policy. Instead: read the `Location` header from the 3xx and issue
the second request from a **separate, token-free** `reqwest::Client` built here, carrying no
`Authorization` header at all. That is the point — the signed URL already carries its own
authorization, and replaying a `Contents: write` bearer token to a storage host we do not control
is exactly the leak the same-origin policy exists to prevent. Bound the body at
`pub const MAX_ASSET_BYTES: u64 = 64 * 1024 * 1024;` — comfortably above `pack::PACK_MAX`, and
checked against `Content-Length` before allocating as well as while reading, so a lying header
cannot exhaust memory either. `vendor::MAX_BODY_BYTES` is for kilobytes of JSON and must not be
used here; equally, this cap must not be relaxed for Phase 5's bundle download, which streams to
a file instead.

`get_contents(&self, repo, path, now) -> Result<Option<(String, Vec<u8>)>>` —
`GET /repos/{o}/{r}/contents/{path}`; `NotFound` yields `Ok(None)`, which is first-push. On 200,
return the response's `sha` together with the base64-decoded `content`. Bound the body at
`pub const MAX_POINTER_BYTES: u64 = 1024 * 1024;` — the Contents API's own full-support
threshold, and far above anything this format writes.

`put_contents(&self, repo, path, message, body: &[u8], sha: Option<&str>, now) -> Result<String>` —
`PUT /repos/{o}/{r}/contents/{path}` with `content` base64 of `body` and the `sha` field
**present only when `sha` is `Some`**. Omitting it is "create, and fail if it exists"; sending a
stale one is the compare-and-swap that yields 409. Serialize the request with a struct whose
`sha` field carries `skip_serializing_if`, not with a hand-built map — a `sha` of `null` is not
the same request as no `sha`. Returns the new blob `sha` from the response, which the caller
keeps for its next flip.

`pub(crate) async fn with_retry<T, F, Fut, S, SFut>(attempts: u32, sleep: S, now: DateTime<Utc>, op: F) -> Result<T>`
is D7 in one place. `op` is an async closure returning `Result<T>`; `sleep` is an injected
async fn taking a `Duration`, so **no test sleeps**. Retry only when the error is
`GithubError::RateLimited` or `GithubError::Transport`, waiting the duration Phase 3's
`http::retry_delay` derives from the headers. Return immediately on `Unauthorized`, `Forbidden`,
`NotFound`, and `Conflict` — a retried 401 is a slower failure, and a retried 409 would overwrite
the very state the precondition exists to protect. On exhausting `attempts`, return the last
error unchanged so the caller still gets Phase 3's actionable text. Every one of the six methods
above wraps its request in this helper; production passes `tokio::time::sleep`.

No method in this file logs, prints, or formats a header map. The bearer token is set by
`Client`'s existing request builder and appears nowhere in this module's own code.

Tests use `mockito::Server::new_async()` with both `Endpoints` fields pointed at `server.url()`,
which is what makes the uploads path testable at all.
  </action>
  <verify>
    <automated>cargo test --lib sync::github::write</automated>
  </verify>
  <done>`cargo test --lib sync::github::write` is green. All six verbs exist with the frozen signatures above; `with_retry` retries exactly the two retryable arms and no other. A test asserts `download_asset`'s second request carries no `Authorization` header. A test asserts a body past `MAX_ASSET_BYTES` and one past `MAX_POINTER_BYTES` are each refused. No test sleeps, opens a socket outside the mockito base, or reads a real `$HOME`.</done>
  <reversibility rating="costly">These six signatures and `Asset`'s shape are what 4-03, 4-04, 4-05, and 4-06 build against in four parallel worktrees; changing one after this merges reworks all four. They come from `github-transport.md` §5.1 and D7 and are not to be improvised.</reversibility>
  <precondition>Phase 3 is merged: `Client`, `Endpoints`, `RepoRef`, `GithubError`, `http::classify`, and `http::retry_delay` all exist. Read `3-01-SUMMARY.md` and `3-03-SUMMARY.md` for their exact signatures rather than re-deriving them from the plans.</precondition>
</task>

<task type="tracer" tdd="true">
  <name>Task 2: The remote layout, and the pointer as the single linearization point</name>
  <files>src/sync/mod.rs, src/sync/push/mod.rs, src/sync/push/pointer.rs, src/sync/push/packer.rs, src/sync/push/upload.rs, src/sync/push/progress.rs, src/sync/push/prune.rs, src/sync/push/rekey.rs, docs/sync-format.md</files>
  <behavior>
    - A `Pointer` round-trips through JSON with its snapshot list intact and its `format` field probed before deserialization.
    - `pointer::load` against a mock 404 yields "no pointer yet" and `None` for the `sha`; against a 200 it yields the parsed pointer and its `sha`.
    - `pointer::load` refuses a pointer whose `format` is above this build's ceiling with the "upgrade ai-usagebar" wording, and one whose `repo_id` is not the caller's own.
    - `pointer::commit` on a first push sends no `sha`; on a subsequent push it sends the `sha` `load` returned.
    - `pack_asset_name` and `keyfile_asset_name` produce their documented shapes and are pure functions of a `ChunkId`.
    - `repo_id_for` never produces an empty string, which `Root::seal` refuses.
  </behavior>
  <action>
Add `pub mod push;` to `src/sync/mod.rs` and extend that module's layout doc with the new file.

Create `src/sync/push/` with six files. Every file is created here so each wave-2 plan owns whole
files and no two ever edit the same one. Files this plan does not fill carry their frozen public
signature plus a working minimum; the gaps are functional, never architectural.

**`push/mod.rs`** declares the five submodules (`packer`, `pointer`, `progress`, `prune`,
`rekey`, `upload`) and holds **every type that crosses a module boundary**. This is the single
most load-bearing rule in the plan: a type declared in `packer.rs` and consumed by `upload.rs`
would leave two wave-2 worktrees unable to compile, because neither owns the other's file.

Constants: `POINTER_PATH` = the literal `sync/pointer.json`, `RELEASE_TAG` = the literal
`ai-usagebar-sync-v1`, `POINTER_VERSION` = 1 and `MAX_SUPPORTED_POINTER` = 1 (read through
`sync::check_version`, at-or-below like every other object).

Naming, as pure functions: `pack_asset_name(&ChunkId) -> String` rendering `pack-` then the 64
hex characters then `.bin`, and `keyfile_asset_name(&ChunkId) -> String` rendering `keyfile-`
then 64 hex then `.json`. Both are content-addressed, which is what makes D4's "already
uploaded" a question with an exact answer, and what lets a rekeyed keyfile coexist with its
predecessor for the instant between upload and delete. Note in the doc comment that
`pack::shard_path`'s two-level directory layout is for a filesystem store; a release asset name
cannot contain a path separator, so the flat form is used here and both address the same bytes.

`repo_id_for(pairing_repo_id: u64) -> String` renders `github:` then the id. It reads the
**pairing record's** numeric repo id, never the id in a response being processed — the format's
§5 rule that a reader binds its own identifier. It can never be empty, which matters because
`Root::seal` refuses an empty `repo_id` and the refusal would otherwise surface as a confusing
failure at the end of a long push.

Types:

`BuiltPack { pub id: ChunkId, pub bytes: Vec<u8> }` — one finished, not-yet-uploaded pack, `id`
being `crypto::content_address` of `bytes`.

`RemoteIndexEntry { pub id: ChunkId, pub pack: ChunkId, pub offset: u64, pub clen: u32, pub true_len: u32 }`,
`Serialize`/`Deserialize`. This is the bootstrap: a reader holding only the pointer needs to know
where the **index object's own chunks** live, or it can never resolve anything else. It carries
the same five fields as `model::IndexEntry` and is a separate type because it is written in the
clear, which `IndexEntry` is not.

`SnapshotRecord { pub root: String, pub index_chunks: Vec<RemoteIndexEntry>, pub packs: Vec<ChunkId> }`
where `root` is base64 of the sealed snapshot root. `packs` names **every** pack the snapshot
needs, reused ones included — that is what makes prune computable from the pointer alone, with
no download and no key.

`Pointer { pub format: u32, pub repo_id: String, pub keyfile: String, pub snapshots: Vec<SnapshotRecord> }`,
`snapshots` ordered oldest first, newest last, at most `keep_snapshots` long.

Document, in `push/mod.rs` and again in `docs/sync-format.md`, exactly why this container is
plaintext and why that is not a regression: **it introduces no new kind of object sealed under
`chunk_key`**, so Phase 1's deferred AAD object-type separator stays untriggered. Every element
of value inside it is already sealed — the root under `root_key` with the reader's own `repo_id`
bound as associated data. Tampering with the container therefore dead-ends rather than opening
anything: dropping entries is a rollback, which the local anchor's counter catches; reordering is
inert, because the reader selects by the `counter` inside each sealed root, not by position;
adding a fabricated entry fails the Poly1305 tag. Deliberately **not** carried in the clear:
`counter` and `created_at`. They would be redundant leakage — the reader opens at most
`keep_snapshots` small roots to find the newest, and prune needs neither. If a later phase finds
itself wanting to seal this container, that is the trigger for the object-type separator and it
must be raised loudly rather than done quietly.

`PushCtx<'a>` bundling what the orchestrator needs: `client`, `repo`, `cfg: &SyncConfig`,
`roots: &SyncRoots`, `keys: &Keys`, `index: &Index`, `repo_id: String`, `keyfile_asset: String`,
`now: DateTime<Utc>`. A tuple of nine arguments threaded through five modules is how signatures
drift; one struct is how they do not.

`PushBundle { pub packs: Vec<BuiltPack>, pub root: Vec<u8>, pub index_chunks: Vec<RemoteIndexEntry>, pub referenced_packs: Vec<ChunkId>, pub counter: u64 }`
— everything one push has to put on the wire, produced by `packer::build`.

`PushOutcome { pub packs_uploaded: usize, pub packs_skipped: usize, pub bytes_uploaded: u64, pub snapshots_kept: usize, pub packs_deleted: usize, pub prune_warning: Option<String> }`.
`prune_warning` is `Option` rather than an error return because D2 is explicit: a prune failure
is a warning and never a push failure. Encoding that in the type means no later plan can
accidentally make it fatal.

`pub async fn run(ctx: PushCtx<'_>, progress: &mut dyn Progress) -> Result<PushOutcome>` is the
orchestrator, and it runs in this order, which is D3 and is not negotiable:

1. `gate::fetch_facts` then `gate::assert_pushable` with `cfg.includes(SyncCategory::Credentials)`
   as `credentials_in_bundle`. **Run here, inside the push.** A clearance obtained during
   `sync setup` is never accepted: a repository can be flipped public from the web UI between the
   two. Call `PushClearance::assert_fresh(now, MAX_CLEARANCE_AGE)` before the first byte, so
   "immediately" is enforced by a call and not by a comment.
2. `packer::build` over Phase 2's `SyncPlan`.
3. `pointer::load` — the current pointer and its `sha`, or `None` on first push.
4. `client.ensure_release`.
5. `upload::run` — resume scan, then the new packs, then verification.
6. **Re-gate.** `fetch_facts` + `assert_pushable` again. A public read here is an incident: delete
   the assets this run uploaded, refuse the flip, and return the error that names the credentials
   to rotate and states plainly that published bytes cannot be un-published. Do not flip. Do not
   prune.
7. `pointer::commit` — the compare-and-swap `PUT`. **This is the only commit point.** An
   interruption anywhere above leaves the remote exactly as it was: the packs are
   content-addressed, immutable, and referenced by nothing.
8. `prune::run` against the pointer that actually landed. Any error from it becomes
   `prune_warning`, never a returned `Err`.

**`push/pointer.rs`** — `pub async fn load(client, repo, expect_repo_id, now) -> Result<(Option<Pointer>, Option<String>)>`
routing through `write::get_contents`, probing `format` before deserializing (a newer pointer may
carry required fields this build has never heard of, and a full deserialize would complain about
a missing field instead of the real problem), checking it through `sync::check_version`, and
refusing a `repo_id` that is not the caller's own.

`pub async fn commit<F>(client, repo, current: Option<&Pointer>, sha: Option<&str>, rebuild: F, now) -> Result<(Pointer, String)>`
where `rebuild: Fn(Option<&Pointer>) -> Result<Pointer>` produces the pointer to write from
whatever the remote currently holds. Fill the no-conflict path here: call `rebuild`, serialize,
`write::put_contents` with the `sha`, return the new pointer and its new `sha`. Plan 4-04 owns
this file and fills the 409 arm — the closure exists from the tracer precisely so 4-04 adds the
re-read-and-rebuild retry without touching the orchestrator that supplies it.

**`push/packer.rs`**, **`push/upload.rs`**, **`push/prune.rs`**, **`push/rekey.rs`** — created
with their frozen signatures and a working minimum sufficient for this tracer:
`packer::build(ctx, plan) -> Result<PushBundle>` handling the straight-line case;
`upload::run(ctx, release_id, packs, progress) -> Result<(usize, usize, u64)>` uploading each pack
sequentially with no resume scan; `prune::run(ctx, release_id, pointer, keep) -> Result<usize>`
returning zero deletions; `rekey::run(...)` returning a not-yet-implemented error. Each file's
module doc names the plan that fills it. The gaps are functional; the architecture is whole.

**`push/progress.rs`** — `pub trait Progress { fn start(&mut self, assets: usize, total_bytes: u64); fn asset_done(&mut self, index: usize, name: &str, bytes: u64); fn finish(&mut self); }`
plus `pub struct Silent;` implementing it as no-ops, which is what every test passes. The
granularity is the asset, per D6 — there is no per-chunk hook and one must not be added. Plan
4-03 adds the terminal and non-terminal implementations behind this trait.

**`docs/sync-format.md`** — add a §10 "Remote layout" recording: the release tag and that there is
one release; the two asset-name shapes and that both are content addresses; the Contents-API
pointer path and that its `sha` precondition is the format's single linearization point; the
`Pointer` / `SnapshotRecord` / `RemoteIndexEntry` JSON with every field's type; the ordering of
`snapshots`; the bootstrap chain a reader walks (pointer → keyfile asset → newest sealed root →
`index_chunks` → the index object → manifest chunks → data chunks); why the container is
plaintext and what an attacker editing it can and cannot achieve; and that the pack-header
single-chunk ceiling is unchanged because `PACK_TARGET` is unchanged. Someone holding only this
page must be able to write a reader — that is the standard the rest of the document already
holds itself to.
  </action>
  <verify>
    <automated>cargo test --lib -- sync::push::pointer sync::push::mod</automated>
  </verify>
  <done>`src/sync/push/` holds six files plus `mod.rs`. Every cross-module type is declared in `push/mod.rs` and none in a sibling. `pointer::load` and `pointer::commit` work against mockito for the no-conflict path, sending no `sha` on a first push and the loaded `sha` afterwards. `docs/sync-format.md` carries a §10 from which a reader could be written. The crate builds with the four unfilled modules present.</done>
  <reversibility rating="one-way">The remote layout — asset names, the pointer path, and the pointer JSON — is what every future reader and every already-pushed bundle depends on. Changing it after a user has pushed once means their bundle is unreadable by the next build unless a migration is written. It is versioned (`POINTER_VERSION`) so it *can* evolve, but the v1 shape ships once.</reversibility>
  <precondition>Phase 1 and Phase 2 are merged: `sync::pack::{PackWriter, PACK_TARGET, should_seal}`, `sync::model::{Root, Manifest, IndexObject, IndexEntry}`, `sync::crypto::{Keys, ChunkId, content_address}`, and `sync::plan::SyncPlan` all exist with the shapes recorded in `2-05-SUMMARY.md`.</precondition>
</task>

<task type="auto" tdd="true">
  <name>Task 3: `ai-usagebar sync push` — one file to the remote, end to end</name>
  <files>src/config.rs, src/widget/cli.rs, src/sync/cli.rs</files>
  <behavior>
    - A `config.toml` with `[sync]` and no `keep_snapshots` loads with the value 10; one setting it to 3 round-trips; one setting it to 0 fails at load naming the key.
    - `run_with` on the push action, against a mockito private repo with a one-file seeded tree, exits 0 having issued exactly one asset upload and one contents `PUT`.
    - The same run against a mock whose second visibility read reports public exits non-zero, issues a `DELETE` for the asset it uploaded, and issues **no** contents `PUT`.
    - The same run against a mock that fails the pointer `PUT` leaves no second attempt and exits non-zero.
    - A run whose prune step fails exits **0** and prints a warning line naming what was not cleaned up.
    - The rendered output contains no substring of the token and no substring of the passphrase.
  </behavior>
  <action>
Add `pub keep_snapshots: u32` to `SyncConfig` in `src/config.rs`, defaulting to **10** per D1,
with a doc comment saying what it buys: old snapshots share chunks, so ten costs little more than
three and covers roughly a week of daily syncs. It is config and not a constant, deliberately.
Validate it at config load: zero would mean the flip that publishes a snapshot also drops it, so
refuse zero with a message naming the key. `SyncConfig` already carries `#[serde(default)]`, so
an existing config keeps loading.

In `src/widget/cli.rs`, extend `SyncAction`. Plan 2-07 introduces the push variant with its
dry-run flag — **read `2-07-SUMMARY.md` for its exact shape rather than guessing**; if 2-07 landed
it as a variant carrying `dry_run`, wire the false arm here and leave the true arm exactly as
2-07 wrote it. Add two new variants: one that prunes superseded remote data on demand, and one
that changes the sync password. Each gets a doc comment that reads as its help text, and the
password one says in that text that changing the password is not revocation.

In `src/sync/cli.rs`, extend `run_with` — plan 3-01's injected-seam entry — with arms for all
three. Each resolves the pairing record for the numeric repo id, builds the `PushCtx`, and
`block_on`s the async call on the local current-thread runtime 3-01 established. The push arm
prints `PushOutcome`: packs uploaded, packs skipped, bytes, snapshots kept, packs deleted, and
`prune_warning` on its own clearly-marked line when present. **A run whose only failure is the
prune step exits 0** — that is D2 in the exit code, and it is the one place where getting it
wrong is silent.

The error path prints Phase 3's `http::actionable` text and nothing else: no token, no prefix of
one, no header dump, no response body echoed unsanitized. Where a remote-supplied string does
reach the user, route it through the existing `sanitize_untrusted_field` path — a remote error
message is untrusted input exactly as a vendor API error already is.

Nothing here touches `widget::run::fallback`. Sync exits non-zero on failure by design; the
widget's exit-0 invariant is a property of the widget path, and routing push errors through it
would hide a failed backup behind a status icon.

Every test drives `run_with` with an injected `Config`, `SyncRoots`, `Endpoints`, `TokenChain`,
and a fixed `now`, against `mockito`. None calls `run`, `Config::load`, `SyncRoots::resolve`, or
`TokenChain::production` — the AUR `check()` runs `cargo test` on installers' machines.
  </action>
  <verify>
    <automated>cargo test --lib -- sync::cli sync::push config::tests</automated>
  </verify>
  <done>`ai-usagebar sync push` runs end to end against a mock private repo: one pack, one asset upload, one `sha`-preconditioned contents `PUT`, exit 0. The re-gate case deletes and refuses to flip. A prune failure exits 0 with a warning. `keep_snapshots` defaults to 10 and refuses 0. No test reads a real `$HOME` or a real token.</done>
  <precondition>Plan 2-07 is merged, so `SyncAction`'s push variant and `sync::report`'s final shape exist; and Phase 3 is fully merged, so `sync::cli::run_with`, `gate::assert_pushable`, `PushClearance::assert_fresh`, and `pairing::read_from` exist.</precondition>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|---|---|
| process → network | The first outbound data path in the project; credential-bearing ciphertext and a `Contents: write` bearer token cross here |
| bearer token → redirect target | A release-asset `GET` answers with a 302 to storage the project does not control |
| GitHub response → process | Attacker-controlled JSON, headers, and `Location` when the remote is hostile |
| remote pointer → process | The one mutable remote object; unauthenticated container, attacker-editable |
| process → remote delete | The first destructive remote operation in the project |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|---|---|---|---|---|---|
| T-4-01 | Information disclosure | bearer token replayed to signed storage | critical | mitigate | `same_origin_redirect_policy` refuses the hop; `download_asset` issues the second request from a separate client carrying no `Authorization` header, asserted by a test reading the mock's recorded headers |
| T-4-02 | Elevation of privilege | flip ordering | critical | mitigate | The pointer `PUT` is the last step and runs only after the re-gate returns a fresh `PushClearance`; a public re-read deletes this run's assets and returns before the `PUT` |
| T-4-03 | Spoofing | a clearance carried from `sync setup` | critical | mitigate | `push::run` calls `fetch_facts` + `assert_pushable` itself and `assert_fresh` before the first byte; `PushClearance` has a private field, no public constructor, and no `Clone`, so one cannot be stashed |
| T-4-04 | Tampering | the plaintext pointer container | high | accept | Every element of value inside is sealed; a dropped entry is a rollback caught by the local anchor's counter, a reordered list is inert because selection is by the sealed `counter`, and a fabricated entry fails its tag. Accepting this is what keeps the deferred AAD object-type separator untriggered |
| T-4-05 | Denial of service | oversized asset or pointer body | high | mitigate | `MAX_ASSET_BYTES` and `MAX_POINTER_BYTES`, checked against `Content-Length` before allocating **and** while reading, so a lying header cannot exhaust memory; `vendor::MAX_BODY_BYTES` is not reused |
| T-4-06 | Denial of service | unbounded asset pagination | medium | mitigate | `list_assets` stops at a page cap derived from the documented 1000-per-release ceiling and errors naming it, rather than following a hostile remote's pages forever |
| T-4-07 | Denial of service | retry storm against a rate limit | medium | mitigate | `with_retry` honours `Retry-After` and `x-ratelimit-reset` through Phase 3's `retry_delay`, caps attempts, and refuses to retry `Unauthorized`, `Forbidden`, `NotFound`, or `Conflict` (D7) |
| T-4-08 | Repudiation | a lost `PUT` response retried | medium | mitigate | A 409 is never retried inside `with_retry`; the compare-and-swap is re-driven only by `pointer::commit`'s own rebuild path (plan 4-04), so a stale write can never clobber a newer one |
| T-4-09 | Information disclosure | remote error text rendered to the terminal | medium | mitigate | Remote-supplied strings route through `sanitize_untrusted_field`; the CLI prints `http::actionable` and never a raw body or header map |
| T-4-10 | Information disclosure | token or passphrase in output | critical | mitigate | Nothing in `push/` or `write.rs` formats a header map or a key; a test asserts the rendered outcome contains no substring of either |
| T-4-11 | Tampering | an unreferenced-but-live pack deleted | critical | mitigate | Prune runs only after a successful flip and only against the pointer that **landed**; it is structurally handed the committed pointer rather than the one this run built |
| T-4-SC | Tampering | dependency surface | low | accept | Zero new crates and zero new features. `reqwest` 0.12, `serde_json`, `base64`, `chrono`, `zeroize`, `tokio` (`rt` supplies `JoinSet`), and `mockito` are all declared today. `Cargo.toml` is **not** in this plan's `files_modified`; if the executor believes it needs an edit there, that is a signal the design drifted, not a step to take |
</threat_model>

<verification>
- `cargo test --lib sync::github::write`, `cargo test --lib sync::push`, and
  `cargo test --lib config::tests` are green.
- `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` are clean.
- `cargo machete` reports nothing new; `Cargo.toml` is unchanged by this plan.
- Plan 3-01's `Client`-has-no-request-body guard still passes — the write verbs live in
  `write.rs`, and `github/mod.rs` gains only a module declaration.
- Every time-dependent function under `src/sync/push/` and in `github/write.rs` takes `now` as a
  parameter.
- No test sleeps, spawns a process, opens a socket outside the mockito base, or reads a real
  `$HOME`.
- Multi-filter test invocations use the `cargo test --lib -- a b` form.
</verification>

<success_criteria>
`ai-usagebar sync push` carries one seeded file to a mock private repository as one release
asset and publishes a `sha`-preconditioned pointer, with the visibility gate re-earned inside the
push and re-checked before the flip. An interruption before the `PUT` leaves the remote
unchanged. The remote layout is written down well enough for someone to implement a reader from
`docs/sync-format.md` alone.
</success_criteria>

<output>
Create `.planning/phases/04-push-packs-atomic-flip-gc-rekey/4-01-SUMMARY.md` when done.

Record the **exact** public signatures of every write verb and `Asset`, of `with_retry`, and of
`Pointer`, `SnapshotRecord`, `RemoteIndexEntry`, `BuiltPack`, `PushBundle`, `PushCtx`,
`PushOutcome`, `Progress`, `pointer::load`, `pointer::commit`, `packer::build`, `upload::run`,
`prune::run`, and `rekey::run` — five plans build against them in parallel worktrees and must not
re-derive them from the diff.

State explicitly, because two later plans depend on it: **prune is handed the pointer that
landed**, and **`prune_warning` is an `Option` on the outcome, never an `Err`**.

State whether `PACK_TARGET` was touched. It must not have been.
</output>
