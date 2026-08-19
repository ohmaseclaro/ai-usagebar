---
phase: 05-pull-and-restore
plan: 01
type: execute
wave: 1
depends_on: []
files_modified:
  - src/sync/mod.rs
  - src/sync/restore/mod.rs
  - src/sync/restore/layout.rs
  - src/sync/restore/fetch.rs
  - src/sync/restore/merge.rs
  - src/sync/restore/write.rs
  - src/sync/restore/backup.rs
  - src/sync/restore/report.rs
  - src/sync/push/packer.rs
  - src/sync/cli.rs
  - src/widget/cli.rs
  - docs/sync-format.md
autonomous: true
requirements: [SAFE-05, UX-01]
must_haves:
  truths:
    - "`ai-usagebar sync pull` against a mockito server walks pointer → keyfile → root → manifest → pack → chunk and reports one file it would create, writing **nothing** (D1, UX-01)."
    - "The same command with `--apply` writes that one file into an injected root at mode 0600, through a tempfile created in the destination's own directory (SAFE-05)."
    - "A manifest path that is absolute, contains `..`, or names an unknown root prefix is rejected before any I/O, and the rejection is reported rather than silently dropped (D5)."
    - "A manifest path whose components `scope::is_excluded` refuses is never written, whatever the bundle says (D4)."
    - "Manifest paths are **root-relative with a root prefix**, so a bundle pushed from one machine resolves on a second machine with a different `$HOME` and a different username."
    - "`restore::run` in dry-run mode is the default; the write path is reachable only through an explicit `apply` in `RestoreOptions` (D1)."
    - "The stored rollback anchor is advanced only after the snapshot has verified — never before the fetch (Phase 1 risk carried from plan 1-05)."
    - "No test in the module opens a socket to a host that is not an injected `Endpoints` base, spawns a process, or reads a real `$HOME`."
  artifacts:
    - src/sync/restore/mod.rs — the frozen cross-module types (`RestoreOptions`, `RestoreCtx`, `ItemPlan`, `Disposition`, `RestorePlan`, `RestoreOutcome`) and the orchestrator
    - src/sync/restore/layout.rs — the relocatable manifest-path mapping, filled here because four wave-2 plans call it
    - src/sync/restore/{fetch,merge,write,backup,report}.rs — created with the frozen signatures and the tracer's one-file path filled
    - "`SyncAction::Pull` on the CLI with its full flag set, frozen so plan 5-07 wires behaviour without redeclaring a flag"
    - docs/sync-format.md §11 — the reader chain a second machine walks, and the manifest path encoding
  key_links:
    - "`layout.rs` is filled, not stubbed: 5-02, 5-03 and 5-04 all call `from_manifest_path` and are in the same wave, so they cannot edit each other's files"
    - "`src/sync/push/packer.rs` must emit `layout::to_manifest_path`, not `FilePlan.path` — an absolute local path in the manifest is unrestorable on a second machine *and* is exactly what D5 rejects"
    - "the anchor is read before the fetch and written after it verifies; the two are in one function so the order cannot drift"
    - "`RestoreOptions` carries every flag 5-07 will wire, so no wave-2 plan has to guess a spelling"
---

<objective>
The tracer for Phase 5: one file, end to end. Fetch the pointer, open the keyfile, open the
snapshot root, reassemble the manifest, download the one pack it needs, open the one chunk, decide
that the file would be created, print that — and write nothing. Then the same run with `--apply`
writes it at mode 0600 through a tempfile in its own destination directory.

Purpose: prove the whole restore path against a mockito remote on the first commit of the phase,
and freeze every cross-module type before five parallel plans build against it. It also fixes the
one thing that would make restore impossible on a second machine: the manifest currently carries
whatever `FilePlan.path` held, which is an **absolute local path**.

Output: `src/sync/restore/` with seven files, `layout.rs` complete, `sync pull` reachable, and
`docs/sync-format.md` §11 describing the chain a reader walks.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/05-pull-and-restore/5-CONTEXT.md
@.planning/phases/04-push-packs-atomic-flip-gc-rekey/4-01-SUMMARY.md
@CLAUDE.md
@docs/sync-format.md
@src/sync/mod.rs
@src/sync/model.rs
@src/sync/chunk.rs
@src/sync/pack.rs
@src/sync/anchor.rs
@src/sync/scope.rs
@src/sync/cli.rs
@src/sync/push/mod.rs
@src/sync/push/pointer.rs
@src/sync/push/packer.rs
@src/sync/github/write.rs
</context>

<source_audit>

| Source | Item | Covered by |
|---|---|---|
| GOAL | A second machine reproduces the user's state from the remote | 5-01 (one file end to end), 5-02 (the full chain), 5-07 (the command) |
| GOAL | A restore that would clobber something newer says so first | 5-03 (dispositions), 5-06 (the report) |
| GOAL | …backs it up, and can be undone | 5-05 |
| REQ | SAFE-03 newer local credentials are never silently overwritten | 5-03, 5-06, 5-07 |
| REQ | SAFE-04 backup before the first write, rollback command printed | 5-05 |
| REQ | SAFE-05 no plaintext at a temp path that outlives the operation | **5-01** (the tracer's write goes through the dest dir), 5-04 (the full write path) |
| REQ | SYNC-06 last-write-wins per item, the user is told what was overwritten | 5-03, 5-06 |
| REQ | UX-01 push and pull, both with `--dry-run` | **5-01** (`sync pull` exists, dry-run is the default), 5-07 (the full flag set) |
| RESEARCH | Whole-pack fetch; `Range:` only if CAL-1 said yes | 5-02 — CAL-1 was **not** run in Phase 3 (plan 3-06), so whole packs it is |
| RESEARCH | `download_asset` caps at 64 MiB, above `pack::PACK_MAX` of 48 MiB | 5-02 — no new streaming verb is needed |
| CONTEXT | D1 dry-run by default | **5-01** (the default is structural, not a flag check), 5-06, 5-07 |
| CONTEXT | D2 a locally-newer item is never silently overwritten | 5-03, 5-07 |
| CONTEXT | D3 backup before the first write, in `~/.claude-acc/backups/` | 5-05 |
| CONTEXT | D4 never restore machine-bound or volatile state, enforced on the write side | **5-01** (`layout::accept_for_write` reuses `scope::is_excluded`), 5-04 |
| CONTEXT | D5 path traversal is hostile input | **5-01** (`layout.rs`), 5-04 (re-checked at the write boundary) |
| CONTEXT | D6 conflict reporting is a report, not a prompt | 5-06 |
| CONTEXT | D7 idempotent and resumable | 5-03 (digest skip), 5-08 (asserted twice-applied) |
| CONTEXT | anchor advances only after the snapshot verifies | **5-01** (both halves in one function), 5-08 (asserted on a failed verify) |
| CONTEXT | `repo_id` mismatch errors even under `allow_rollback` | already true in `anchor::accept`; 5-08 asserts the caller does not route around it |

**One reconciliation, and it is load-bearing.** Phase 4 plan 4-02 builds the manifest from
`plan.file_plans` — "path, mode, `true_len`, and the ordered chunk id list, all of which
`FilePlan` already carries". `FilePlan.path` is a `PathBuf` produced by `scope::collect` walking
the **injected roots**, so in production it is an absolute path containing the pusher's username.
Written into the manifest verbatim it is (a) unresolvable on a second machine and (b) precisely
the absolute path D5 orders restore to reject — a bundle that can only be restored by disabling
its own traversal defence. This plan owns the fix: `layout::to_manifest_path` on the push side,
`layout::from_manifest_path` on the restore side, one module, both directions, tested as a
bijection. The edit to `src/sync/push/packer.rs` is one expression.
</source_audit>

<tasks>

<task type="tracer" tdd="true">
  <name>Task 1: Relocatable manifest paths, in both directions, with traversal treated as hostile</name>
  <files>src/sync/restore/layout.rs, src/sync/restore/mod.rs, src/sync/mod.rs, src/sync/push/packer.rs</files>
  <behavior>
    - A path under each of the four roots maps to a prefixed relative string, and mapping it back under a *different* `SyncRoots` yields the corresponding path under the second machine's root.
    - `to_manifest_path` on a path under no root is an error naming the path, not a silent skip.
    - Two roots where one is a prefix of the other resolve to the longer one; a test constructs that nesting deliberately.
    - `from_manifest_path` rejects: an absolute string, a string containing a `..` component, a string with a leading `/` or a Windows drive prefix, a bare unknown prefix, an empty remainder, and a string whose resolved parent escapes its root.
    - `from_manifest_path` rejects a string containing a NUL byte or a path separator that is not `/`, so the wire spelling is one spelling.
    - `accept_for_write` refuses any resolved path that `scope::is_excluded` refuses, and refuses one whose components include an excluded directory name, so a bundle naming `local-agent-mode-sessions/x` is dropped whatever its prefix (D4).
    - The mapping is a bijection over a table of realistic paths from all five categories: `to_manifest_path` then `from_manifest_path` under the same roots is the identity.
  </behavior>
  <action>
Add `pub mod restore;` to `src/sync/mod.rs`. Create `src/sync/restore/` with `mod.rs` declaring
six submodules — `layout`, `fetch`, `merge`, `write`, `backup`, `report` — and create all six
files. Only `layout.rs` is filled in this task; the other five are created in task 2.

`layout.rs` owns the wire spelling of a path inside the bundle, and its module doc says that
plainly: every string in a manifest is untrusted input from a remote an attacker may control, and
this module is the only place that turns one into a `PathBuf`.

The encoding is **root-prefixed and relative**. Four prefixes, one per `SyncRoots` field that is a
directory — the literals `config`, `desktop-data`, `desktop-profiles`, and `claude-home` — mapping
to `config_dir`, `desktop_data_dir`, `desktop_profiles_dir`, and `claude_home`. `config_file`
needs no fifth prefix because `SyncRoots::resolve` derives `config_dir` as its parent. Keep the
prefix table as one `const` slice of `(&str, fn(&SyncRoots) -> &Path)` pairs so both directions
read the same table; a second table is a second thing to get wrong. Separator is `/`, always,
including on Windows: the bundle is portable or it is nothing.

`pub fn to_manifest_path(roots: &SyncRoots, abs: &Path) -> Result<String>` picks the **longest**
matching root — the roots can nest on a customised install and the shortest match would silently
file a path under the wrong tree — renders the prefix, a `/`, and the remainder with `/`
separators. A path under none of the four is an error naming the path.

`pub fn from_manifest_path(roots: &SyncRoots, s: &str) -> Result<PathBuf>` is the hostile-input
boundary. Reject before touching the filesystem, each with its own message: an empty string, a NUL
byte, a leading `/`, a backslash anywhere, a `..` component, a `.` component, an unknown prefix,
an empty remainder. Build the result by joining components one at a time onto the root — never by
`Path::join` on the whole remainder, because a single absolute component would replace the root
wholesale, which is the classic form of this bug. Then assert the result still `starts_with` its
root. Do **not** call `canonicalize`: the destination may not exist yet, and resolving symlinks
that the bundle can influence is how the escape sneaks back in. Symlinks are handled at the write
boundary instead, and plan 5-04 owns that.

`pub fn accept_for_write(rel: &Path) -> bool` is D4 on the write side: return false when any
component is an excluded directory name and when `scope::is_excluded` refuses the path. Reuse
`scope::is_excluded` rather than restating its lists — the whole point of D4 is that the two sides
agree, and two copies of a list diverge. If `scope`'s exclusion consts are private, make the
predicate `pub(crate)` in `scope.rs` rather than copying it, and say why in a comment.

In `src/sync/push/packer.rs`, replace the expression that fills `model::FileEntry.path` with a
`layout::to_manifest_path(ctx.roots, &fp.path)` call, propagating its error. That is the only edit
this plan makes to Phase 4's file. Leave `mode` and `true_len` as they are.
  </action>
  <verify>
    <automated>cargo test --lib -- sync::restore::layout sync::push::packer</automated>
  </verify>
  <done>`cargo test --lib -- sync::restore::layout sync::push::packer` is green. `src/sync/restore/` holds seven files. The prefix table appears exactly once. `from_manifest_path` refuses all eight rejection cases with distinct messages, and the bijection test covers all five categories. `packer.rs` writes prefixed relative paths and a pushed manifest contains no absolute path, asserted in `packer.rs`'s own tests.</done>
  <reversibility rating="costly">The manifest path encoding is on the wire, and after this ships every future build must read what it shipped. It is *not* a one-way door only because the recovery is a re-push: a bundle pushed between Phase 4 and this change carries absolute paths, and the honest fix for that user is one `sync push`, which is the cheap direction of this milestone's asymmetry. The v1 spelling still ships once, inside `MANIFEST_VERSION`, and is not to be improvised.</reversibility>
</task>

<task type="auto">
  <name>Task 2: The frozen restore types, and the orchestrator that is dry-run by construction</name>
  <files>src/sync/restore/mod.rs, src/sync/restore/fetch.rs, src/sync/restore/merge.rs, src/sync/restore/write.rs, src/sync/restore/backup.rs, src/sync/restore/report.rs, docs/sync-format.md</files>
  <behavior>
    - `RestorePlan` round-trips through the renderer with every disposition variant present, so 5-06 has a fixture that already covers its whole surface.
    - `restore::run` with `RestoreOptions::default()` returns a plan and calls neither the backup nor the write module, asserted with a recorder rather than by inspecting a temp dir.
    - `restore::run` with `apply` set calls the backup module exactly once and *before* the first write call, asserted by the recorder's call order.
    - `restore::run` reads the anchor before the fetch and writes it only after the fetch returns `Ok`; a fetch that errors leaves the anchor file byte-identical.
    - An `allow_rollback` run whose remote `repo_id` differs from the anchor's still errors — the escape hatch is not routed around.
  </behavior>
  <action>
Fill `src/sync/restore/mod.rs` with every type the other five modules exchange, and declare none
of them in a sibling. This is the same discipline 4-01 used and it is what lets five wave-2 plans
compile in isolation.

`RestoreOptions { pub apply: bool, pub force: bool, pub force_credentials: bool, pub allow_rollback: bool, pub rebuild_index: bool, pub force_rehash: bool, pub assume_yes: bool }`
with a `Default` where every field is false. Dry-run is therefore the **absence** of `apply`, not a
flag the code has to remember to check — D1 expressed in the type. Say that in the doc comment.

`RestoreCtx<'a>` carrying `client: &'a github::Client`, `repo: &'a RepoRef`, `roots: &'a SyncRoots`,
`anchor_path: &'a Path`, `backups_dir: &'a Path`, `opts: RestoreOptions`, `now: DateTime<Utc>`.
Every path is injected; nothing in the module resolves `$HOME`.

`Disposition` as an enum with a payload where the reason needs one: `Create`, `Update`,
`SkipIdentical`, `SkipLocalNewer { local_mtime, remote_mtime }`, `Overwrite { local_mtime,
remote_mtime }`, `NeedsCredentialConfirm { local_mtime, remote_mtime }`, `ExcludedByPolicy`,
`RejectedPath(String)`. `SkipLocalNewer` and `Overwrite` are the same fact under different
options; keeping them distinct is what lets the report name exactly what was lost (SYNC-06).

`ItemPlan { pub manifest_path: String, pub dest: Option<PathBuf>, pub category: SyncCategory, pub true_len: u64, pub chunks: Vec<ChunkId>, pub disposition: Disposition }`.
`dest` is `Option` because a rejected or excluded path deliberately has none — a rejected entry
still appears in the report, which is how a tampered bundle becomes visible instead of invisible.

`RestorePlan { pub items: Vec<ItemPlan>, pub counter: u64, pub created_at: DateTime<Utc>, pub repo_id: String, pub packs_needed: usize, pub bytes_to_fetch: u64 }`
and `RestoreOutcome { pub plan: RestorePlan, pub applied: bool, pub backup: Option<BackupRecord>, pub written: usize, pub overwritten: Vec<String>, pub skipped: usize, pub failed_at: Option<String> }`.

Two more, declared here because `run` returns through them and two wave-2 plans fill them:
`Applied { pub written: usize, pub overwritten: Vec<String>, pub skipped: usize, pub failed_at: Option<String> }`
(5-04) and `BackupRecord { pub archive: PathBuf, pub root: PathBuf, pub members: usize, pub bytes: u64 }`
with `pub fn rollback_command(&self) -> String` (5-05).

`pub async fn run(ctx: RestoreCtx<'_>) -> Result<RestoreOutcome>` is the orchestrator, and its
order is a security property, spelled out as numbered steps in the doc comment:

1. `anchor::read_from(ctx.anchor_path)` — the local high-water mark. A parse failure is an error,
   never `None`; that is already `anchor`'s rule and must not be softened here.
2. `fetch::resolve` — pointer, keyfile, root, manifest, index. It is handed the anchor and calls
   `anchor::accept` itself, because accept must run against the root's own sealed `counter` and
   `repo_id`, not against the plaintext pointer.
3. `merge::plan` — every manifest entry to an `ItemPlan`.
4. If `!opts.apply`, return the outcome now with `applied: false`. **The write path is not
   reachable from here.**
5. `backup::take` — before the first byte, even under `force`, even for a partial restore.
6. `write::apply` — item by item.
7. `anchor::write_to` — **only now**, and only if steps 2 through 6 all returned `Ok`. Advancing
   before verification lets a forged high counter lock the user out of their own bundle for good;
   that is a denial of service an attacker with repo write access can trigger at will, and it is
   the risk plan 1-05 recorded against this phase. Write the anchor from the **root's** sealed
   `counter`, never the pointer's.

`Resolved { pub root: Root, pub manifest: Manifest, pub index: IndexObject, pub packs: PackSource }`
and `PackSource` — the pack cache, with `pub fn chunk(&mut self, id: &ChunkId) -> Result<Zeroizing<Vec<u8>>>`
as its only accessor — are declared **here in `mod.rs`**, not in `fetch.rs`, even though 5-02 fills
them. Three wave-2 plans name these types across module lines; a type declared in `fetch.rs` and
consumed by `merge.rs` would make two parallel worktrees uncompilable, which is exactly what
4-01 learned. Same rule as `Applied` and `BackupRecord`.

Create the five sibling files with their frozen signatures and enough body to carry the tracer's
one file, leaving the hard cases to their owning plan and saying so in a module-level comment
naming the plan:

- `fetch.rs` — `pub async fn resolve(ctx: &RestoreCtx<'_>, local_anchor: Option<&Anchor>) -> Result<Resolved>`. The tracer fills pointer load (through `push::pointer::load`, which already probes `format` and checks `repo_id`), keyfile download and open, `Root::open`, and a single-pack `download_asset`. Plan 5-02 owns the bounds and the multi-pack cache.
- `merge.rs` — `pub fn plan(ctx: &RestoreCtx<'_>, resolved: &Resolved) -> Result<RestorePlan>`. The tracer fills `Create` and `RejectedPath` only. Plan 5-03 owns the rest.
- `write.rs` — `pub fn apply(ctx: &RestoreCtx<'_>, plan: &RestorePlan, packs: &PackSource) -> Result<Applied>`. The tracer fills the one-file happy path: `NamedTempFile::new_in(dest_dir)`, write, `sync_all`, explicit mode 0600, `persist`. Plan 5-04 owns directories, cleanup, and times.
- `backup.rs` — `pub fn take(ctx: &RestoreCtx<'_>, targets: &[PathBuf]) -> Result<Option<BackupRecord>>`, `None` meaning there was nothing on disk to preserve. The tracer fills the signature and that arm. Plan 5-05 owns the archive.
- `report.rs` — `pub fn render_plan(plan: &RestorePlan) -> String` and `pub fn render_outcome(outcome: &RestoreOutcome) -> String`, following `sync/report.rs`'s existing `render_*` shape. The tracer fills enough to print the tracer's one line. Plan 5-06 owns the table and the gate.

Add §11 to `docs/sync-format.md`: the chain a reader walks (pointer → keyfile asset → the newest
`SnapshotRecord`'s root → `manifest_chunks` → manifest → per-file chunk ids → `RemoteIndexEntry` →
pack asset → blob → plaintext), the manifest path encoding from task 1 with its four prefixes, and
the rule that the anchor advances only after the root verifies. A reader of §11 plus §10 should be
able to write a restore client without reading the crate.
  </action>
  <verify>
    <automated>cargo test --lib sync::restore</automated>
  </verify>
  <done>`cargo test --lib sync::restore` is green and the crate builds with all six restore modules present. Every cross-module type is declared in `restore/mod.rs` and none in a sibling. The recorder test proves backup precedes the first write and that a default-options run reaches neither. The anchor-ordering test proves a failed fetch leaves the anchor file unchanged. `docs/sync-format.md` carries a §11 covering the reader chain and the path encoding.</done>
  <reversibility rating="costly">`RestoreOptions`, `RestoreCtx`, `Disposition`, `ItemPlan`, `RestorePlan` and the five module signatures are what five wave-2 plans build against in parallel worktrees. Changing any of them after this merges reworks all five.</reversibility>
</task>

<task type="auto" tdd="true">
  <name>Task 3: `sync pull` end to end against mockito — one file, reported, then written</name>
  <files>src/widget/cli.rs, src/sync/cli.rs</files>
  <behavior>
    - `sync pull` with no flags against a mockito remote holding a one-file bundle exits 0, prints that the file would be created, and creates nothing anywhere under the injected roots.
    - The same run with `--apply` exits 0, creates the file with its exact pushed bytes, and the file's mode is 0600.
    - `--apply` and `--dry-run` together is a clap conflict, refused before anything runs.
    - A pull whose root fails to open (wrong password) exits non-zero, names the failure once, and creates zero files.
    - A pull against a mock with no pointer exits non-zero saying there is nothing pushed yet, and does not treat it as an empty successful restore.
  </behavior>
  <action>
Add `Pull` to `SyncAction` in `src/widget/cli.rs` with its **complete** flag set, frozen here so
plan 5-07 wires behaviour without redeclaring a flag: `--dry-run` (default behaviour, accepted for
symmetry with push and for UX-01's wording), `--apply` (conflicts with `dry_run`), `--force`,
`--allow-rollback`, `--rebuild-index`, `--force-rehash`, and `--yes`. Each `#[arg]` doc comment
states the consequence in the user's terms, not the mechanism — `--force` says what it can lose.
There is no `--password` flag and no env fallback; the passphrase arrives the way `sync push`
already takes it.

In `src/sync/cli.rs`, add the `SyncAction::Pull` arm to `run` and to Phase 3's injectable
`run_with`, mapping the flags onto `RestoreOptions` and building a `RestoreCtx` from the injected
`Config`, `SyncRoots`, `Endpoints`, and `now`. Reuse `keyfile_path` and `keys_at`; do not add a
second passphrase path. On success print `report::render_plan` for a dry run and
`report::render_outcome` for an applied one, and return 0; on error print the message once to
stderr and return 1, matching the module's existing shape.

Test through `run_with` against a `mockito::Server` seeded with a pointer, a keyfile asset, and
one pack, with `SyncRoots::at` pointing at a `TempDir`. Build the fixture by calling the **push**
side — `push::packer::build` — rather than hand-rolling remote JSON: a fixture assembled by hand
would pass while the real pair is broken, which is the failure this tracer exists to catch. Assert
mode with `std::os::unix::fs::PermissionsExt` behind `#[cfg(unix)]`.
  </action>
  <verify>
    <automated>cargo test --lib sync::cli</automated>
  </verify>
  <done>`ai-usagebar sync pull` runs end to end against a mock remote: dry-run by default with nothing written, `--apply` writing one file at mode 0600 with the exact pushed bytes. The fixture is produced by the push side, not hand-written. `--apply --dry-run` is refused by clap. No test reads a real `$HOME`, a real token, or opens a socket outside the mockito base. `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` are clean.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| remote bundle → local filesystem | Manifest paths, modes, and lengths are attacker-controllable in the accepted threat model (repo write access) and become local writes |
| plaintext pointer → process | The one unauthenticated remote object; everything of value inside it is sealed |
| remote counter → local anchor | A counter that decides whether this machine can ever read its own bundle again |
| decrypted plaintext → disk | The only place in the crate where a synced file's plaintext reaches a filesystem |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|-----------|----------|-----------|----------|-------------|-----------------|
| T-5-01 | Tampering | `layout::from_manifest_path` | critical | mitigate | Eight rejection cases checked before any I/O; the result is built component-by-component onto the root, never by joining an untrusted remainder, and is asserted to still `starts_with` its root. No `canonicalize` on a path the bundle influences |
| T-5-02 | Elevation of privilege | manifest naming machine-bound state | high | mitigate | `layout::accept_for_write` refuses anything `scope::is_excluded` refuses, reusing that predicate rather than restating its lists (D4) |
| T-5-03 | Denial of service | a forged high `counter` | critical | mitigate | The anchor is written only after `fetch::resolve` returns `Ok`, and from the root's **sealed** counter, never the plaintext pointer's. Ordering lives in one function so it cannot drift |
| T-5-04 | Spoofing | a counter borrowed by renaming a repo | high | mitigate | `anchor::accept` errors on a `repo_id` mismatch at any counter and under `allow_rollback`; the orchestrator calls it rather than reimplementing the comparison, asserted by a test |
| T-5-05 | Information disclosure | plaintext at a world-readable temp path | critical | mitigate | The tracer's write uses `NamedTempFile::new_in(dest_dir)` plus explicit mode 0600 before `persist`. Never `/tmp` — a different filesystem degrades `persist` to a copy that leaves the plaintext original behind (SAFE-05) |
| T-5-06 | Tampering | an absolute path already inside a pushed manifest | high | mitigate | `packer.rs` is changed to emit relocatable paths, so the format never contains an absolute path to be trusted; `from_manifest_path` refuses one regardless |
| T-5-07 | Information disclosure | a file's contents in an error or log line | high | mitigate | Errors carry a manifest path and an io source only, following the crate's existing `io_at` helper; nothing in `restore/` prints a body |
| T-5-08 | Repudiation | a silently dropped hostile entry | medium | mitigate | A rejected path becomes `Disposition::RejectedPath` and appears in the report; dropping it silently would hide tampering |
| T-5-SC | Tampering | npm/pip/cargo installs | high | mitigate | No new crates in this phase; `cargo machete` runs in the phase gate. No install task exists, so no legitimacy checkpoint is required |
</threat_model>

<verification>
- `cargo test --lib sync::restore` and `cargo test --lib sync::cli` green.
- `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, `cargo machete` clean.
- `grep -rn "canonicalize" src/sync/restore/` returns nothing.
- `grep -rn "std::env::temp_dir\|\"/tmp\"" src/sync/restore/` returns nothing.
- `HOME= cargo test --lib sync::restore` passes.
</verification>

<success_criteria>
1. `sync pull` reports one file and writes nothing; `sync pull --apply` writes it at 0600 with the pushed bytes.
2. The manifest path encoding is relocatable, bijective, and refuses all eight hostile forms.
3. `packer.rs` no longer emits an absolute path.
4. The anchor advances only after the root verifies, proven by a test on the failure path.
5. Every cross-module restore type is declared in `restore/mod.rs`, and the crate builds with five sibling modules only partly filled.
</success_criteria>

<output>
Create `.planning/phases/05-pull-and-restore/5-01-SUMMARY.md` when done, recording the exact frozen
signatures of `RestoreOptions`, `RestoreCtx`, `Disposition`, `ItemPlan`, `RestorePlan`,
`RestoreOutcome`, `Resolved`, `PackSource`, `fetch::resolve`, `merge::plan`, `write::apply`,
`backup::take`, `report::render_plan`, `report::render_outcome`, and `layout`'s three functions —
five wave-2 plans read that summary instead of this plan.
</output>
