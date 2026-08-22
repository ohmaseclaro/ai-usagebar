---
phase: 6
plan: 13
subsystem: sync
tags: [keystore, cursor, claude-desktop, safe-storage, credentials, sqlite]
requires:
  - 6-09 (the ForeignSafeStorage refusal, now the fallback rather than the primary path)
  - 6-10 (the keystore Store/Stores abstraction this extends)
provides:
  - "Store::CursorAuth — the cursorAuth/* rows travel, the 38 MB database does not"
  - "Store::DesktopTokenCache{profile,slot} — every Claude Desktop profile, decrypted on push and re-sealed under the target's key"
  - "cursor-user root: Cursor's conversation databases as an allow-list of three shapes"
  - "restore writes every file before any store, so two writers on one path have a fixed order"
affects:
  - src/cursor/db.rs
  - src/sync/keystore.rs
  - src/sync/mod.rs
  - src/sync/scope.rs
  - src/sync/plan.rs
  - src/sync/report.rs
  - src/sync/push/packer.rs
  - src/sync/restore/layout.rs
  - src/sync/restore/merge.rs
  - src/sync/restore/write.rs
  - tests/sync_restore_e2e.rs
tech-stack:
  added: []
  patterns:
    - "a credential travels as its value, never as the file that happens to hold it"
    - "decrypt-on-push / re-encrypt-on-restore through an injected key, so the transform is pure and tests run on every platform"
    - "row-level SQLite write inside one transaction with SQLite's own busy handler, never a file copy"
    - "allow-list of named shapes rather than a deny-list of known junk"
    - "files before stores in the write queue, so two writers on one path cannot race on category order"
key-files:
  created: []
  modified:
    - src/cursor/db.rs
    - src/sync/keystore.rs
    - src/sync/mod.rs
    - src/sync/scope.rs
    - src/sync/plan.rs
    - src/sync/report.rs
    - src/sync/push/packer.rs
    - src/sync/restore/layout.rs
    - src/sync/restore/merge.rs
    - src/sync/restore/write.rs
    - tests/sync_restore_e2e.rs
decisions:
  - "Cursor's credential travels as rows and never as state.vscdb: 424 bytes inside 38 MB of the receiving machine's own editor state."
  - "Store::CursorAuth is the one cross-platform store — no Keychain, no safeStorage, a plaintext JWT in a SQLite key-value table on every OS. Its condition is the database existing, not the platform."
  - "The Desktop token caches stopped being collected as files the moment the store carried them. Two carriers for one credential is two carriers that disagree."
  - "Cursor's conversations are files under the existing opt-in transcripts switch, not a sixth category: same kind of thing, same order of magnitude, already off by default."
  - "restore::write sorts files ahead of stores, so the keystore's row-write lands on top of a replaced state.vscdb rather than under it."
  - "desktop-state/ cookies are not re-encrypted. The login is the token cache; the cookie blobs stay inert on the target and the app re-authenticates from the restored token."
metrics:
  duration: ~3h
  completed: 2026-08-21
status: complete
---

# Phase 6 Plan 13: Cursor and every Claude Desktop login travel between two Macs — Summary

The `credentials` category now carries the **Cursor sign-in** and **all four
Claude Desktop profiles** across machines, and the opt-in `transcripts`
category carries **Cursor's conversations**. Nothing new is copied as opaque
bytes: each credential travels as its *value* and is written back into whatever
holds it on the target.

## Signature changes

Stated first, as asked.

| Symbol | Change |
|---|---|
| `keystore::Store` | **no longer `Copy`** — the Desktop variant names a profile. Every `Store` argument moved to `&Store`. |
| `keystore::Store::ALL` | **removed.** Enumeration is per-machine now; use `Stores::all()`. |
| `Store::manifest_path(&self) -> String` | was `(self) -> &'static str` |
| `Store::describe(&self) -> String` | was `(self) -> &'static str` |
| `Store::CursorAuth`, `Store::DesktopTokenCache { profile, slot }` | new variants |
| `Store::read_failure_is_fatal(&self) -> bool` | new |
| `keystore::TokenSlot` | new public enum (`V2`, `V1`), owns the `config-tokenCache{,V2}` spellings |
| `keystore::MachinePaths` | new public struct; `MachinePaths::new(cursor_db, desktop_profiles_dir)` |
| `keystore::Stores::Machine` | `Machine` → **`Machine(Arc<MachinePaths>)`** |
| `Stores::{read,has,write,writable}` | take `&Store` |
| `Stores::all(&self) -> Result<Vec<Store>>` | new — the push side's enumeration |
| `Stores::read_or_skip(&self, &Store)` | new — folds a per-profile failure into `None` |
| `keystore::Fixture::get(&self, &Store)` | takes `&Store` |
| `SyncRoots::cursor_user_dir` | **new field.** Derived inside `SyncRoots::at`, so all 26 test callers are unchanged. |
| `restore::layout::ROOT_PREFIXES` | `[_; 4]` → **`[_; 5]`** (`cursor-user`) |
| `cursor::db::{read_auth_rows, has_auth_rows, write_auth_rows}` | new public fns |
| `plan::plan_store`, `restore::write::write_store` | take `&Store` (private) |

`Cargo.toml` and `Cargo.lock` are byte-identical. No crate added — `rusqlite`
was already a dependency of `cursor::db` and `sync::index`.

## What now travels between two Macs, and what still does not

**Travels (new):**

- **The Cursor sign-in.** Every `cursorAuth/*` row of `state.vscdb` — access
  token, refresh token, cached email, sign-up type, membership type, membership
  scopes — carried as one JSON object and written back **row by row** into the
  target's own database. The 401 on the second Mac is what this fixes.
- **All four Claude Desktop logins.** Each profile's `config-tokenCacheV2` and
  `config-tokenCache` are decrypted with the pushing Mac's Safe Storage key,
  carried as plaintext *inside the already-encrypted bundle*, and re-sealed
  under the **target's** key on arrival. One key, N values, per profile.
- **Cursor's conversations**, under the existing opt-in `transcripts` switch:
  `globalStorage/state.vscdb`, `globalStorage/conversation-search.db`, and
  every `workspaceStorage/*/state.vscdb`.

**Still does not travel, and why:**

- **`desktop-state/` cookie and local-storage values.** They are still carried
  as files (unchanged), but the values inside a Chromium `Cookies` database are
  sealed with the same Safe Storage key, so on the target they are inert. The
  consequence is bounded: the *account is signed in* — `anthropic::desktop_creds`
  authenticates from the token cache, which is exactly what this plan moves —
  and the in-app web view re-authenticates from that token instead of resuming a
  browser session. Re-encrypting them means walking a `Cookies` table row by row
  plus a `leveldb` tree, each with its own per-row format; that is separate work
  and the account works without it. Recorded as a ceiling in `keystore`'s docs.
- **A `CLAUDE_CONFIG_DIR`-scoped Claude Code login.** Unchanged ceiling from
  6-10: it lives under a per-account Keychain service name, and a bundle-chosen
  account name reaching a service-name hash wants its own validation.
- **Cursor's `-wal`/`-shm` sidecars.** A database copied while Cursor is running
  can be missing whatever is only in its write-ahead log — the same exposure
  every other SQLite file in the bundle already has. Quitting Cursor before a
  push checkpoints them.

## Measured bundle size, before and after

`sync status --json` on the user's machine, same config, `main` versus this
branch:

| category | before | after | delta |
|---|---|---|---|
| config | 1 file, 310 B | 1 file, 310 B | — |
| credentials | 108 files, 24,963,746 B | 109 files, 24,952,026 B | **+1 file, −11,720 B** |
| routines | 9 files, 15,377 B | unchanged | — |
| chat_index | 1,543 files, 80,220,553 B | unchanged | — |
| transcripts | 2,184 files, 2,119,972,143 B | 2,383 files, 2,378,417,351 B | **+199 files, +258,445,208 B** |
| **total** | **3,845 files, 2,225,172,129 B (2.07 GiB)** | **4,045 files, 2,483,605,617 B (2.31 GiB)** | **+200 files, +246.5 MiB** |

Reading the two deltas:

- **credentials −11,720 B.** Eight `config-tokenCache{,V2}` files (four
  profiles × two slots) left the file collector; the machine-bound store count
  went from 1 to 10 (Claude Code + Cursor + those eight). The *credentials* the
  user gains cost the bundle **less** than before, because a store's count is
  reported without reading its value and the sealed files stopped being carried.
- **transcripts +199 files.** Exactly `globalStorage/state.vscdb` +
  `globalStorage/conversation-search.db` + 197 `workspaceStorage/*/state.vscdb`
  — the 197 workspaces measured on that Mac, one file each.
- **+246.5 MiB out of a 37 GB Cursor directory** — 0.65% of it. The allow-list
  is what makes that number small: the 33 GB `state.vscdb.bloated.bak`, 1.6 GB
  of agent-worker data, 688 MB of `History`, and ~6 GB of caches are excluded by
  not being named.

The would-upload figure could not be measured: `sync push --dry-run` prints
`would upload: not computed — no sync password arrived on stdin`, and the
password is the user's. Against the calibrated ratio the phase already recorded
(2.1 GiB raw pushing ~880 MiB, ~2.4×), the new ~246 MiB of SQLite should add
roughly 100 MiB to a full push — **an estimate, not a measurement.**

## How it works

### `Store::CursorAuth` — rows, never the database

`cursor::db` gained three verbs beside its existing read-only reader:

- `read_auth_rows(path)` — every row whose key starts with `cursorAuth/`, as a
  `BTreeMap`. Ordered, so the same login serialises to the same bytes twice and
  `merge::decide_store`'s digest comparison can answer *identical* instead of
  asking about a credential that did not change. A missing database is an empty
  map (never signed in); a database that exists but will not open is an error,
  because reporting a locked store as "no login here" is how a push ships a
  bundle without the credential in it.
- `has_auth_rows(path)` — `SELECT 1 … LIMIT 1`, no value returned. `sync status`
  runs on every macOS menu open and may not read a credential.
- `write_auth_rows(path, rows)` — one transaction, `INSERT OR REPLACE` per row.

The namespace is a **prefix, not a hand-list**. The six keys Cursor's sign-in
writes were measured, but the list is Cursor's to change and a copy of it here
goes stale silently — leaving a restored session that authenticates and then
cannot say which plan it is on. It is also the *write* rule: a restore may only
write that namespace, so a tampered bundle cannot reach the editor-state keys
that share the table. (SQLite's `LIKE` is ASCII-case-insensitive, so the byte-exact
check is applied in Rust rather than trusted to the query.)

**If Cursor is running during a restore.** The write takes SQLite's writer lock
with a 5-second busy timeout — SQLite's own handler, not a lock invented here,
because `state.vscdb` is Cursor's file and a second protocol only one side
observes is not a lock. If Cursor holds it, the transaction never begins, the
error refuses *that one item*, and the existing rows are untouched. The converse
is the case this cannot solve and the user must know: a running Cursor holds
those values in memory and may write them back over ours when it next
persists. **Quit Cursor before restoring into it.** Every error on this path
names "(is Cursor running?)".

### `Store::DesktopTokenCache { profile, slot }` — decrypt out, re-encrypt in

`Stores::all()` lists the profile store on disk, so four accounts produce eight
stores rather than one. Push decrypts each with `safe_storage::macos_key()`;
restore re-seals with the *target's* key and writes atomically (the tempfile is
created 0600 and keeps that mode across the rename).

**Per-profile failure is per-profile.** `Store::read_failure_is_fatal()` is
`true` for the two single stores — a bundle that silently omitted the Claude
Code or Cursor login is the defect this module exists to end — and `false` per
Desktop profile. `Stores::read_or_skip()` names the failing profile on stderr
and carries the other three. Nothing in that message is or contains a
credential; it is `Store::describe()` plus the error, and the profile label is
rendered with `{:?}`, which escapes.

The **one bundle-chosen component** in the whole module is that profile label,
and `plain_component` checks it before it means anything: one ordinary
`Component::Normal` (the rule `restore::layout` already uses per component),
plus explicit rejections of `\` and `:` — `Path::new("C:")` is one ordinary
component on Unix and a drive-relative path on Windows — plus control
characters, a leading `.`, and `NAME_MAX`. A name that fails is **not a store**,
so it is skipped rather than resolved anywhere.

`ForeignSafeStorage` (6-09) is untouched and is now the fallback: bundles pushed
before this existed still carry those files, and a target with no key of its own
still refuses them.

### Cursor conversations — an allow-list, and no walk

Measured, that directory is 37 GB. The collector names three shapes and
everything else is excluded by not being named. A deny-list that missed
`state.vscdb.bloated.bak` would have multiplied the bundle fifteen-fold, and
would silently admit whatever Cursor adds next.

It also means **no directory walk**: two explicit files plus one listing of
`workspaceStorage`. The 197 workspaces are 197 `push_path` calls, so nothing
here can hit `MAX_WALK_ENTRIES` and quietly truncate a user's chat history at
some count. (`scan.walk_capped` is asserted false in the test.)

They are filed under `SyncCategory::Transcripts` rather than a sixth category:
the same kind of thing as Claude Code's transcripts, the same order of
magnitude, and already **off by default**, which is the right posture for
246 MiB. They are added after `transcripts::collect_bounded`, so the D3 age/byte
bounds do not apply to them — their bound is the allow-list itself.

### Two writers on one path, and which wins

`cursor-user/globalStorage/state.vscdb` is carried as a *file* (it holds the
conversations) **and** has the `cursorAuth/*` rows written into it by the
keystore. `restore::write::apply` now sorts the queue so **every file is written
before any store**, whatever the manifest says — `sort_by_key` on a `bool` is
stable, so within each half manifest order is preserved and a partial restore
still stops in the same place twice. The file carries the conversations; the
store's row-write lands on top.

The order is fixed in the write queue rather than left to category order,
because category order is not something this should rest on. The test plants the
store *first* in manifest order, makes the file's write fail, and asserts the
store stayed empty.

**The file cannot smuggle a login past the credential gate.** The source
machine's rows are inside that database, and `state.vscdb` is not itself
credential-bearing, so `--force` alone would write it. It does not: if the
target holds a *different* live Cursor login the store is
`ReplacesLiveCredential`, and `write::apply`'s preflight refuses the **whole
run** before the first byte — so the file never lands either.
`a_refused_cursor_store_stops_the_database_file_landing_too` asserts exactly
that.

## Non-negotiables, held

- **Opt-in stays opt-in.** Every store is `SyncCategory::Credentials`; with that
  switched off `plan::build` never calls `Stores::all()`, so no store is read.
  Cursor's conversations are `Transcripts`, off by default. The D-04 private-repo
  gate is unchanged and still refuses a credential-bearing bundle for a readable
  repository.
- **`--force` alone overwrites neither.** Writing a Cursor token or a Desktop
  token cache *is* overwriting a live login, and `decide_store` treats both
  exactly as 6-10 treats the Keychain credential: a different live credential is
  `ReplacesLiveCredential` until `--force-credentials`, and identity is decided
  by hashing with the same `Keys::chunk_id` the push used, so a repeated pull
  onto the machine that pushed asks nothing.
- **A failed decrypt, re-encrypt or database write refuses that item and leaves
  the existing one untouched.** No path is a read-modify-write: the encryption
  happens before the file write, and the database write is one transaction.
- **No secret in any message.** Nothing logs, prints, or formats a token, a
  decrypted blob, or a key. `Stores`' hand-written `Debug` still redacts;
  `desktop_read`'s refusals name the store and never the value; `cursor_write`'s
  parse error is a fixed string rather than `serde_json`'s, which can quote its
  input. Asserted in `debug_never_renders_a_stored_credential` and in each
  refusal test.
- **No crate added.** `Cargo.toml`/`Cargo.lock` byte-identical.

## Is `CursorAuth` cross-platform? Yes — and here is why

Cursor keeps a bare JWT in a SQLite key-value table on macOS, Linux and Windows
alike. There is no Keychain, no safeStorage, and no platform-specific sealing
anywhere in the path. Gating it on macOS would refuse a working restore for no
reason. The reason it is a *store* rather than a file is not the platform — it
is that 424 bytes of credential live inside 38 MB of the **receiving** machine's
editor state. So `machine_writable` gives three different answers for three
different reasons: Claude Code's Keychain item is macOS or nothing, a Desktop
token cache needs a Safe Storage key to re-seal with, and Cursor needs only the
database to exist. This build will not fabricate one — a target that has never
opened Cursor is told to open it once.

`machine_all` lists the Desktop caches on macOS only: off-Mac each would be
enumerated, read, fail for want of a key, and warn about a credential the
platform never had. That gate is `cfg!` plus a directory listing, deliberately
**not** `MachinePaths::safe_key` — `sync status` reaches this path, the macOS
menu bar runs `sync status --json` on every menu open, and reading the Safe
Storage key runs `security(1)` against an item whose ACL does not name it, which
can raise a Keychain prompt. Existence is answered with a stat.

## Hermeticity

No test touches the real login Keychain, the real Cursor database, or the real
profile store.

- The Desktop transform takes its key as an **argument** (`Option<Key>`), so
  `desktop_read`/`desktop_write` are pure and exercised on Linux CI too. Only
  `machine_safe_key()` is `#[cfg(target_os = "macos")]`.
- The Cursor verbs take a path, so tests seed a temp database.
- `Stores::Machine` is still constructible in exactly one production place, and
  the structural guard now also refuses a stray `MachinePaths::new`.

## Tests

`cargo test` **1763 passing, 0 failing** (was 1801 across the full suite;
`--lib` was 1749 before this plan, 1763 after). `cargo clippy --all-targets -D warnings`
clean, `cargo fmt --check` clean, `make test` green including the GNOME, KDE and
Omarchy contract suites.

New coverage, by the property asked for:

| property | test |
|---|---|
| a Cursor token round-trips push → restore | `keystore::the_cursor_store_round_trips_the_rows_through_a_real_database` |
| **the other rows in the target database are byte-identical afterwards** | `cursor::db::writing_the_login_leaves_every_other_row_untouched` (500 seeded editor-state rows, compared as a map before and after) |
| a Desktop cache decrypted under one key opens on a target with another | `keystore::a_desktop_cache_sealed_by_one_mac_opens_on_another` |
| a target with no key refuses, local file untouched | `keystore::a_target_with_no_key_refuses_and_leaves_the_existing_login_alone`, `merge::a_target_that_cannot_reseal_a_desktop_cache_is_refused_in_the_planner` |
| `--force` alone overwrites neither | `merge::force_alone_replaces_neither_a_live_cursor_login_nor_a_desktop_one` |
| the category off carries neither | `plan::switching_credentials_off_carries_no_store_at_all` |
| four profiles all travel | `plan::every_seeded_store_reaches_the_plan_under_its_own_wire_name`, `merge::a_cursor_login_and_four_desktop_profiles_all_land_in_one_restore`, `keystore::every_profile_and_slot_on_disk_is_enumerated` |
| one bad profile does not take the others down | `keystore::a_failing_desktop_profile_is_skipped_while_a_failing_single_store_is_fatal` |
| a traversing profile name is not a store | `keystore::a_profile_name_that_is_not_one_plain_directory_name_is_not_a_store`, `merge::a_desktop_entry_with_a_traversing_profile_name_is_excluded_never_resolved`, `layout::a_machine_bound_stores_wire_name_never_resolves_to_a_place_on_disk` |
| Cursor's allow-list excludes the 33 GB backup | `scope::cursor_carries_the_three_conversation_shapes_and_nothing_else` |
| conversations are opt-in | `scope::cursor_conversations_do_not_travel_unless_transcripts_are_switched_on` |
| files are written before stores | `write::every_file_is_written_before_any_store_whatever_the_manifest_order_says` |
| the file cannot bypass the credential gate | `merge::a_refused_cursor_store_stops_the_database_file_landing_too` |
| a sealed blob is not copied machine to machine | `sync_restore_e2e::criterion_1_…` (new assertions) |

**Linux verified** in `rust:1.88` under Docker: `cargo clippy --all-targets -D warnings`
clean, `cargo test --lib` **1754 passed, 1 failed** — the failure is
`supergrok::acp::tests::missing_binary_has_a_clear_non_secret_error`, which
fails identically on untouched `main`.

## Production call sites — every function added, enumerated

Asked for explicitly, because this milestone has shipped thirteen instances of
tested code nothing calls. Counted over **production code only** (comments and
`#[cfg(test)]` regions stripped, definitions excluded). **No symbol has zero.**

| symbol | production call sites | reached from |
|---|---|---|
| `cursor::db::read_auth_rows` | 1 | `keystore::cursor_read` → `machine_read` → `Stores::read` → `plan::build`, `packer::build` |
| `cursor::db::has_auth_rows` | 1 | `machine_has` → `Stores::has` → `report::count_stores` → `sync status` |
| `cursor::db::write_auth_rows` | 1 | `keystore::cursor_write` → `machine_write` → `Stores::write` → `restore::write::write_store` |
| `cursor::db::text_at` | 1 | `read_auth_rows` |
| `keystore::plain_component` | 3 | `Store::from_manifest_path`, `desktop_write`, `desktop_caches` |
| `keystore::desktop_caches` | 1 | `machine_all` |
| `keystore::machine_all` | 1 | `Stores::all` → `plan::build`, `report::count_stores` |
| `keystore::cursor_read` / `cursor_write` | 1 each | `machine_read` / `machine_write` |
| `keystore::desktop_read` / `desktop_write` | 1 each | `machine_read` / `machine_write` |
| `keystore::profile_label` | 4 | `desktop_read` (×3), `desktop_write` |
| `keystore::claude_code_read` / `_has` / `_write` | 1 each | `machine_read` / `machine_has` / `machine_write` |
| `Store::read_failure_is_fatal` | 1 | `Stores::read_or_skip` |
| `Stores::read_or_skip` | 2 | `plan::build`, `packer::build` |
| `Stores::all` | 2 | `plan::build`, `report::count_stores` |
| `keystore::MachinePaths::new` | 1 | `SyncRoots::resolve` → `sync::cli::run` |
| `keystore::TokenSlot::from_wire` | 1 | `Store::from_manifest_path` |
| `keystore::TokenSlot::file_name` | 6 | wire spelling, `desktop_caches`, `desktop_read/write`, `profile_label`, `describe` |
| `scope::collect_cursor` | 1 | `scope::collect(Transcripts)` |
| `SyncRoots::cursor_user_dir` | 4 | `collect_cursor` (×2), `layout::ROOT_PREFIXES`, `packer::manifest_path` |

Smoke-tested against the real machine: `./target/release/ai-usagebar sync status`
and `sync status --json` both run and report the ten stores and the 199 Cursor
files — the numbers in the size table above came from that run, not from a
prediction.

## Deviations from plan

**[Rule 2 — missing critical functionality] `plain_component` rejects `:`.**
The first version reused `restore::layout`'s `Component::Normal` rule alone.
`Path::new("C:")` is one ordinary component on Unix, so a bundle naming the
profile `C:` was accepted on macOS while being drive-relative on Windows —
"an absolute path in disguise", which `layout::from_manifest_path` refuses by
name. Caught by the module's own traversal test. Fixed with an explicit `\` and
`:` rejection, since a bundle is portable and a name has to be refused on macOS
for being dangerous on Windows.

**[Rule 2] `machine_all` gates the Desktop caches on the platform.** The first
version enumerated them everywhere, so a Linux machine with a copied profile
store would read each one, fail for want of a key, and print a warning per
profile on every push. Narrowed to macOS, with an explicit note that the gate is
a directory listing and not a key read, because `sync status` is on this path.

**[Rule 1 — bug] The e2e fixture compared the two trees byte for byte.**
`bundle_view` walked both machines' roots and required identical file sets, so
dropping `config-tokenCache` from the collector failed four e2e tests. That
comparison was asserting the thing that must *not* happen — a blob sealed under
A's key copied onto B. `bundle_view` now skips the two token-cache names with a
comment saying why, and criterion 1 gained an explicit assertion that the sealed
file exists on A and does **not** exist on B.

**[Rule 1] `text_at` was lossy.** The first version used
`String::from_utf8_lossy` on a `state.vscdb` value. A replacement character
substituted into a token produces a credential that is silently wrong rather
than visibly missing; changed to a strict `from_utf8` that refuses.

## Commits

| commit | what |
|---|---|
| `1542295` | `cursor::db` gains the row-level read/has/write over the `cursorAuth/` namespace |
| `16d5935` | the keystore extension, the collector changes, the `cursor-user` root, and the write ordering |
| `ab240a1` | the tests, including the byte-identical-rows assertion and the e2e change |
| `665fe8e` | the platform gate on `machine_all` and the file-cannot-bypass-the-gate test |

## Known Stubs

None.

## Self-Check: PASSED
