---
phase: 2-bundle-scope-local-index-dry-run-planning
plan: 02
type: execute
wave: 2
depends_on: ["2-01"]
files_modified:
  - src/sync/scope.rs
autonomous: true
requirements: [SCOPE-01, SCOPE-02]
user_setup: []

must_haves:
  truths:
    - "The credentials category collects each profile's meta.json, config-tokenCache, config-tokenCacheV2 and the whole desktop-state tree, and nothing else from the profile store."
    - "The routines category collects `~/.claude/scheduled-tasks/**` plus every account's scheduled-tasks.json registry."
    - "The chat_index category collects `claude-code-sessions/<account>/<org>/local_*.json` and does not collect the sibling scheduled-tasks.json twice."
    - "bridge-state.json and ant-device-registry.json are absent from every category's scan even when seeded inside a profile."
    - "Unchecking a category in config removes it from the scan without touching the filesystem for it."
  artifacts:
    - src/sync/scope.rs
  key_links:
    - "All three collectors call the plan 2-01 `walk`, so D2's exclusions and the symlink guard apply without being restated."
    - "Profile enumeration reuses the layout `claude_desktop::load_profiles` already reads, so the two tools stay consistent about what a profile is."
---

<objective>
Fill in the three remaining always-on categories from D1 — credentials, routines and
chat_index — on top of the walker plan 2-01 proved.

Purpose: these are the categories that carry the user's four Claude Desktop accounts. D1
names them exactly; this plan implements that mapping and proves the D2 exclusions actually
bite on the trees where the dangerous files live.
Output: `scope::collect` returns real scans for four of the five categories.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/2-bundle-scope-local-index-dry-run-planning/2-CONTEXT.md
@.planning/phases/2-bundle-scope-local-index-dry-run-planning/2-01-SUMMARY.md
@CLAUDE.md
@src/claude_desktop/mod.rs
</context>

<tasks>

<task type="auto" tdd="true">
  <name>Task 1: credentials and routines collectors</name>
  <files>src/sync/scope.rs</files>
  <read_first>src/claude_desktop/mod.rs lines 37-60 (the `CONFIG_JSON` / `SESSIONS_DIR` / `TOKEN_CACHE` / `DESKTOP_STATE` / `BRIDGE_FILE` / `DEVICE_REGISTRY` consts — the authoritative names, do not retype them from memory) and lines 143-196 (`ProfileMeta` / `load_profiles`, the profile-store layout).</read_first>
  <behavior>
    - A profile directory containing meta.json, config-tokenCache, config-tokenCacheV2 and a desktop-state/ subtree yields exactly those files, with desktop-state walked recursively.
    - A second profile in the same store is collected too — the user has four accounts, so multi-profile is the normal case, not an edge case.
    - A profile whose meta.json is missing is skipped without failing the other profiles, matching how `load_profiles` already treats a mangled profile.
    - bridge-state.json and ant-device-registry.json seeded inside a profile and inside desktop-state/ are both absent from the result.
    - A `backups/` or `prelogin-backup/` sibling of the profile store contributes nothing.
    - `~/.claude/scheduled-tasks/<name>/` files are collected recursively.
    - Each `claude-code-sessions/<account>/<org>/scheduled-tasks.json` is collected by the routines category.
    - A missing profile store or a missing scheduled-tasks directory yields an empty scan, not an error.
  </behavior>
  <action>
Replace the `SyncCategory::Credentials` arm of `collect` with a real collector, per D1.
Enumerate immediate subdirectories of `roots.desktop_profiles_dir` — a profile is a
directory, as `claude_desktop::load_profiles` already defines it; skip any without a
readable meta.json so one hand-mangled profile cannot fail the other three. For each
surviving profile, add its meta.json, config-tokenCache and config-tokenCacheV2 as direct
files and `walk` its desktop-state directory. Use the filename constants from
`crate::claude_desktop` if they are public; if they are private, add a short private const
block here naming the same strings and cite the module it mirrors, rather than importing
by string literal at each use site.

Replace the `SyncCategory::Routines` arm, also per D1: `walk` `roots.claude_home.join("scheduled-tasks")`,
then add each account's registry — for every `<account>/<org>/` directory under
`roots.desktop_data_dir.join("claude-code-sessions")`, add that directory's
scheduled-tasks.json when present. Enumerate the account and org levels by directory listing
rather than by parsing meta.json, so an account whose profile was never captured still has
its routines carried.

Both collectors funnel every candidate through the plan 2-01 `walk`/`is_excluded` path; do
not add a second exclusion check and do not add a second symlink check. If a direct-file
add bypasses `walk`, run it through `is_excluded` explicitly before pushing.

Do not edit src/sync/mod.rs, src/sync/transcripts.rs, or any file outside src/sync/scope.rs —
plans 2-03 and 2-04 are executing in parallel against their own files.
  </action>
  <verify>
    <automated>cargo test --lib sync::scope</automated>
  </verify>
  <done>Every bullet in `&lt;behavior&gt;` for these two categories has a passing test that seeds its tree under a `TempDir` and injects it with `SyncRoots::at`. The four-profile case is exercised with at least two profiles.</done>
</task>

<task type="auto" tdd="true">
  <name>Task 2: chat_index collector</name>
  <files>src/sync/scope.rs</files>
  <behavior>
    - `claude-code-sessions/<account>/<org>/local_abc.json` is collected.
    - A sibling file that is not `local_*.json` — including scheduled-tasks.json — is not collected by this category.
    - Two accounts each with one org are both collected, keyed by their own directory names.
    - A `local-agent-mode-sessions/` directory anywhere under the sessions root contributes nothing, because a Cowork transcript's path embeds an unreconstructable account suffix and a copy renders as an empty chat.
    - A missing sessions root yields an empty scan.
  </behavior>
  <action>
Replace the `SyncCategory::ChatIndex` arm with the D1 mapping:
`claude-code-sessions/<account>/<org>/local_*.json`. Walk the sessions root and keep only
regular files whose file name starts with `local_` and ends in `.json`, at the
account/org depth. The `local-agent-mode-sessions/` exclusion is already carried by plan
2-01's component-level rule — assert it here rather than re-implementing it, so the test
proves the shared predicate is actually reached from this collector.

The user's measured tree is roughly 1300 of these files across four accounts, so the
collector must not read any file body; a `stat` per entry is the whole cost.
  </action>
  <verify>
    <automated>cargo test --lib sync::scope</automated>
  </verify>
  <done>Every bullet in `&lt;behavior&gt;` has a passing test. A test asserts that a seeded scheduled-tasks.json appears in the routines scan and not in the chat_index scan, so the two collectors do not double-count it.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| Claude Desktop profile store → collector | Files written by another tool, holding OAuth token caches for four accounts, cross into a set destined for another machine. |
| sessions tree → collector | Directory names are account and org UUIDs supplied by a remote service. |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|-----------|----------|-----------|----------|-------------|-----------------|
| T-2-07 | Information disclosure | credentials collector | critical | mitigate | bridge-state.json and ant-device-registry.json are rejected by the plan 2-01 predicate; this plan adds a test seeding both inside a profile and inside desktop-state/ and asserting neither appears. Restoring a stale bridge id is already known to break `/remote-control`. |
| T-2-08 | Information disclosure | credentials collector | high | mitigate | Only the four D1 profile members are added — meta.json, both token caches, desktop-state/. `backups/`, `prelogin-backup/` and `hidden/` are machine-specific rollback state and are excluded by path component. |
| T-2-09 | Information disclosure | chat_index collector | high | mitigate | `local-agent-mode-sessions/` is excluded; a copied Cowork transcript cannot be made valid on another machine, so carrying it leaks content for no benefit. |
| T-2-10 | Spoofing | profile enumeration | medium | mitigate | The profile directory name is authoritative, as it already is in `load_profiles`, so a hand-edited meta.json cannot make a profile answer to another account's label. |
| T-2-SC | Tampering | npm/pip/cargo installs | high | accept | This plan adds no dependency. `cargo machete` runs in the phase-end gate. |
</threat_model>

<verification>
`cargo test --lib sync::scope` is green. Every new test injects its roots via `SyncRoots::at`
over a `TempDir`; none reads the real `~/.claude-acc/profiles` or the real Claude Desktop
data dir.
</verification>

<success_criteria>
`scope::collect` returns real, D1-accurate scans for config, credentials, routines and
chat_index, with the D2 exclusions proven to bite on the trees where the dangerous files
actually live.
</success_criteria>

<output>
Create `.planning/phases/2-bundle-scope-local-index-dry-run-planning/2-02-SUMMARY.md` when done.
</output>
