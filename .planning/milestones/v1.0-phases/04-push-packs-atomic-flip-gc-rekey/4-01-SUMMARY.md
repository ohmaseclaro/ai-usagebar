---
phase: 04-push-packs-atomic-flip-gc-rekey
plan: 01
subsystem: transport
tags: [push, write-verbs, atomic-flip, compare-and-swap, prune-grace, tracer, frozen-seams, hermetic-tests]

requires:
  - phase: 03-github-auth-and-the-private-repo-gate
    plan: 01
    provides: "`Client`, `Endpoints` (both hosts), `RepoRef`, `GithubError`, `http::classify`"
  - phase: 03-github-auth-and-the-private-repo-gate
    plan: 03
    provides: "`http::retry_delay`, `http::is_retryable`, `actionable`'s seven arms"
  - phase: 03-github-auth-and-the-private-repo-gate
    plan: 08
    provides: "`gate::Pushing`, `PushClearance::spend`, `gate::FetchError` — the security remediation this plan was rebased onto mid-execution"
  - phase: 02-bundle-scope-local-index-dry-run-planning
    plan: 05
    provides: "`SyncPlan`, `FilePlan`, `plan::build_with_keys`"
  - phase: 01-encrypted-bundle-core
    provides: "`PackWriter`, `should_seal`, `PACK_MAX`, `Manifest`, `IndexObject`, `Root`, `Keys`, `content_address`"
provides:
  - "`src/sync/github/write.rs` — six write verbs, `Asset`, `with_retry`; the only file in the crate that sends a request body"
  - "`src/sync/push/` — seven files; every cross-module type declared in `mod.rs`"
  - "`ai-usagebar sync push` / `sync prune` / `sync rekey`"
  - "`[sync] keep_snapshots`, defaulting to 10, refusing 0 at config load"
  - "`docs/sync-format.md` §10 — the remote layout, complete enough to write a reader from"
  - "The rewritten write-path guard: bodies only in `write.rs`, and exactly two `reqwest::Client`s under `src/sync/`"
affects: [4-02, 4-03, 4-04, 4-05, 4-06, 4-07, phase-5-restore]

tech-stack:
  added: []
  patterns:
    - "A capability taken by reference where the caller writes many times under one check, by value where 'once' is the real semantics. `Pushing` is consumed at the push entry points and borrowed by the verbs."
    - "A guard test that assembles its own needles at runtime, so it can scan whole files instead of skipping the half it lives in."
    - "A frozen stub is wired from the tracer, not merely declared: `plan_deletions` is called by `prune::run`, which is called by `push::run`, which is called by the CLI."

key-files:
  created:
    - src/sync/github/write.rs
    - src/sync/push/mod.rs
    - src/sync/push/pointer.rs
    - src/sync/push/packer.rs
    - src/sync/push/upload.rs
    - src/sync/push/prune.rs
    - src/sync/push/rekey.rs
    - src/sync/push/progress.rs
  modified:
    - src/sync/github/mod.rs
    - src/sync/mod.rs
    - src/sync/cli.rs
    - src/widget/cli.rs
    - src/config.rs
    - src/sync/plan.rs
    - src/sync/report.rs
    - docs/sync-format.md
    - docs/sync-github.md

requirements-completed: [REPO-06, REPO-07, SYNC-04]

duration: 3h
completed: 2026-08-19
status: complete
---

# Phase 4 / Plan 01: The Push Tracer

**`ai-usagebar sync push` carries one seeded file to a mock private repository as
one release asset and publishes a `sha`-preconditioned pointer — with the
visibility gate re-earned inside the push and re-checked before the flip.** An
interruption anywhere before that flip leaves the remote pointer byte-identical
to what it was, and there are two tests that fail if a `PUT` is ever issued
early.

## Task commits

1. `f009372` — the six write verbs, D7's retry, and the guard's new claim
2. `868f573` — hardening the guard: runtime needles, whole-file scan, client count
3. `5649fb5` — merge of `milestone/encrypted-sync` (plan 3-08's security remediation)
4. `d1aeeda` — every write verb takes 3-08's `Pushing` capability
5. `dbfe128` — the remote layout, the pointer's compare-and-swap, `sync-format.md` §10
6. `2bf1e20` — `sync push` / `prune` / `rekey`, and `keep_snapshots`
7. `2d40b13` — the verification gate before the flip, and the user doc

## THE FROZEN SIGNATURES — copy these, do not re-derive them

Six plans build against these in parallel worktrees. Everything below is verbatim
from the merged code.

### `src/sync/github/write.rs`

```rust
pub const MAX_ASSET_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_POINTER_BYTES: u64 = 1024 * 1024;
pub const ASSET_STATE_UPLOADED: &str = "uploaded";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Asset {
    pub id: u64,
    pub name: String,
    pub size: u64,
    pub state: String,                    // String, not an enum — MEDIUM-confidence transitions
    pub created_at: DateTime<Utc>,        // prune's grace window is computed from this
    #[serde(default)]
    pub digest: Option<String>,           // GitHub does not always populate it
}

impl Client {
    pub async fn ensure_release(&self, repo: &RepoRef, tag: &str,
                                _permit: &Pushing, now: DateTime<Utc>) -> Result<u64>;

    pub async fn list_assets(&self, repo: &RepoRef, release_id: u64,
                             now: DateTime<Utc>) -> Result<Vec<Asset>>;

    pub async fn upload_asset(&self, repo: &RepoRef, release_id: u64, name: &str,
                              body: Vec<u8>, permit: &Pushing,
                              now: DateTime<Utc>) -> Result<Asset>;

    pub async fn delete_asset(&self, repo: &RepoRef, asset_id: u64,
                              _permit: &Pushing, now: DateTime<Utc>) -> Result<()>;

    pub async fn download_asset(&self, repo: &RepoRef, asset_id: u64,
                                now: DateTime<Utc>) -> Result<Vec<u8>>;

    pub async fn get_contents(&self, repo: &RepoRef, path: &str,
                              now: DateTime<Utc>) -> Result<Option<(String, Vec<u8>)>>;

    #[allow(clippy::too_many_arguments)]
    pub async fn put_contents(&self, repo: &RepoRef, path: &str, message: &str,
                              body: &[u8], sha: Option<&str>, _permit: &Pushing,
                              now: DateTime<Utc>) -> Result<String>;
}

type GhResult<T> = std::result::Result<T, GithubError>;   // private to the module

pub(crate) async fn with_retry<T, F, Fut, S, SFut>(
    attempts: u32,
    sleep: S,
    now: DateTime<Utc>,
    op: F,
) -> GhResult<T>
where
    F: Fn() -> Fut,
    Fut: Future<Output = GhResult<T>>,
    S: Fn(Duration) -> SFut,
    SFut: Future<Output = ()>;
```

`with_retry` retries `RateLimited` and `Transport` and **nothing else** — that is
`http::is_retryable`'s rule rather than a second one — waiting the delay a
`RateLimited` carries, or `http::retry_delay(&HeaderMap::new(), attempt, now)` for
a transport failure. `Unauthorized`, `Forbidden`, `NotFound` and `Conflict`
return on the first attempt, asserted with a mock expecting exactly one hit. On
exhaustion the last error is returned unchanged, so the caller still gets Phase
3's actionable text. Production passes `tokio::time::sleep`; **no test sleeps.**

### `src/sync/push/mod.rs` — the frozen remote layout

```rust
pub const POINTER_PATH: &str = "sync/pointer.json";
pub const RELEASE_TAG: &str = "ai-usagebar-sync-v1";
pub const POINTER_VERSION: u32 = 1;
pub const MAX_SUPPORTED_POINTER: u32 = 1;
pub const PRUNE_GRACE: chrono::TimeDelta = TimeDelta::hours(24);

pub fn pack_asset_name(id: &ChunkId) -> String;      // "pack-<64 hex>.bin"
pub fn keyfile_asset_name(id: &ChunkId) -> String;   // "keyfile-<64 hex>.json"
pub fn repo_id_for(pairing_repo_id: u64) -> String;  // "github:<id>"

#[derive(Debug, Clone)]
pub struct BuiltPack { pub id: ChunkId, pub bytes: Vec<u8> }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteIndexEntry {
    pub id: ChunkId, pub pack: ChunkId,
    pub offset: u64, pub clen: u32, pub true_len: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotRecord {
    pub root: String,                          // base64 of the sealed root
    pub index_chunks: Vec<RemoteIndexEntry>,
    pub packs: Vec<ChunkId>,                   // EVERY pack, reused ones included
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pointer {
    pub format: u32,
    pub repo_id: String,
    pub keyfile: String,
    pub snapshots: Vec<SnapshotRecord>,        // oldest first, newest last
}

#[derive(Debug)]
pub struct PushBundle {
    pub packs: Vec<BuiltPack>,
    pub root: Vec<u8>,
    pub index_chunks: Vec<RemoteIndexEntry>,
    pub referenced_packs: Vec<ChunkId>,
    pub counter: u64,
}

// No `Debug`: it holds `Keys`, and `Index` has none.
pub struct PushCtx<'a> {
    pub client: &'a Client,
    pub repo: &'a RepoRef,
    pub cfg: &'a SyncConfig,
    pub roots: &'a SyncRoots,
    pub keys: &'a Keys,
    pub kdf: KdfParams,                 // ADDED — 4-02 needs it for `Root::new`
    pub index: &'a Index,
    pub repo_id: String,
    pub keyfile_asset: String,
    pub previous: Option<Pointer>,      // ADDED — filled by `run`, not by the caller
    pub now: DateTime<Utc>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct PushOutcome {
    pub packs_uploaded: usize,
    pub packs_skipped: usize,
    pub bytes_uploaded: u64,
    pub snapshots_kept: usize,
    pub packs_deleted: usize,
    pub prune_warning: Option<String>,
}

pub async fn run(mut ctx: PushCtx<'_>, progress: &mut dyn Progress) -> Result<PushOutcome>;

pub(crate) async fn gate_now(ctx: &PushCtx<'_>, credentials_in_bundle: bool)
    -> Result<gate::Pushing>;
```

### The rest of `src/sync/push/`

```rust
// progress.rs
pub trait Progress {
    fn start(&mut self, assets: usize, total_bytes: u64);
    fn asset_done(&mut self, index: usize, name: &str, bytes: u64);
    fn finish(&mut self);
}
pub struct Silent;                                       // no-ops; what every test passes

// pointer.rs
pub async fn load(client: &Client, repo: &RepoRef, expect_repo_id: &str,
                  now: DateTime<Utc>) -> Result<(Option<Pointer>, Option<String>)>;

pub async fn commit<F>(client: &Client, repo: &RepoRef, current: Option<&Pointer>,
                       sha: Option<&str>, rebuild: F, permit: &gate::Pushing,
                       now: DateTime<Utc>) -> Result<(Pointer, String)>
where F: Fn(Option<&Pointer>) -> Result<Pointer>;

// packer.rs
pub fn build(ctx: &PushCtx<'_>, plan: &SyncPlan) -> Result<PushBundle>;
pub fn manifest_path(roots: &SyncRoots, path: &Path) -> Result<String>;

// upload.rs
pub async fn run(ctx: &PushCtx<'_>, release_id: u64, packs: &[BuiltPack],
                 permit: &gate::Pushing, progress: &mut dyn Progress)
    -> Result<(usize, usize, u64)>;                       // (uploaded, skipped, bytes)

// prune.rs
pub fn plan_deletions(pointer: &Pointer, assets: &[Asset], keep: usize,
                      now: DateTime<Utc>, grace: TimeDelta) -> (Pointer, Vec<u64>);

pub async fn run(ctx: &PushCtx<'_>, release_id: u64, landed: &Pointer,
                 keep: usize, permit: &gate::Pushing) -> Result<usize>;

pub async fn run_on_demand(ctx: &PushCtx<'_>, keep: usize) -> Result<usize>;

// rekey.rs
pub async fn run(ctx: &PushCtx<'_>, old_pw: &Zeroizing<String>,
                 new_pw: &Zeroizing<String>) -> Result<String>;
```

## The three things two later plans depend on, stated explicitly

1. **Prune is handed the pointer that *landed*** — `prune::run`'s `landed`
   parameter is whatever `pointer::commit` returned, never the pointer this run
   built. If another machine won the flip, `landed` is *its* pointer and its packs
   are consequently live.
2. **`prune_warning` is an `Option` on `PushOutcome`, never an `Err`.** D2 is
   encoded in the type so no later plan can accidentally make a prune failure
   fatal. `render_push` prints it as a `warning:` line and the process exits 0.
3. **Neither `PACK_TARGET` nor `PACK_MAX` was touched.** `pack::should_seal`
   compares against **`PACK_MAX` = 48 MiB** and never reads `PACK_TARGET`, so
   packs fill to 48 MiB and `PACK_TARGET` = 32 MiB is advisory. 4-02 and 4-07 must
   build their guards against `PACK_MAX`. `docs/sync-format.md` §4 was corrected
   to say this, and to say that raising `PACK_MAX` is what would break the
   single-chunk pack-header ceiling.

## Deviations from the plan

**1. [Forced, blocking] Rebased onto plan 3-08's security remediation mid-execution.**
The coordinator flagged it after task 1 was written. `assert_fresh` is gone;
`PushClearance::spend(self, now) -> Result<Pushing>` replaces it, and
`fetch_facts` now returns `Result<RepoFacts, gate::FetchError>`. Merged
`milestone/encrypted-sync` at `5649fb5` and adapted.

**2. [Contract interpretation] `Pushing` is taken by *reference* by the write
verbs, by value at the entry points.** 3-08 says "every write verb takes a
`Pushing` by value". `Pushing` is not `Clone`, so by-value at the verb level makes
a push that uploads *n* packs and then flips uncompilable. The capability is
therefore consumed where "once" is the real semantics — `push::run`,
`prune::run_on_demand` and `rekey::run` each mint their own and hold it for
exactly the span of writes it gates — and borrowed by the verbs. The security
property is unchanged: a `&Pushing` cannot exist without a `Pushing`, which cannot
exist without a fresh `spend`. **The flip gets a second permit**, minted by the
re-gate after the uploads, which is what makes D3 a type-level fact.

**3. [Signature change, recorded] Two frozen signatures gained a permit, two lost
`release_id`.**
- `upload::run` and `prune::run` gained `permit: &gate::Pushing`; `pointer::commit`
  gained one too. A function that writes now says so in its signature.
- `prune::run_on_demand(ctx, keep)` and `rekey::run(ctx, old_pw, new_pw)` **dropped
  `release_id`**. Obtaining one means calling `ensure_release`, which is itself a
  write and needs the permit these functions mint — a gate-first entry point cannot
  be handed a release id fetched before its own gate. Both call `ensure_release`
  themselves. **4-05 and 4-06: build against this summary, not against the plan
  text.**

**4. [Additive] `PushCtx` has eleven fields, not nine.**
- `kdf: KdfParams` — 4-02's step 4 builds `Root::new(…, the keyfile's KdfParams,
  …)` and `Keys` does not carry them. Without this, `packer.rs` would have to
  re-read the keyfile off disk.
- `previous: Option<Pointer>` — the snapshot counter is one above the newest
  published snapshot's, so the packer needs the pointer. **`run` takes `ctx` by
  value and fills this itself** after `pointer::load`; callers construct with
  `None`. This reorders the plan's steps 2 and 3 (load before pack), which D3 does
  not constrain — D3 is about upload → verify → flip.

**5. [Simplification] The tracer's packer uses one `PackWriter` throughout.**
Data chunks, then the manifest's blobs, then the index object's, sealing only when
`should_seal` fires. A one-file fixture therefore produces exactly one pack and
one asset, which is what the plan's own must-have requires. 4-02 restructures the
fill loop but **must preserve the order** — manifest packed before the index
object is built, or the root's `manifest_chunks` name ids the index object does not
describe.

**6. [Defect fixed at the source] `packer::manifest_path` renders the
root-prefixed relative encoding**, per `4-CONTEXT.md`'s appended defect note:
`config/…`, `desktop-data/…`, `desktop-profiles/…`, `claude-home/…`. A file under
none of the roots is an **error**, never a fallback to the absolute path. Two
tests assert no rendered path is absolute, contains `..`, or carries the temp
prefix. 4-02 owns finishing it; 5-01 verifies it.

**7. [Guard rewritten twice]** The first rewrite passed on a real violation
because it split each file at `#[cfg(test)]` — a marker that also occurs inside a
doc comment in `pairing.rs`, truncating that file's scanned region to its first 76
lines. See below.

**8. [Scope, additive] `Client` derives `Clone`** — 4-03 spawns one upload task
per pack, each with an owned client, and 4-03 does not own `github/mod.rs`.
`Client::authed` was extracted so the bearer token becomes a header in exactly one
place, and it takes `accept` as a parameter because
`RequestBuilder::header` *appends*: a caller adding `application/octet-stream`
afterwards would send two `Accept` headers.

**9. [Not delivered, and it is a real gap] No plan in Phase 4 uploads the keyfile
asset on a first push.** `Pointer.keyfile` is set from `ctx.keyfile_asset` — the
content address of the local keyfile — but only `rekey` (4-06) ever *uploads* one.
A first push therefore publishes a pointer naming an asset that does not exist,
and Phase 5's restore cannot bootstrap from it. Grepped every plan in the phase to
confirm: 4-02, 4-03, 4-04, 4-05 and 4-07 do not mention it. **4-02 or 4-03 should
upload the keyfile when the arriving pointer does not already name it.** It is one
`upload_asset` call, and it is the difference between a restorable bundle and an
unrestorable one.

## The guard, and why it was rewritten twice

Plan 3-01's guard read `include_str!("mod.rs")` and proved `Client` had no
body-carrying method. Rust lets a sibling module open an inherent `impl Client`,
so this plan gave `Client` six such methods next door and **3-01's guard would have
stayed green while the sentence it was named for stopped being true.**

The replacement, `every_request_body_in_this_directory_lives_in_write_rs`, walks
`src/sync/github/`, skips `write.rs`, and fails on any body call site elsewhere.
It took two attempts to make honest:

- **Attempt 1** spelled its needles as string literals, which forced it to skip
  each file's `#[cfg(test)]` half so it would not trip on itself — and the marker
  it split on also appears inside a doc comment in `pairing.rs`, so that file was
  scanned only to line 76. A negative control (`.post(` injected below that line)
  **passed**. That is the same class of failure as the plan's own warning about
  `http::actionable`.
- **Attempt 2** assembles the needles at runtime (`format!(".{verb}(")`), which
  removes the reason to skip anything, so nothing is skipped. Negative control:
  `.post(` in `pairing.rs`'s production half turns it red.

A second half closes the way around the first, which the 3-08 audit independently
asked for as F-7: `no_third_http_client_is_built_under_src_sync` asserts exactly
two `reqwest::Client` constructions under `src/sync/` — the authenticated one in
`github/mod.rs` (with the same-origin redirect policy) and the token-free storage
one in `write.rs`. A third would be a request path with neither property. Negative
control: a `reqwest::Client::new()` added to `gate.rs` turns it red.

Both negative controls were run by hand and reverted.

## Security properties, and how each is enforced

| Property | Enforcement |
|---|---|
| T-4-01 — the bearer token never follows the 302 to storage | `download_asset` reads `Location` off the 3xx and issues the second request from a separate client built with no `Authorization` at all. The test matches the storage mock on `authorization` being **Missing**; a replayed token would not match and would 501. |
| T-4-02 — the flip is last | `push::run`'s step 7 is the only `PUT`. Two tests assert `expect(0)` on it: one where the re-gate reads public, one where a pack fails verification. |
| T-4-03 — the clearance is re-earned | `gate_now` runs `fetch_facts` → `check_drift` → `assert_pushable` → `spend` inside the push, at step 1 and again at step 6. `sync setup` no longer hands one out at all. |
| T-4-05 — oversized bodies | `MAX_ASSET_BYTES` / `MAX_POINTER_BYTES` through `vendor::read_body_capped`, which checks `Content-Length` before allocating **and** while reading. `vendor::MAX_BODY_BYTES` is not reused for either. |
| T-4-06 — unbounded pagination | `list_assets` stops at 10 pages (the documented 1,000-asset ceiling) and errors naming it. Tested against a remote that never runs out of pages. |
| T-4-07 — retry storms | `with_retry`, tested for both the retryable arm and all four terminal ones. |
| T-4-08 — a lost `PUT` retried | `with_retry` never retries a `Conflict`; a 409 at the flip exits non-zero with exactly one attempt. |
| T-4-10 — token or passphrase in output | `render_push` is pure and `PushOutcome` has no field that could hold either; asserted against both values and their eight-character prefixes. |
| T-4-11 — a live pack deleted | Both halves present: `prune::run` takes the **landed** pointer, and `PRUNE_GRACE` = 24 h is passed into `plan_deletions` from the tracer, so the call site exists before 4-05 fills the rules. |
| SAFE-02 mid-push | The re-gate's incident path deletes only assets that (a) carry a name this run's bundle produced **and** (b) were created at or after this run's clock — a pack this run *skipped* belongs to an earlier run and may be live. |

**No new object is sealed under `chunk_key`**, so Phase 1's deferred AAD
object-type separator stays untriggered. The pointer container is plaintext, and
`push/mod.rs` and `docs/sync-format.md` §10 both record why that is safe and what
would trigger the separator.

## Verification

```
cargo test --lib                            1300 passed, 0 failed, 0 ignored   (baseline 1285)
cargo clippy --all-targets -- -D warnings   clean
cargo fmt --check                           clean
Cargo.toml / Cargo.lock                     unchanged — zero new crates, zero new features
```

- `grep -rn 'Utc::now' src/sync/push/ src/sync/github/write.rs` — **no hits.** Every
  time-dependent function takes `now`.
- No test under `src/sync/push/` or in `write.rs` reads `$HOME`, calls
  `TokenChain::production` / `Config::load` / `SyncRoots::resolve`, spawns a
  process, sleeps, or opens a socket outside an injected `Endpoints` base.
- Every keyfile in a test is created with `m_kib = 8`, never production KDF
  parameters — the AUR `check()` runs these on installers' machines.
- The REPO-03 guard still passes: none of `/user/repos`, `/orgs/`, `/generate`,
  `/forks` appears anywhere under `src/`, including in comments.
- Every frozen stub has a live call site reachable from the CLI:
  `push::run` ← `cli::push_with_parts`; `prune::run_on_demand` ← `cli::prune`;
  `rekey::run` ← `cli::rekey`; `packer::build`, `upload::run`, `pointer::load`,
  `pointer::commit`, `prune::run` ← `push::run`; `plan_deletions` ← `prune::run`.

## Not done here, deliberately

- **The keyfile is never uploaded** — see deviation 9. This is the one gap that
  blocks Phase 5, and it belongs to 4-02 or 4-03.
- `upload::run` uploads sequentially with no resume scan (4-03), `pointer::commit`
  has no 409 arm (4-04), `plan_deletions` returns no deletions (4-05), `rekey::run`
  returns a not-yet-implemented error after gating (4-06).
- `packer::build` does not consult the local `chunk` table — it has no writer yet
  (4-02), and the snapshot counter is derived from the pointer's snapshot count
  rather than by opening the newest root (4-02).
- The CLI passes `Silent` for progress; 4-03 adds the terminal and non-terminal
  reporters behind the same trait, and `upload::run` already calls every hook.
