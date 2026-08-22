---
phase: 02-bundle-scope-local-index-dry-run-planning
plan: 02
subsystem: infra
tags: [sync, filesystem-scan, claude-desktop, symlink-safety]

requires:
  - phase: 02-bundle-scope-local-index-dry-run-planning
    plan: 01
    provides: "`scope::walk` / `push_path` / `is_excluded`, `SyncRoots::at`, `CategoryScan`, and the pre-wired `collect` arms"
provides:
  - "`scope::collect(Credentials, …)` — per profile: meta.json, config-tokenCache, config-tokenCacheV2, desktop-state/**"
  - "`scope::collect(Routines, …)` — `~/.claude/scheduled-tasks/**` plus every `<account>/<org>/scheduled-tasks.json`"
  - "`scope::collect(ChatIndex, …)` — `claude-code-sessions/<account>/<org>/local_*.json`"
  - "`scope::subdirs` / `scope::account_org_dirs` — symlink-refusing directory enumeration (private to scope.rs)"
affects: [2-05-bundle-build, 2-07-dry-run]

tech-stack:
  added: []
  patterns:
    - "Enumerate by directory listing, never from meta.json — an uncaptured account still has its routines carried"
    - "`file_type` from `read_dir` (which does not traverse) is what decides a directory is enterable"
    - "Walk-then-retain-by-name, the shape the `config` arm already established, so the shared D2 predicate is on the path"

key-files:
  modified:
    - src/sync/scope.rs

key-decisions:
  - "`subdirs()` filters on `entry.file_type()`, not `path.is_dir()`. `is_dir()` follows symlinks, so a symlinked profile or account directory would have been entered and its real files collected — the file-level `symlink_metadata` guard in `push_path` would not have caught them."
  - "chat_index walks the sessions root and retains `local_*.json` rather than enumerating account/org and listing. Keeps the walk bounded by `MAX_WALK_ENTRIES`, keeps `walk_capped` reporting, and — the point of the plan's instruction — routes `local-agent-mode-sessions/` through the plan 2-01 predicate instead of a second rule here."
  - "routines enumerates account/org rather than walking the sessions root, so picking 4 registry files does not stat ~1300 session indexes a second time in the same `sync status` run."
  - "The claude-acc layout names (`meta.json`, `config-tokenCache`, `config-tokenCacheV2`, `desktop-state`, `claude-code-sessions`, `scheduled-tasks.json`) are restated as a private const block citing `crate::claude_desktop`, because they are private there and widening them would have edited a file this plan does not own."

requirements-completed: [SCOPE-01, SCOPE-02]

coverage:
  - id: D1
    description: "A profile yields exactly meta.json, both token caches and the whole desktop-state tree, and nothing else from the profile store"
    requirement: SCOPE-01
    verification:
      - kind: unit
        ref: "src/sync/scope.rs#a_profile_yields_its_meta_both_token_caches_and_the_whole_desktop_state_tree"
        status: pass
      - kind: unit
        ref: "src/sync/scope.rs#every_profile_is_collected_and_one_without_meta_json_does_not_fail_the_others"
        status: pass
    human_judgment: false
  - id: D2
    description: "bridge-state.json and ant-device-registry.json are absent even when seeded in a profile root and inside desktop-state/ (T-2-07)"
    requirement: SCOPE-01
    verification:
      - kind: unit
        ref: "src/sync/scope.rs#bridge_state_and_the_device_registry_never_leave_a_profile"
        status: pass
    human_judgment: false
  - id: D3
    description: "backups/, prelogin-backup/ and hidden/ contribute nothing, beside the store or inside it (T-2-08)"
    requirement: SCOPE-01
    verification:
      - kind: unit
        ref: "src/sync/scope.rs#rollback_state_beside_and_inside_the_profile_store_contributes_nothing"
        status: pass
    human_judgment: false
  - id: D4
    description: "Routines collect ~/.claude/scheduled-tasks/** recursively plus every account's scheduled-tasks.json registry"
    requirement: SCOPE-01
    verification:
      - kind: unit
        ref: "src/sync/scope.rs#routines_take_the_claude_home_tree_and_every_account_registry"
        status: pass
    human_judgment: false
  - id: D5
    description: "chat_index takes local_*.json from every account/org and no sibling, and a registry lands in routines only — never double-counted"
    requirement: SCOPE-01
    verification:
      - kind: unit
        ref: "src/sync/scope.rs#the_chat_index_takes_local_session_files_from_every_account_and_nothing_else"
        status: pass
      - kind: unit
        ref: "src/sync/scope.rs#an_account_registry_lands_in_routines_and_never_in_the_chat_index"
        status: pass
    human_judgment: false
  - id: D6
    description: "A local-agent-mode-sessions/ tree anywhere under the sessions root contributes nothing (T-2-09)"
    requirement: SCOPE-01
    verification:
      - kind: unit
        ref: "src/sync/scope.rs#a_cowork_local_agent_mode_sessions_tree_contributes_nothing"
        status: pass
    human_judgment: false
  - id: D7
    description: "A missing profile store, routines tree or sessions root is an empty scan, not an error"
    requirement: SCOPE-01
    verification:
      - kind: unit
        ref: "src/sync/scope.rs#a_missing_profile_store_is_an_empty_scan_not_an_error"
        status: pass
      - kind: unit
        ref: "src/sync/scope.rs#a_missing_routines_tree_is_an_empty_scan_not_an_error"
        status: pass
      - kind: unit
        ref: "src/sync/scope.rs#a_missing_sessions_root_is_an_empty_chat_index_scan"
        status: pass
    human_judgment: false
  - id: D8
    description: "Unchecking a category removes it from the scan without touching the filesystem for it"
    requirement: SCOPE-02
    verification:
      - kind: unit
        ref: "src/sync/scope.rs#unchecking_credentials_scans_nothing_even_with_a_full_profile_store"
        status: pass
    human_judgment: false
  - id: D9
    description: "A symlinked profile or account directory is not entered (T-2-01, extended to the new enumerator)"
    requirement: SCOPE-01
    verification:
      - kind: unit
        ref: "src/sync/scope.rs#a_symlinked_profile_or_account_directory_is_not_entered"
        status: pass
    human_judgment: false

duration: 20min
completed: 2026-08-19
status: complete
---

# Phase 2 / Plan 02: credentials, routines and chat_index collectors Summary

**`scope::collect` now returns real, D1-accurate scans for four of the five categories, with D2's two dangerous files proven absent from the trees they actually live in.**

## Performance

- **Duration:** ~20 min
- **Tasks:** 2 of 2
- **Files modified:** 1 (`src/sync/scope.rs`)
- **Tests:** 24 in `sync::scope` (13 new), 136 in `sync::` — all green

## Accomplishments

- **credentials** — enumerates the profile store by directory listing, skips any profile
  without a readable `meta.json` (the same best-effort rule `load_profiles` already applies),
  and adds exactly the four D1 members: `meta.json`, `config-tokenCache`,
  `config-tokenCacheV2`, and `desktop-state/` walked recursively. Four seeded profiles is the
  test case, not two — that is this user's real setup.
- **routines** — `~/.claude/scheduled-tasks/**` plus every `<account>/<org>/scheduled-tasks.json`,
  with account and org levels found by listing rather than from any profile's metadata, so an
  account that was never captured still has its routines carried.
- **chat_index** — `claude-code-sessions/<account>/<org>/local_*.json`, name-filtered off the
  shared walker. No file body is read.
- **The D2 exclusions bite where it matters.** `bridge-state.json` and
  `ant-device-registry.json` are seeded both in a profile root and inside `desktop-state/`
  and are absent from the result; `backups/`, `prelogin-backup/` and `hidden/` contribute
  nothing whether they sit beside the store or inside it; a `local-agent-mode-sessions/`
  tree at either the account or the org level contributes nothing. None of this is
  re-implemented here — every candidate reaches plan 2-01's `is_excluded`, which is precisely
  what the tests prove.

## Task Commits

1. **Task 1: credentials and routines collectors** — `9f6dd93` (feat)
2. **Task 2: chat_index collector** — `2cf6f7c` (feat)

## Decisions Made

- **`subdirs()` filters on `entry.file_type()`, not `path.is_dir()`.** This is the one
  non-obvious line in the plan. `is_dir()` follows symlinks, so a symlinked profile or
  account directory would have been *entered*, and the real files inside it would have passed
  `push_path`'s `symlink_metadata` check — the file-level guard only refuses a link that is
  itself the collected file. `file_type()` comes from `read_dir` and does not traverse, which
  is the same rule `walk` already uses. `a_symlinked_profile_or_account_directory_is_not_entered`
  is the test that fails if this regresses.
- **chat_index walks and retains; routines enumerates.** They look asymmetric on purpose.
  chat_index wants every `local_*.json` under the root, so the bounded walker (which also
  gives it `walk_capped` and the component-level Cowork rule for free) is the right tool.
  routines wants 4 named files out of a ~1300-file tree, so walking it a second time in the
  same `sync status` run to stat 1300 entries for 4 hits would be waste; a two-level listing
  finds them directly.
- **The claude-acc layout names are restated, not imported.** `META_JSON`, `TOKEN_CACHE`,
  `TOKEN_CACHE_V2`, `DESKTOP_STATE`, `SESSIONS_DIR` and `SCHEDULED_TASKS` are private in
  `crate::claude_desktop` / `claude_desktop::merge`. Widening them would have edited files
  this plan does not own while 2-03 and 2-04 run in parallel, so scope.rs carries a private
  const block that cites the module it mirrors — better than a bare literal at each use site.
- **No second exclusion check and no second symlink check were added.** Every direct-file add
  goes through `push_path`, which already applies `is_excluded` and refuses a symlink.

## Deviations from Plan

None. Both tasks landed as specified, in the file the plan names and no other.

One thing worth flagging as *additive rather than deviating*: the plan's behaviour list did
not call out symlinked profile/account **directories** — only the file-level symlink rule
plan 2-01 already proved. Adding the enumerator opened that door, so it was closed in the
same commit and given its own test.

## Issues Encountered

None.

## Security notes carried forward

- **No new object is sealed under `chunk_key`.** This plan only reads directory metadata, so
  the deferred Phase 1 NEW-3 AAD object-type separator is still not triggered.
- **No unbounded id list is read before its container authenticates.** The new enumerator is
  two levels deep by construction; the walks it feeds remain capped by `MAX_WALK_ENTRIES`.
- **No file body is read by any of the three collectors** — a `stat` per entry is the whole
  cost, and no credential's contents ever enter the process.
- T-2-07, T-2-08, T-2-09 and T-2-10 all have a passing test; T-2-SC is unchanged (no
  dependency added).

## User Setup Required

None.

## Next Phase Readiness

- `sync status` now reports real counts for `config`, `credentials`, `routines` and
  `chat_index`; `transcripts` still reports `off` until plan 2-04 fills
  `transcripts::collect_bounded`.
- **CAL-2** (does Claude Desktop's LevelDB compaction rewrite the 24 MB profile wholesale?)
  is now *measurable* — the credentials collector returns the `desktop-state/` file set with
  size and mtime, which is the input that measurement needs. Still open.
- **CAL-4** (real compressed bundle size) needs 2-05's bundle build.

---
*Phase: 02-bundle-scope-local-index-dry-run-planning*
*Completed: 2026-08-19*
