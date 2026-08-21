---
phase: 05-pull-and-restore
plan: 01
subsystem: sync/restore
tags: [restore, path-traversal, rollback-anchor, tracer, frozen-signatures]
status: complete
requires:
  - "sync::push::packer::manifest_path (Phase 4) — the push-side spelling this reverses"
  - "sync::anchor::{read_from, accept, write_to} (Phase 1)"
  - "sync::github::Client::{get_json, list_assets, download_asset, get_contents} (Phases 3–4)"
  - "sync::push::pointer::load, Pointer, SnapshotRecord, RELEASE_TAG, pack_asset_name, keyfile_asset_name (Phase 4)"
provides:
  - "sync::restore::{RestoreOptions, RestoreCtx, Disposition, ItemPlan, RestorePlan, RestoreOutcome, Applied, BackupRecord, Resolved, PackSource}"
  - "sync::restore::run — the seven-step orchestrator"
  - "sync::restore::layout::{to_manifest_path, from_manifest_path, accept_for_write}"
  - "sync::restore::fetch::resolve — the authenticated read chain"
  - "sync::restore::{merge::plan, write::apply, backup::take, report::render_plan, report::render_outcome} — signatures frozen, tracer path filled"
affects:
  - "5-02 (fetch.rs), 5-03 (merge.rs), 5-04 (write.rs), 5-05 (backup.rs), 5-06 (report.rs), 5-07 (cli.rs)"
tech-stack:
  added: []
  patterns:
    - "one prefix table, both directions, guarded by a test that pins push and restore to the same four literals"
    - "hostile-input boundary refuses before any I/O and builds the result component-by-component onto the root"
    - "dry-run as the absence of a field rather than a checked flag"
key-files:
  created:
    - src/sync/restore/mod.rs
    - src/sync/restore/layout.rs
    - src/sync/restore/fetch.rs
    - src/sync/restore/merge.rs
    - src/sync/restore/write.rs
    - src/sync/restore/backup.rs
    - src/sync/restore/report.rs
  modified:
    - src/sync/mod.rs
decisions:
  - "PackSource::chunk takes &self, not &mut self — resolve() downloads eagerly, so a cache mutation is unnecessary and would have contradicted write::apply(&PackSource)"
  - "RestoreCtx carries repo_id and passphrase, which the plan's field list omitted and fetch cannot work without"
  - "the pointer's RemoteIndexEntry offsets are never used to slice; the pack's own sealed header is"
  - "a dry run downloads only the metadata packs, never a byte of file content"
metrics:
  duration: ~50 min
  completed: 2026-08-19
---

# Phase 5 Plan 01: Pull-and-restore tracer and frozen seams — Summary

The whole restore chain walked end to end against a mockito remote on the phase's
first commit — pointer → keyfile → root → index → manifest → pack → chunk → one
file on disk at mode 0600 — with every cross-module type frozen in
`restore/mod.rs` so five wave-2 plans can fill five sibling modules in parallel.

---

## THE FROZEN SIGNATURES — copy these, do not re-derive them

Five plans build against these in parallel worktrees. Everything below is
**verbatim from the merged code**.

### `src/sync/restore/mod.rs` — every cross-module type, and none in a sibling

```rust
pub mod backup;
pub mod fetch;
pub mod layout;
pub mod merge;
pub mod report;
pub mod write;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RestoreOptions {
    pub apply: bool,               // the ONLY field that lets a byte reach disk
    pub force: bool,
    pub force_credentials: bool,   // `force` alone never grants this
    pub allow_rollback: bool,
    pub rebuild_index: bool,
    pub force_rehash: bool,
    pub assume_yes: bool,
}
// Dry-run is the *absence* of `apply`, not a flag anything checks. Default = all false.

// No `Debug`: it holds the passphrase.
pub struct RestoreCtx<'a> {
    pub client: &'a Client,
    pub repo: &'a RepoRef,
    pub roots: &'a SyncRoots,
    pub repo_id: &'a str,                    // ADDED vs the plan — see Deviations
    pub passphrase: &'a Zeroizing<String>,   // ADDED vs the plan — see Deviations
    pub anchor_path: &'a Path,
    pub backups_dir: &'a Path,
    pub opts: RestoreOptions,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Disposition {
    Create,
    Update,
    SkipIdentical,
    SkipLocalNewer { local_mtime: DateTime<Utc>, remote_mtime: DateTime<Utc> },
    Overwrite { local_mtime: DateTime<Utc>, remote_mtime: DateTime<Utc> },
    NeedsCredentialConfirm { local_mtime: DateTime<Utc>, remote_mtime: DateTime<Utc> },
    ExcludedByPolicy,
    RejectedPath(String),
}

impl Disposition {
    /// Create | Update | Overwrite. Everything else is false.
    pub fn writes(&self) -> bool;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemPlan {
    pub manifest_path: String,
    pub dest: Option<PathBuf>,      // None for RejectedPath / ExcludedByPolicy
    pub category: SyncCategory,
    pub true_len: u64,
    pub chunks: Vec<ChunkId>,
    pub disposition: Disposition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestorePlan {
    pub items: Vec<ItemPlan>,
    pub counter: u64,                 // the ROOT's sealed counter, never the pointer's
    pub created_at: DateTime<Utc>,    // the remote mtime for every item in the snapshot
    pub repo_id: String,
    pub packs_needed: usize,
    pub bytes_to_fetch: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Applied {
    pub written: usize,
    pub overwritten: Vec<String>,
    pub skipped: usize,
    pub failed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupRecord {
    pub archive: PathBuf,
    pub root: PathBuf,       // what the archive's members are relative to
    pub members: usize,
    pub bytes: u64,
}

impl BackupRecord {
    pub fn rollback_command(&self) -> String;   // "tar -xzf <archive> -C <root>"
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreOutcome {
    pub plan: RestorePlan,
    pub applied: bool,
    pub backup: Option<BackupRecord>,
    pub written: usize,
    pub overwritten: Vec<String>,
    pub skipped: usize,
    pub failed_at: Option<String>,
}

pub struct Resolved {
    pub root: Root,
    pub manifest: Manifest,
    pub index: IndexObject,
    pub packs: PackSource,
}

// No `Debug`. Holds `Keys`.
pub struct PackSource { /* private */ }

impl PackSource {
    pub fn empty(keys: Keys) -> Self;
    /// Verifies content_address(bytes) == id, then reads the pack's SEALED header.
    pub fn add(&mut self, id: ChunkId, bytes: Vec<u8>) -> Result<()>;
    pub fn holds_pack(&self, id: &ChunkId) -> bool;
    pub fn keys(&self) -> &Keys;
    pub fn packs(&self) -> usize;
    pub fn bytes(&self) -> u64;
    /// Ciphertext — what Manifest::open / IndexObject::open take.
    pub fn sealed(&self, id: &ChunkId) -> Result<Vec<u8>>;
    /// Plaintext — what write::apply concatenates.
    pub fn chunk(&self, id: &ChunkId) -> Result<Zeroizing<Vec<u8>>>;
}

pub async fn run(ctx: RestoreCtx<'_>) -> Result<RestoreOutcome>;
```

### The six modules

```rust
// layout.rs — owned outright by 5-01; nobody else restates a path rule
pub fn to_manifest_path(roots: &SyncRoots, abs: &Path) -> Result<String>;
pub fn from_manifest_path(roots: &SyncRoots, s: &str) -> Result<PathBuf>;
pub fn accept_for_write(rel: &Path) -> bool;   // pass the MANIFEST path, not the resolved dest

// fetch.rs — 5-02
pub async fn resolve(ctx: &RestoreCtx<'_>, local_anchor: Option<&Anchor>) -> Result<Resolved>;

// merge.rs — 5-03
pub fn plan(ctx: &RestoreCtx<'_>, resolved: &Resolved) -> Result<RestorePlan>;

// write.rs — 5-04
pub fn apply(ctx: &RestoreCtx<'_>, plan: &RestorePlan, packs: &PackSource) -> Result<Applied>;

// backup.rs — 5-05
pub fn take(ctx: &RestoreCtx<'_>, targets: &[PathBuf]) -> Result<Option<BackupRecord>>;

// report.rs — 5-06
pub fn render_plan(plan: &RestorePlan) -> String;
pub fn render_outcome(outcome: &RestoreOutcome) -> String;
```

### The manifest path encoding, frozen

Four prefixes, one `/` separator on every platform, remainder relative:

| prefix | `SyncRoots` field |
|---|---|
| `config` | `config_dir` (`config_file` needs no fifth prefix — it is its child) |
| `desktop-data` | `desktop_data_dir` |
| `desktop-profiles` | `desktop_profiles_dir` |
| `claude-home` | `claude_home` |

Longest matching root wins. `config/accounts/work/.credentials.json` is a real
example. **No absolute path, no username, ever.**

### The seven steps of `run`, in the order that is a security property

1. `anchor::read_from(ctx.anchor_path)` — a parse failure is an error, never `None`.
   The path is `ctx.anchor_path` and is **never** derived from the remote's claimed `repo_id`.
2. `fetch::resolve` — calls `anchor::accept` itself, against the **root's sealed** counter and `repo_id`.
3. `merge::plan`.
4. `if !opts.apply` → return. **The write path is not reachable past this line.**
5. `backup::take`, over only the destinations whose `Disposition::writes()`.
6. `write::apply`.
7. `anchor::write_to` — only now, only from `resolved.root.counter`, and only when
   `applied.failed_at.is_none()`.

---

## What was built

- **`layout.rs`** — the wire spelling in both directions plus the D4 write-side
  policy. `to_manifest_path` delegates to `push::packer::manifest_path` (Phase 4
  already emits the relocatable form, so the plan's packer edit was already
  done); a test pins the two to the same four literals so they cannot drift.
  `from_manifest_path` refuses eleven distinct hostile spellings before touching
  the filesystem and builds the result **one component at a time** onto the root.
  No `canonicalize` anywhere.
- **`mod.rs`** — the ten frozen types and the seven-step orchestrator.
- **`fetch.rs`** — the authenticated chain. Reuses `push::pointer::load` (version
  probe + `repo_id` refusal already live there). Finds the release with the
  read-only `Client::get_json`, deliberately *not* `write::ensure_release`, whose
  404 arm creates. One `list_assets` for the whole restore. Every remote-chosen
  list is bounded before it is walked. A pack is refused unless its bytes hash to
  the content-addressed asset name it was served under, and its offsets come from
  its own sealed header — the pointer's unauthenticated `offset`/`clen`/`true_len`
  never index into anything.
- **`merge.rs` / `write.rs` / `backup.rs` / `report.rs`** — created with the frozen
  signatures and the tracer's one-file path filled, each with a module comment
  naming the plan that owns the rest.

## Deviations from Plan

### Scope narrowed by the orchestrator

The orchestrator scoped this run to `src/sync/mod.rs` and `src/sync/restore/{mod,layout,fetch}.rs`.
Two of the plan's items therefore did **not** ship and are noted for their owners:

1. **Task 3 (`sync pull` on the CLI) was not done.** `src/sync/cli.rs` and
   `src/widget/cli.rs` are 5-07's files. The tracer's end-to-end proof lives at
   the `restore::run` level instead — same chain, same mockito remote, same
   assertions — so nothing about the command's shape is unproven except the clap
   wiring. **5-07 still owns the full flag set**, and `RestoreOptions`' seven
   fields are the spelling to map onto: `--apply`, `--force`, `--force-credentials`,
   `--allow-rollback`, `--rebuild-index`, `--force-rehash`, `--yes`, plus a
   `--dry-run` that conflicts with `--apply`.
2. **`docs/sync-format.md` §11 was not written.** That file is 5-08's. The reader
   chain and the path encoding are specified above and in `layout.rs`'s and
   `fetch.rs`'s module docs; §11 should be lifted from them.

### [Rule 3 — blocking] `merge.rs`, `write.rs`, `backup.rs` and `report.rs` were created here

`mod.rs` declares six submodules; without the files the crate does not compile,
so `cargo test` could not have run. They are created with their frozen signatures
and the tracer's path. Their owners (5-03…5-06) are in wave 2 and branch off this
merge, so each modifies a file that already exists — no conflict.

### [Rule 2 — missing critical functionality] `RestoreCtx` gained `repo_id` and `passphrase`

The plan's field list had neither. `pointer::load` needs `expect_repo_id` and
`Root::open` needs it as associated data; the keyfile asset is the *only* route to
the master key on a second machine and needs the passphrase. Without both,
`fetch::resolve` cannot be written at all.

### [Rule 1 — correctness] `PackSource::chunk` takes `&self`, not `&mut self`

The plan specified `chunk(&mut self)` *and* `write::apply(…, packs: &PackSource)`,
which cannot both hold. Resolved in favour of `&self`: `resolve` downloads eagerly
(metadata packs always, data packs only under `apply`, so a dry run never fetches
a byte of file content), which makes a mutable cache unnecessary and lets the
accessors stay synchronous. `PackSource::empty` + `add` replace the plan's
implied `new`, because `Keys` is not `Clone` and the download happens in rounds.

### [Rule 2] `backup::take` refuses rather than returning `None` for an unarchived tree

The stub returns `Ok(None)` only when nothing at the destinations exists. When a
restore *would* overwrite something, it errors instead of proceeding without an
undo. A `None` for a tree it simply did not archive would have been the one
genuinely dangerous stub in the set. **5-05 replaces the error arm with the
archive**; it must not relax the "nothing archived ⇒ nothing overwritten" rule.

## Known Stubs

| File | What is stubbed | Owner |
|---|---|---|
| `merge.rs` | `SkipIdentical`, `SkipLocalNewer`, `Overwrite`, `NeedsCredentialConfirm` — an existing destination is `Update` for now | 5-03 |
| `write.rs` | directory modes, failure-path cleanup, the mtime stamp, symlink refusal | 5-04 |
| `backup.rs` | the archive itself; currently refuses any restore that would overwrite | 5-05 |
| `report.rs` | grouped table, line budget, the two interactive gates | 5-06 |

None of these prevents the tracer's goal — one file, end to end, dry-run then
applied — and each is the named deliverable of a wave-2 plan.

## Verification

```
cargo test                                  1388 lib passed, 0 failed  (baseline 1365, +23)
                                            1429 total passed, 0 failed (baseline 1406, +23)
cargo clippy --all-targets -- -D warnings   clean
cargo fmt --check                           clean
HOME= cargo test --lib sync::restore        23 passed, 0 failed
grep -rn canonicalize src/sync/restore/     doc comment only, no call
grep -rn 'temp_dir|"/tmp"' src/sync/restore/  none
grep -rn reqwest src/sync/restore/          none
git diff Cargo.toml Cargo.lock              empty — no new crates
```

`cargo machete` is not installed on this machine; `Cargo.toml` is byte-identical,
so no dependency could have become unused.

## Self-Check: PASSED

- `src/sync/restore/{mod,layout,fetch,merge,write,backup,report}.rs` — all present.
- `src/sync/mod.rs` — modified, `pub mod restore;` at line 51.
- Commits `38b2c49`, `97cf27a` — both in `git log` on `gsd/5-01`.
