---
phase: 6
plan: 10
subsystem: sync/keystore
tags: [security, credentials, macos, keychain, push, restore, portability]
status: complete
requires: [anthropic/keychain, safe_storage, sync/plan, sync/push/packer, sync/restore/merge, sync/restore/write]
provides:
  - sync::keystore (Store, Stores, Fixture, PREFIX)
  - SyncRoots::stores
  - Disposition::ReplacesLiveCredential
affects:
  - src/sync/keystore.rs
  - src/sync/mod.rs
  - src/sync/scope.rs
  - src/sync/plan.rs
  - src/sync/push/packer.rs
  - src/sync/restore/mod.rs
  - src/sync/restore/merge.rs
  - src/sync/restore/write.rs
  - src/sync/restore/layout.rs
  - src/sync/restore/report.rs
  - src/sync/cli.rs
---

# Phase 6 Plan 10: A Credential That Is Not A File Is Read, Not Copied

## Signature changes, at the top because they are the thing to read first

**One struct gained a public field.** `SyncRoots::stores: keystore::Stores`
(`src/sync/mod.rs`). `SyncRoots::at` — the seam every test in the crate
constructs — fills it with an empty injected fixture; `SyncRoots::resolve`, the
one production wrapper, is the only place `Stores::Machine` is built. **No
existing call site changed**, which is the whole reason the seam was hung there
rather than threaded through `plan::build`, `packer::build`, `merge::plan` and
`write::apply` as a parameter (about forty edits, and four more chances to
forget one).

**One public enum gained a variant.** `Disposition::ReplacesLiveCredential`
(`src/sync/restore/mod.rs`). Unit variant, `writes()` is `false` for it, and
both exhaustive matches — `report::facing` and `report::mtimes` — were extended.
It also joins the two `matches!` filters that already handled
`NeedsCredentialConfirm`: the CLI's consent gate (`cli.rs`) and `write::apply`'s
preflight tripwire.

**One new module**, `src/sync/keystore.rs`: `PREFIX`, `Store`, `Stores`,
`Fixture`.

**One deletion**: nothing. **One removal during the plan**: `SyncRoots::with_stores`
was written, found to have zero callers, and deleted before it shipped.

`Cargo.toml` and `Cargo.lock` are byte-identical. No dependency added.

## What a user gets, and does not get, on a second Mac

**Gets:** the Claude Code login. `ai-usagebar` reports Anthropic usage on the
second Mac immediately after a `sync pull --apply`, with no `claude` login step,
because the OAuth credential that lives in the first Mac's login Keychain is now
read on push and written into the second Mac's Keychain on restore — byte for
byte, asserted end to end through a real push, a served bundle and a real
`restore::run`.

**Also gets, and did not before:** `~/.claude/.credentials.json` where Claude
Code writes a file rather than a Keychain item (Linux, and older macOS builds).
Nothing collected it. `scope`'s `Credentials` arm does now.

**Does not get:** the Claude Desktop session. `desktop-profiles/*/config-tokenCache{,V2}`
still travels and is still refused on arrival by 6-09's `ForeignSafeStorage`
gate. The second Mac's Claude Desktop needs one sign-in. Everything else in the
profile store — `meta.json`, the whole `desktop-state/` tree, the routines, the
chat index — restores as it always did.

## Decision 1: the Desktop token cache stays refused

**Chosen: refused.** Not decrypted-and-re-encrypted.

The re-encryption is not hard — `safe_storage::decrypt` on push,
`safe_storage::encrypt` on restore, both already written, both already
macOS-gated. Three things decided it the other way:

1. **It buys a sign-in button.** Claude Desktop is a GUI app with a "Sign in"
   button on its first screen. The Claude Code credential is different in kind:
   it is what *this tool* reads to render the thing the user installed it for,
   and its absence is the defect the plan opens with. The two are not the same
   size of loss and do not justify the same size of risk.

2. **It would put a bundle-chosen path component in front of a Keychain key.**
   The profile name in `desktop-profiles/<profile>/config-tokenCacheV2` comes
   from the manifest, and the manifest comes from a remote the threat model
   treats as fully hostile (D5). As a *file* that component already passes
   through `layout::from_manifest_path`'s eight refusals. As a **store** it
   would have to be validated again, separately, in a second place — exactly the
   "two copies of one rule" shape this module tree has been bitten by twice
   (F-1's case folding, F-8's hand-maintained guard list). The Claude Code store
   avoids it completely: its wire path is a compile-time literal with no
   component from the bundle at all.

3. **It doubles the plaintext credential surface for that.** The bundle now
   carries the CLI's live OAuth token in plaintext (inside end-to-end
   encryption, alongside the `.credentials.json` it already carried — no new
   exposure *class*, but a real concentration of value). Adding Desktop's
   session material too is a second live secret for a button.

The refusal is already shipped, already tested, and already says what to do
about it in the report. And the upgrade path is open and costs nothing now: a
`keystore/desktop-token-cache/<profile>` entry is additive, and **an unknown
`keystore/…` entry is refused rather than fatal by every build**, including the
ones already in the field — asserted by
`an_unknown_store_is_refused_and_leaves_the_local_one_untouched`.

## Decision 2: a different live login is never replaced without being asked

The worst outcome in this milestone is silently swapping the account the tool
exists to report on. So `decide_store` answers four ways, and none of them is
"overwrite quietly":

| this machine's store holds | consent | disposition |
|---|---|---|
| nothing | — | `Create` |
| the same credential | — | `SkipIdentical` |
| a **different** credential | none | `ReplacesLiveCredential` |
| a **different** credential | `--force-credentials` | `Update` |
| an unknown store, or one this build cannot write | — | `ExcludedByPolicy` |

Three things about that table are deliberate.

**Identity is a digest, not a clock.** A Keychain item has no mtime this side
can compare, so "is the local one newer?" has no honest answer and is not
pretended to. The question asked instead is the one that *can* be answered —
are these the same bytes? — by hashing what the store holds with the same
`Keys::chunk_id` the push side used. It needs no data pack, so a dry run reports
exactly what the applying run will do (unlike 6-09's gate, which cannot see
inside a dry run). And a repeated pull onto the machine that pushed is silent.

**`--force` alone does not promote it; `--force-credentials` does, on its own.**
`--force` means "overwrite something newer", and there is nothing newer here for
it to mean that about. The plan's non-negotiable — *`--force` alone must never
overwrite a live credential; that needs `--force-credentials` too* — holds
exactly: `--force` alone leaves it refused. Requiring **both** would have broken
the interactive path, where answering the CLI's prompt sets only
`force_credentials` and a re-plan then has to promote; the item would loop back
into `write::apply`'s tripwire and hard-abort a restore the user had just
consented to.

**A read failure is not "there is nothing here."** A locked Keychain read as an
empty one would make the next branch call a live credential absent and `Create`
over it without asking. `Stores::read` returns `Result<Option<_>>` and
`decide_store` maps `Err` to `ReplacesLiveCredential` — the refusing direction.
The same rule runs on the push side, where a locked Keychain is an error rather
than a bundle that silently omits the credential.

### The one thing a user loses that a file restore would not

**The pre-restore backup cannot archive a Keychain item.** `restore::run`
archives *destinations*, and a store has none; the only place to put one is a
plaintext file on disk, which is the thing this design refuses to create. So the
protection is the consent rather than the archive, and the report says so in
those words rather than leaving it to be discovered:

```
  NEEDS YOUR CONFIRMATION — this machine already has a different login here,
  and it is not archived by the pre-restore backup     (--force-credentials)
```

## The wire spelling, and why it can never become a path

A store's manifest entry is `keystore/claude-code-oauth` — a complete
compile-time literal. `keystore` is deliberately **absent** from
`layout::ROOT_PREFIXES`, so `from_manifest_path` refuses it with "it names a
root this build does not know". That refusal is the property that matters: **a
synthetic entry that landed as a file would be a live OAuth token written in
plaintext under the user's home directory.**

It is defended three times over, and each is a test:

- `merge::decide_entry` intercepts `keystore/…` **before** anything resolves a
  path, and every store keeps `dest: None`, so there is structurally nowhere for
  one to be written.
- `write::apply` routes on a `Target` enum — `File(PathBuf)` or `Store(Store)` —
  so a store cannot acquire a destination further down the call chain.
- `layout::from_manifest_path` refuses the prefix anyway, for this build's
  spelling, a later build's, `keystore/../config/config.toml`, and bare
  `keystore`.

The end-to-end test asserts the negative directly: after a full apply, no file
anywhere under the restorer's roots contains the token.

## Fail-closed on the write

`write_store` reassembles the whole value into a `Zeroizing<Vec<u8>>`,
length-checks it against the manifest's `true_len`, and UTF-8-checks it —
**all before `Stores::write` is called**. `Stores::write` replaces the whole
value or fails; there is no truncate-then-fill and no read-modify-write
anywhere on the path. So every failure mode leaves the credential that was there
exactly as it was, and two tests assert that on the *store contents*, not on the
disposition: a disagreeing length, and bytes that are not valid UTF-8.

## Hermeticity: why no test can touch the real login Keychain

The AUR `check()` runs `cargo test` during `makepkg`, so a test that wrote the
real `Claude Code-credentials` item would clobber an installer's Claude login at
install time — which a previous plan in this milestone did, and the user deleted
the item by hand.

`Stores::Machine` is the only door to a real Keychain, and it is opened in
exactly one production place. That is not a convention:
`the_machine_store_is_constructed_in_exactly_one_place` walks every `.rs` file
under `src/sync` through the shared `guard::rs_files_in` + `production_code`
helpers, and fails if any file outside `{sync/mod.rs, sync/keystore.rs}` names
it. A guard that enumerates what to check fails open on everything written after
it; this one walks and fails closed. A test would have to *name* `Stores::Machine`
to reach a Keychain, and naming it fails a test.

The platform arm of `Stores::writable` was pulled out into `machine_writable`
for exactly this reason: it is asserted directly against
`cfg!(target_os = "macos")` without any test constructing a `Machine`.

`merge::plan`'s Safe Storage key now comes from the same injected `Stores`, so
there is one rule for every machine-bound secret rather than two. 6-09's own
tests all drive `plan_with_safe_key` and therefore never asserted that `plan` —
the entry `restore::run` actually calls — reaches a key at all; a regression
there would have disabled that gate while every test stayed green.
`plan_takes_its_safe_storage_key_from_the_injected_stores` closes it.

## Secret discipline

- `Stores` has a **hand-written `Debug`** that prints `Stores::Fixture(<redacted>)`.
  `SyncRoots` is `Debug`, and `RestoreCtx` holds one, so a derived `Debug` would
  have printed a live OAuth token into any `{:?}` of a restore context.
  `debug_never_renders_a_stored_credential` asserts it.
- `Zeroizing` throughout: `Stores::read`'s return, the `Fixture`'s stored values,
  `write_store`'s reassembly buffer. `Fixture` has no `Debug` at all.
- **Nothing formats, logs or prints a value.** The only error strings on these
  paths carry `Store::describe()` (a compile-time literal), byte *lengths*, and
  the manifest path — which is itself a compile-time literal for a store.
- **`sync::guard`'s environment-read rule:** `keystore.rs` contains no
  `std::env`, `env::var`, `var_os`, `clap` or `Arg::new`, so it passes
  `no_password_input_path_reads_the_process_environment` unexempted. It reaches
  `$USER` only indirectly, through `anthropic::keychain::account()`, which is
  outside `src/sync` and is the account selector the read and write paths must
  agree on — pre-existing, and the reason that function is shared rather than
  copied.

  ponytail ceiling, recorded rather than hidden: `keychain::read_raw` hands back
  a plain `String`, so one un-zeroized copy exists inside it before this module
  wraps the value. Narrowing that is a signature change across `creds.rs`,
  `cli_account.rs` and the widget; everything *this* module holds is `Zeroizing`.

## The change-detection decision, and the bug it avoids

A store gets **no index row and no D5 short-circuit**. It has no
`(size, mtime_ns, inode)` key, and the nearest available thing — the value's
length — would declare a *rotated* OAuth token unchanged, because a fresh token
is the same shape as the one it replaced. That single mistake would have made
the whole feature pointless: the bundle would carry the first token forever.
`a_rotated_token_of_identical_length_is_planned_as_new_bytes` pins it.

The cost is a few hundred bytes re-hashed per run. It is not re-*uploaded*: the
packer's existing two-halved dedup (already published, or already packed this
run) recognises an unchanged credential's chunk, so an idle push still sends
nothing for it. `plan.files_opened` is untouched, so SYNC-02's "a no-op sync
opens nothing" evidence still means exactly what it says.

The packer re-reads the store rather than carrying the plan's bytes down, which
is this module's own stated rule — *the manifest describes what the packer
sealed, not what the planner predicted*. A credential that rotated in between is
sealed as it now is; one that vanished is simply not in the snapshot, never a
zero-length entry and never a chunk id nothing sealed. Both are tested.

## Opt-in and the private-repo gate

Stores ride under `SyncCategory::Credentials` and are read **only** when
`cfg.includes(Credentials)`. That check is in `plan::build` and is load-bearing
rather than belt-and-braces: `scope::collect` is what enforces the switch for
*files*, and it returns early before the category loop body ever runs, so a
store read trusting the loop alone would carry a live OAuth token into a bundle
whose owner had switched credentials off.

Putting them under `Credentials` is also what keeps D-04 honest:
`gate::assert_pushable`'s `credentials_in_bundle` argument is
`config.sync.includes(SyncCategory::Credentials)`, so a public repository still
refuses a bundle carrying one, naming rotation. `~/.claude/.credentials.json`
went into the same category for the same reason.

Both directions agree on the category: `merge::category_of` maps
`keystore/…` and `claude-home/.credentials.json` (case-folded, through
`CREDENTIAL_FILE`) to `Credentials`, so the restore report does not file an item
under a category its owner never switched on.

## Production call sites — every added function, and none with zero

| Added | Production caller(s) |
|---|---|
| `keystore::PREFIX` | `Store::is_store_path`, `merge::category_of` |
| `Store` / `Store::ALL` | `plan::build`, `Store::from_manifest_path` |
| `Store::manifest_path` | `packer::build`, `plan::plan_store`, `Store::from_manifest_path` |
| `Store::from_manifest_path` | `packer::build`, `merge::decide_store`, `write::apply` |
| `Store::is_store_path` | `merge::decide_entry` |
| `Store::describe` | `write::write_store` (×2), `keystore::machine_write` |
| `Fixture::get` | `Stores::read` |
| `Fixture::set` | `Stores::write` |
| `Fixture::set_safe_key` | *tests only* — kept, see below |
| `Stores::fixture` | `SyncRoots::at` |
| `Stores::edit` | `Stores::{read, write, safe_key}` |
| `Stores::writable` | `merge::decide_store` |
| `Stores::read` | `plan::build`, `packer::build`, `merge::decide_store` |
| `Stores::write` | `write::write_store` |
| `Stores::safe_key` | `merge::plan` |
| `keystore::machine_{read,write,writable,safe_key}` | `Stores::{read,write,writable,safe_key}` |
| `SyncRoots::stores` | all of the above |
| `plan::plan_store` | `plan::build` |
| `packer::pack_store` | `packer::build` |
| `merge::decide_store` | `merge::decide_entry` |
| `merge::store_chunk_ids` | `merge::decide_store` |
| `write::Target` | `write::apply` |
| `write::write_store` | `write::apply` |
| `Disposition::ReplacesLiveCredential` | produced by `merge::decide_store`; consumed by `report::facing`, `report::mtimes`, `Disposition::writes`, `cli`'s consent filter, `write::apply`'s preflight |

**`Fixture::set_safe_key` is the one entry with no production caller, and it is
deliberate.** A `Fixture` is test-only data by definition — it is the injected
half of the seam — and this is the setter for the field `Stores::safe_key`
returns in production. It exists because without it nothing could assert that
`merge::plan` reads the *injected* key, which is the regression that would
silently disable 6-09's gate. `Fixture::get` and `Fixture::set` sit beside it
and *are* reached from production, through `Stores::read` and `Stores::write`.

`SyncRoots::with_stores` was written, found to have zero callers once the tests
settled on `roots.stores.edit()`, and deleted in commit `41ba0c5` rather than
shipped as tested code nothing calls.

## Tests — 28 added, 1785 passing, 0 failing (baseline 1757)

All hermetic: `TempDir`, injected roots, injected stores, fixed timestamps, no
network, no wall clock, **and no real login Keychain anywhere.**

| Where | Test | Asserts |
|---|---|---|
| `keystore` | `every_store_has_its_own_fixed_wire_name_under_the_prefix` | no bundle-chosen component; no two stores collide |
| | `an_unknown_keystore_entry_is_a_store_path_with_no_store` | forward compatibility; `keystores/` is not the prefix |
| | `a_fixture_round_trips_a_value_and_shares_it_across_clones` | the seam works through a `SyncRoots` clone |
| | `a_second_write_replaces_the_value_rather_than_merging_it` | never a splice of two credentials |
| | `debug_never_renders_a_stored_credential` | the hand-written `Debug` |
| | `a_store_is_writable_exactly_where_this_platform_has_one` | the platform arm, without a `Machine` |
| | `the_machine_store_is_constructed_in_exactly_one_place` | **the hermeticity guard** |
| `scope` | `claude_codes_own_credential_file_is_collected_with_the_credentials` | the file half, which nothing collected |
| | `claude_codes_credential_file_is_left_behind_when_credentials_are_off` | and only when switched on |
| `plan` | `a_keychain_login_is_planned_under_the_stores_wire_name` | wire name, never a path; `files_opened` still 0 |
| | `switching_credentials_off_carries_no_store_at_all` | **the opt-in** |
| | `a_rotated_token_of_identical_length_is_planned_as_new_bytes` | the length-key bug that would break everything |
| | `a_machine_with_no_login_contributes_no_entry` | absent and empty both contribute nothing |
| `packer` | `a_stores_manifest_entry_is_its_wire_name_and_its_bytes_are_sealed` | plaintext read back through the format's own readers |
| | `a_credential_that_rotated_since_planning_is_sealed_as_it_now_is` | the packer is the authority |
| | `a_credential_that_vanished_since_planning_is_simply_not_in_the_snapshot` | no zero-length entry, no unsealed id |
| `layout` | `a_machine_bound_stores_wire_name_never_resolves_to_a_place_on_disk` | **four spellings, all refused** |
| `merge` | `a_machine_with_no_login_receives_one_byte_for_byte` | `Create`, `dest: None`, exact bytes |
| | `the_same_login_already_here_is_identical_and_silent` | digest-first; a repeat pull asks nothing |
| | `force_alone_never_replaces_a_different_live_login` | **`--force` alone does not**; report names it and the missing archive; write half refuses; login survives |
| | `force_credentials_replaces_the_live_login_and_nothing_else_does` | the gate widened, not jammed |
| | `an_unknown_store_is_refused_and_leaves_the_local_one_untouched` | forward compatibility, end to end |
| | `a_length_that_disagrees_refuses_the_item_and_keeps_the_existing_login` | **never half-write** |
| | `a_value_that_is_not_utf8_is_refused_rather_than_repaired` | same, for bytes that are not a credential |
| | `a_store_and_the_credential_file_are_both_filed_under_credentials` | push and restore agree on the category |
| | `every_machine_in_this_suite_has_an_injected_store` | the seam, asserted rather than assumed |
| | `plan_takes_its_safe_storage_key_from_the_injected_stores` | the 6-09 wiring nothing tested |
| `restore` | `a_keychain_login_pushed_on_one_mac_arrives_byte_for_byte_on_the_other` | **the headline**: real push → served bundle → real `restore::run` → exact bytes in the store, and **not one byte on disk** |

`push_one_file` was refactored into a shared `push_bundle` + `file_plan_for`
so `push_login` reuses the real push path rather than owning a second copy of it.

### Negative controls (run after committing, per the plan's warning)

Three, each reverted with `git restore` against a clean tree:

| Break | Result |
|---|---|
| Drop `cfg.includes(category)` from `plan`'s store pass | `switching_credentials_off_carries_no_store_at_all` FAILED |
| Drop `merge::decide_entry`'s `keystore/…` interception | **8 tests FAILED**, including the end-to-end one |
| Add `("keystore", claude_home)` to `layout::ROOT_PREFIXES` | 2 tests FAILED, including the prefix-agreement guard |

## Verification

- `cargo test` — **1785 passed, 0 failed** (baseline 1757 + 28).
- `cargo clippy --all-targets -- -D warnings` — clean.
- `cargo fmt --check` — clean.
- `make test` — GNOME, KDE and Omarchy contract suites pass.
- `Cargo.toml` / `Cargo.lock` byte-identical; no dependency added.
- No Swift touched, so `./macos/run-tests.sh` was not run.
- **Linux:** no cross-compilation target installed (out of scope, as in 6-09).
  Verified the same way instead — every `target_os = "macos"` predicate under
  `src/` flipped to a value that never matches, then `cargo check`. The library
  builds clean, and `keystore.rs` produced no diagnostic at all. `--all-targets`
  reproduces only 6-09's two pre-existing, unrelated errors (`sync::github::keychain`
  and its `clear`, imported by `tests/live.rs`). Tree restored and re-checked
  green afterwards.
- All tests run with `< /dev/null` — `sync::cli` hangs on an inherited open pipe.

## Deviations from plan

Two, both small and both in the "auto-fix" family.

1. **[Rule 2 — missing critical functionality] `~/.claude/.credentials.json` was
   collected by nothing.** The plan's premise was that "Linux Claude Code writes
   a real file, which the existing collectors already handle." It does not:
   `scope`'s `Config` arm walks *ai-usagebar's* config dir, and the `claude_home`
   root is only reached for `scheduled-tasks/` and `projects/`. So the same
   defect — the CLI credential never reaches the bundle — existed on Linux for
   the opposite reason. Fixed at the root, one `push_path` in the `Credentials`
   arm, plus the `category_of` entry that keeps both directions agreeing.
   Commit `bd4489c`.

2. **[Rule 1 — bug] Five shell-escaped apostrophes in `merge.rs`'s prose.**
   6-09 wrote that file through a bash heredoc and left `'"'"'` where an
   apostrophe belonged, in five doc comments. One of them was inside a block this
   plan deletes; the other four were fixed in passing. Commit `c7718f9`. Prose
   only — no code, no behaviour.

## Deferred, and why it is not in scope here

**The D-04 gate's `credentials_in_bundle` reads only the `Credentials` switch.**
With `Config` on and `Credentials` off, `config/accounts/*/.credentials.json`
and a `config.toml` holding an inline `api_key` still go into the bundle, while
`assert_pushable` tells the user "the credentials category is off, so there is
nothing to leak" and clears a **public** repository with a warning. That is a
pre-existing hole, unrelated to this plan and not widened by it — every path this
plan adds is under `Credentials`, so the gate covers all of it. Logged to
`.planning/phases/06-surfaces-and-ship/deferred-items.md`; the fix is one
predicate (`includes(Credentials) || includes(Config)`), but it changes the
posture of a shipped security gate and wants its own plan.

## Known Stubs

None. Nothing in this plan is a placeholder, and no `<verify>` went unrun.
