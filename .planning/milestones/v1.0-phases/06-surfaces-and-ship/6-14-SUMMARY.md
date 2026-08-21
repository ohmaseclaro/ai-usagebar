---
phase: 6
plan: 14
subsystem: sync
tags: [keystore, claude-desktop, cookies, safe-storage, credentials, sqlite, chromium]
requires:
  - 6-13 (the Store/Stores machinery, the files-before-stores write order, and the token cache this completes)
  - 6-09 (the ForeignSafeStorage refusal, still the fallback for pre-6-13 bundles)
provides:
  - "Store::DesktopCookies{profile} — the Claude Desktop web view's session travels between Macs"
  - "claude_desktop::cookies — row-level read/has/write over a Chromium cookie jar, keyed on the whole seven-column unique index"
  - "safe_storage::{looks_like_raw,decrypt_raw,encrypt_raw} — the raw framing a Cookies row stores, beside the base64 one a token-cache file stores"
  - "measured proof that the cookie table is the entire remaining delta between a local account switch and a restored one"
affects:
  - src/claude_desktop/cookies.rs
  - src/claude_desktop/mod.rs
  - src/safe_storage.rs
  - src/sync/keystore.rs
  - src/sync/restore/write.rs
tech-stack:
  added: []
  patterns:
    - "one carrier for the row and one for the secret inside it — the file brings twenty columns, the store brings the one a copy cannot"
    - "carry the whole plaintext, prefix and all, so this build never has to agree with Chromium about where a value begins"
    - "identity is the database's own unique index, read off the schema, never the three columns that look obvious"
    - "UPDATE only — a restore may re-seal a row and may never create or delete one"
    - "let SQLite's own recovery handle a hot journal; a second recovery protocol beside it would be the bug"
key-files:
  created:
    - src/claude_desktop/cookies.rs
  modified:
    - src/claude_desktop/mod.rs
    - src/safe_storage.rs
    - src/sync/keystore.rs
    - src/sync/restore/write.rs
decisions:
  - "Cookie identity is Chromium's seven-column cookies_unique_index, not (host_key, name, path): the reported three-tuple is not unique on a real jar, where partitioned cookies share host, name and path."
  - "The whole Chromium plaintext travels — SHA256(host_key) prefix included — rather than the value alone. The domain binding survives for free, and no version of this build has to track where Chromium puts the boundary."
  - "The jar keeps travelling as a file and the store carries only values. That is not two carriers for one thing: the file brings the twenty columns a value cannot reconstruct, the store brings the one column a file copy cannot make usable."
  - "bridge-state.json and ant-device-registry.json stay out, verified rather than assumed. Neither has ever travelled, and carrying either would be wrong — a stale cse_… id breaks /remote-control, and a device registration describes the Mac that made it."
  - "A hot Cookies-journal is left to SQLite. Opening the jar rolls it back, our transaction commits on the recovered database, and the journal is deleted — the restored pair ends self-consistent, which is strictly better than before."
metrics:
  duration: ~3h
  completed: 2026-08-21
status: complete
---

# Phase 6 Plan 14: the Claude Desktop app signs in on the second Mac — Summary

**What a user gets after this:** on a second Mac, `ai-usagebar usage` shows all
four Claude Desktop accounts' quota — that was 6-13 — **and the app itself opens
signed in**, because the web view's session cookies now cross too. What still
needs a sign-in is stated in its own section below, and so is the one thing I
could not test.

## Read this first: what is evidence and what is inference

The user has been told twice that something works. Here is the split.

**Proven, on their own machine, by measurement:**

| claim | how |
|---|---|
| every cookie value is a safeStorage `v10` blob | 108/108 rows across all four profiles |
| the LevelDB trees hold no sealed value at all | `grep -rl v10` over `Local Storage`, `Session Storage`, `IndexedDB` in all four profiles → **0 files** |
| Chromium seals `SHA256(host_key) ‖ value`, not the value | 108/108 rows, prefix verified against the row's own `host_key` |
| the transform is lossless on real data | decrypt → re-encrypt under the same key reproduces the original blob **byte for byte**, 108/108 |
| a re-sealed value opens under the second key and no longer under the first | 108/108 |
| identity needs seven columns | the jar's `cookies_unique_index` has seven, and `top_frame_site_key` / `has_cross_site_ancestor` genuinely vary across rows |
| `bridge-state.json` has never travelled | it is in `scope::EXCLUDED_NAMES`, and is not in the profile store to begin with |
| `ant-device-registry.json` has never travelled | same, plus an existing test that asserts both |

**Inferred, and not proven, because it needs two Macs:** that a re-sealed
`sessionKey` is accepted by claude.ai when it arrives from a different machine.
The cookie is a server-side session, not a device-bound token, and nothing in
its plaintext binds it to hardware — but I have not watched the app open signed
in on Mac B. **That is the one acceptance test still outstanding**, and it is
the user's to run.

Two cookies in the jar *will* be re-issued rather than resumed, and that is
normal: `cf_clearance` is Cloudflare's bot clearance, bound to IP and
user-agent, and `__cf_bm` is its short-lived sibling. Both are re-minted on the
first request. They are carried because leaving a hole in the jar is worse than
carrying a value the server replaces.

## What was measured, before anything was built

The brief named `__ssid` as the session cookie. It is not — it is a 36-byte
analytics id. The session is **`sessionKey` and `sessionKeyV3`**, both
`httponly` and `secure` on `.claude.ai`, 131 bytes of value each. The brief also
gave the primary key as `(host_key, name, path)`. Chromium's actual index is

```
CREATE UNIQUE INDEX cookies_unique_index ON cookies(
  host_key, top_frame_site_key, has_cross_site_ancestor,
  name, path, source_scheme, source_port);
```

and the jar carries three distinct `(top_frame_site_key, has_cross_site_ancestor)`
combinations, so the three-column match would have been a live bug: a
partitioned twin sharing host, name and path would have been overwritten with
the other partition's value. `identity_is_the_whole_index_so_a_partitioned_twin_is_not_overwritten`
is the test, and it fails against the three-column version (negative control run).

The third measurement is the one that decided the payload format. Every
plaintext is 32 bytes longer than its value, and those 32 bytes are
`SHA256(host_key)` — Chromium's domain binding, which it re-checks on read and
drops the cookie if it does not match. So the payload carries the **whole
plaintext**, prefix included: nothing here has to know where Chromium puts the
boundary, the binding survives the crossing intact, and a tampered bundle that
paired one domain's row with another's plaintext produces a cookie Chromium
itself rejects.

## The coordinator's three points, answered

### 1. Cookies are the whole scope — confirmed

`restore_desktop_state` moves `COOKIE_FILES` and `LEVELDB_DIRS` and nothing
else. The LevelDB trees are plaintext (0 `v10` blobs in 17 MB, all four
profiles) and already arrive working as files. So **cookies + token cache is the
entire delta between a local switch and a remote one.** Built as briefed.

**`Cookies-journal`: it cannot corrupt the target, and this plan improves it.**
I built a genuinely hot journal — a process killed mid-transaction — and ran the
production write path at it. SQLite rolls the journal back on open (the correct
recovery, and the same one Chromium would perform later anyway), our transaction
then commits on the recovered database, and **the journal file is deleted**. The
restored pair is left self-consistent, where before this plan the stale journal
travelled into the live data directory untouched. Nothing here inspects or
removes the journal by hand; a second recovery protocol beside SQLite's own
would be the bug rather than the fix.

### 2 and 3. `bridge-state.json` and `ant-device-registry.json` — verified excluded, nothing to build

Both are in `scope::EXCLUDED_NAMES` (case-folding-safe `FixedName`), neither is
in the profile store to begin with, and
`scope::bridge_state_and_the_device_registry_never_leave_a_profile` already
asserts it — so no stale `cse_…` id can reach the target and break
`/remote-control`, and each Mac keeps its own device registrations, which is the
correct outcome and needs no merge because nothing arrives.

## How it works

### `claude_desktop::cookies` — SQL only, no key

Three verbs beside the local switch that owns the file:

- `read_sealed(path)` — every row as `(CookieKey, Vec<u8>)`, sorted by identity
  so one unchanged jar hashes to the same chunk ids twice and
  `merge::decide_store` can answer *identical*. A missing jar is an empty vector
  (never signed in); a jar that exists and will not open is an **error**,
  because reporting an unreadable jar as "no session here" is how a push ships a
  bundle without the thing it was asked to carry.
- `has_rows(path)` — `SELECT 1 … LIMIT 1`, no value returned. `sync status` runs
  on every macOS menu open and may not read a credential.
- `write_sealed(path, rows)` — one transaction, `UPDATE` per row, five-second
  busy timeout. Returns `(updated, missing)`.

**`UPDATE`, never `INSERT`.** The twenty other columns — expiry, `is_secure`,
`is_httponly`, `samesite`, creation time — belong to the `Cookies` file, which
travels as a file and lands first. A carried row that matches nothing is counted
and skipped, so nothing here can invent a cookie. A call where *nothing* matched
is an `Err` with the transaction dropped, because committing there would report
a restored login where none landed.

The crypto is deliberately not in this module: the keystore holds the key and
this holds the SQL, so the blobs passing through are opaque bytes and the whole
file compiles and tests on every platform.

### `keystore::Store::DesktopCookies` — decrypt out, re-seal in

The same shape as `DesktopTokenCache`, and enumerated beside it from the profile
store on disk, each on its own file's existence — a profile can legitimately
have a token cache and no jar (one of the four does).

The wire payload is `[[identity, base64(plaintext)], …]`, ordered by identity.

**Per-row isolation.** A value that is not `v10` at all is skipped *silently* —
it was never this scheme's, so there is nothing to report. A `v10` value that
will not open under this Mac's key is skipped and **named** on stderr, by cookie
name only. One bad row costs the other twenty-five nothing, and the target's
copy of a row that did not travel is left byte-identical.

**Per-profile isolation.** `read_failure_is_fatal()` is `false`, so
`Stores::read_or_skip` names the failing profile and carries the other three.

### Two writers on one path, in one category

`desktop-profiles/<profile>/desktop-state/Cookies` is carried as a file **and**
has its values written by the store. `restore::write::apply` already sorts every
file ahead of every store, over the whole queue:

```rust
queue.sort_by_key(|(_, target)| matches!(target, Target::Store(_)));
```

That is a property of the queue and not of category order — asked for
explicitly, and now asserted for the case that proves it. The 6-13 test paired a
`Transcripts` file with a `Credentials` store, so it would have passed on
category order alone; `the_cookie_jar_lands_before_its_store_even_when_both_are_credentials`
puts **both halves in `Credentials`** with the store first in manifest order,
makes the file write fail, and asserts the store stayed empty. Deleting the
`sort_by_key` fails it (negative control run).

## Non-negotiables, held

- **Opt-in stays opt-in.** `SyncCategory::Credentials`. With it off, `plan::build`
  never calls `Stores::all()`, so no jar is opened and no key is read.
  `plan::switching_credentials_off_carries_no_store_at_all` covers it.
- **The D-04 private-repo gate is unchanged** and still refuses a
  credential-bearing bundle for a readable repository.
- **`--force` alone overwrites nothing.** A jar holding a different live session
  is `ReplacesLiveCredential` until `--force-credentials`, decided by the same
  `Keys::chunk_id` hash the push used — so a repeated pull onto the machine that
  pushed asks nothing.
- **No secret in any message.** No cookie value, plaintext, or key reaches a log
  line, an error, or a `Debug`. The only jar-derived text in any message is a
  cookie *name* (`sessionKey`, `__cf_bm`) — not a secret, and rendered with
  `{:?}`, which escapes. `serde_json`'s parse error is replaced with a fixed
  string because it can quote its input, and its input is a jar full of live
  sessions. Asserted in `no_message_on_any_arm_carries_a_cookie_value`.
- **macOS-gated like the rest.** `machine_all` lists these on macOS only, via a
  directory stat rather than a key read, because `sync status` is on that path
  and reading the Safe Storage key runs `security(1)`. Linux and Windows build.
- **No crate added.** `Cargo.toml` and `Cargo.lock` are **byte-identical** —
  `git diff HEAD~3 --name-only | grep -c Cargo` → 0. `rusqlite`, `serde`,
  `base64` and `zeroize` were all already dependencies.
- **No test touches the real jar or the real Keychain.** Both keys are injected,
  so `cookies_read`/`cookies_write` are pure and run on Linux CI; jars are
  seeded into temp directories from Chromium's verbatim schema.

## Measured cost on the user's machine

Both binaries run against **the same machine state today**, so the delta is
honest (the 6-13 summary's figures were taken on an older state and are not
comparable line for line):

| binary | credentials | delta |
|---|---|---|
| `main` @ `c33cda3` (6-13) | 116 files, 37,636,993 B | — |
| this branch | 120 files, 37,636,993 B | **+4 files, +0 bytes** |

Four files, one per profile, and **zero bytes**: a store's count is reported
without reading its value, and the jar itself was already travelling as a file.
The most valuable thing in the bundle is now also the cheapest thing added to it.

## Tests

`cargo test` **1853 passing, 0 failing** (baseline 1838, **+15**).
`cargo clippy --all-targets -- -D warnings` clean, `cargo fmt --check` clean,
`make test` green including the GNOME, KDE and Omarchy contract suites.

**Linux** in `rust:1.88` under Docker: clippy clean, `cargo test --lib` 1780
passed / 1 failed — `supergrok::acp::tests::missing_binary_has_a_clear_non_secret_error`,
which fails identically on untouched `main`. Pre-existing, not this plan's.

By the property asked for:

| property | test |
|---|---|
| a cookie sealed under key A opens under key B after restore | `keystore::a_cookie_sealed_by_one_mac_opens_on_another` |
| **the target's other rows are byte-identical afterwards** | `cookies::writing_a_value_leaves_every_other_column_of_every_row_untouched` (every column of every row compared), and again in the keystore round trip |
| an undecryptable row is skipped and the target's existing row survives | `keystore::a_cookie_that_will_not_decrypt_is_skipped_and_the_targets_row_survives` |
| a profile with no key refuses and changes nothing | `keystore::a_target_with_no_key_refuses_the_cookie_jar_and_leaves_it_alone`, `keystore::a_jar_with_no_key_or_no_readable_row_refuses_that_profile_only` |
| the category turned off carries none of it | `plan::switching_credentials_off_carries_no_store_at_all` (existing, covers every store) |
| identity is the whole index | `cookies::identity_is_the_whole_index_so_a_partitioned_twin_is_not_overwritten` |
| rows are never created or deleted | `cookies::a_carried_row_that_matches_nothing_is_counted_and_never_inserted`, `cookies::a_missing_jar_is_never_fabricated` |
| a write matching nothing refuses rather than claiming success | `cookies::a_write_that_matches_nothing_at_all_refuses_and_changes_nothing` |
| an unreadable jar is never read as "no session" | `cookies::a_jar_that_is_not_one_is_an_error_and_never_an_empty_reading` |
| a malformed payload refuses before the database is opened | `keystore::a_payload_that_is_not_a_jar_refuses_before_the_database_is_opened` |
| the domain prefix survives the crossing | asserted inside `a_cookie_sealed_by_one_mac_opens_on_another` |
| a traversing profile name is not a store, in **both** wire spellings | `keystore::a_profile_name_that_is_not_one_plain_directory_name_is_not_a_store` |
| every profile's jar is enumerated, and a profile without one is not | `keystore::every_profile_and_slot_on_disk_is_enumerated` |
| files land before stores even inside one category | `write::the_cookie_jar_lands_before_its_store_even_when_both_are_credentials` |
| no message carries a value | `cookies::no_message_on_any_arm_carries_a_cookie_value` |

### Negative controls — run after committing, each reverted

| break | test that failed |
|---|---|
| delete the `sort_by_key` that puts files first | `the_cookie_jar_lands_before_its_store_even_when_both_are_credentials` |
| match on `(host_key, name, path)` — the brief's guess | `identity_is_the_whole_index_so_a_partitioned_twin_is_not_overwritten` |
| write the plaintext instead of re-sealing under the target key | `a_cookie_sealed_by_one_mac_opens_on_another` |

## Production call sites — every symbol added, enumerated

Asked for explicitly, because this milestone has shipped thirteen instances of
tested code nothing calls. Counted over **production code only** (`#[cfg(test)]`
regions and comments stripped, definitions excluded). **No symbol has zero.**

| symbol | sites | reached from |
|---|---|---|
| `safe_storage::looks_like_raw` | 2 | `decrypt_raw`, `keystore::cookies_read` |
| `safe_storage::decrypt_raw` | 2 | `safe_storage::decrypt`, `keystore::cookies_read` |
| `safe_storage::encrypt_raw` | 2 | `safe_storage::encrypt`, `keystore::cookies_write` |
| `cookies::read_sealed` | 1 | `keystore::cookies_read` → `machine_read` → `Stores::read` → `plan::build`, `packer::build` |
| `cookies::has_rows` | 1 | `machine_has` → `Stores::has` → `report::count_stores` → `sync status` |
| `cookies::write_sealed` | 1 | `keystore::cookies_write` → `machine_write` → `Stores::write` → `restore::write::write_store` |
| `cookies::{opening,reading,running}` | 2 / 11 / 5 | the error arms of the three verbs above |
| `keystore::cookie_jar` | 4 | `desktop_caches`, `machine_has`, `cookies_read`, `cookies_write` |
| `keystore::cookies_read` / `cookies_write` | 1 each | `machine_read` / `machine_write` |
| `keystore::cookies_label` | 8 | every message arm of the two above |
| `Store::DesktopCookies` | 11 | `manifest_path`, `from_manifest_path`, `describe`, `read_failure_is_fatal`, `writable`, `machine_writable`, `desktop_caches`, `machine_read`, `machine_has`, `machine_write` |

`safe_storage::{decrypt,encrypt}` keep their existing callers; they are now thin
wrappers over the raw pair, so the base64 framing and the raw framing share one
AES path.

## What still needs a sign-in on the second Mac

- **Claude Code under a `CLAUDE_CONFIG_DIR`-scoped account.** Unchanged ceiling
  from 6-10: it lives under a per-account Keychain service name, and a
  bundle-chosen account name reaching a service-name hash wants its own
  validation.
- **A target that has never run Claude Desktop.** There is no Safe Storage key
  to seal with and no jar to write into. Both refuse in the planner and say so:
  install and sign in once, then restore again. Nothing is fabricated.
- **Cloudflare's `cf_clearance` / `__cf_bm`** are re-issued on the first
  request rather than resumed. Expected, not a gap.
- **Quit Claude Desktop before restoring into it.** A running app holds the jar
  in memory and may write it back over ours when it next persists. The write
  takes SQLite's lock with a five-second busy timeout and refuses that one item
  if the app holds it, but it cannot stop a running app overwriting afterwards.
  Every error on this path says "(is Claude Desktop running?)".

## Deviations from plan

**[Rule 1 — bug] The brief's cookie identity was wrong, and using it would have
corrupted a jar.** `(host_key, name, path)` is not unique on a real Claude
Desktop cookie table; three distinct partition combinations exist on the user's
machine. Corrected to the full seven-column `cookies_unique_index`, read off the
schema rather than assumed, with a test that fails against the three-column
version.

**[Rule 2 — missing critical functionality] The plaintext is not the value.**
Chromium prefixes every cookie plaintext with `SHA256(host_key)` and re-checks
it on read. Carrying "the value" would have produced 108 cookies Chromium
silently drops. Fixed by carrying the plaintext whole.

**[Rule 2] `write_sealed` refuses a call where nothing matched.** The first
version committed and returned `Ok`, which would report a restored session where
none landed — the precise failure mode this plan exists to end.

**[Rule 3 — blocking] The cookie column is not base64.** `safe_storage` only
spoke the base64 framing that `config-tokenCache*` uses; a `Cookies` row stores
the `v10` blob raw. Rather than base64-encode a blob to decode it again, the
transform was split into `{decrypt,encrypt}_raw` with the base64 pair as
wrappers — one AES path, both framings, no behaviour change for existing callers.

## Commits

| commit | what |
|---|---|
| `e7aba3a` | `claude_desktop::cookies` row-level read/has/write, and `safe_storage`'s raw framing |
| `29aca7e` | `Store::DesktopCookies` — the decrypt-out / re-seal-in transform and its enumeration |
| `bd68c94` | the jar lands before its store even when both halves are `Credentials` |

## Known Stubs

None.

## Self-Check: PASSED
