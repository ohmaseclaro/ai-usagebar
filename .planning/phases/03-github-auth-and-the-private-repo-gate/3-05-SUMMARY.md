---
phase: 03-github-auth-and-the-private-repo-gate
plan: 05
type: summary
date: 2026-08-19
---

# 3-05 Execution Summary

## Deliverables

✓ `docs/sync-github.md` — comprehensive setup guide covering token recipe, repository creation, and verification
✓ `docs/configuration.md` — updated with `[sync] repo` configuration key
✓ `README.md` — new sync feature section with setup overview and reference links
✓ All automated verification checks pass
✓ No token-shaped literals in documentation
✓ Single commit with all changes

## Documentation coverage

The documentation implements all locked decisions from 3-CONTEXT.md:

- **D1 (explicit repo naming):** Documents that tool never creates repos, provides exact `gh repo create` command, and explains why (tool holds no creation permission)
- **D2 (token resolution order):** Lists all four sources in order: env var, Keychain, config file, gh CLI; explains which is for what use case
- **D3 (required permissions):** Names exact permission set (Contents: read/write + Metadata: read) and explicitly calls out why Administration must NOT be granted
- **D5 (zero uploads this phase):** States plainly that `sync setup` authenticates and verifies only; pushing arrives later
- **D4 (private gate):** Documents that visibility check runs before every push, is never cached, and what happens if repo becomes public (abort + rotate credentials)
- **D6 (failure messages):** Discussed at command level; implementation deferred to phase 3-02 (error handling)

## Assumptions about not-yet-merged phases

The documentation assumes the following will be true when phases 3-01 through 3-04 merge:

| Assumption | Source | Plan |
|---|---|---|
| Command `ai-usagebar sync setup` exists and authenticates | Spec calls it | 3-01 or 3-02 |
| Command `ai-usagebar sync status` exists | Spec calls it | 3-01 or 3-02 |
| Config key `[sync] repo = "owner/name"` is parsed and validated | D1 (locked) | 3-01 |
| Env var `AI_USAGEBAR_SYNC_TOKEN` is checked first | D2 (locked) | 3-02 |
| Keychain service name is `ai-usagebar-sync-token` | D2 (locked) | 3-02 (macOS) |
| Config file path is `~/.config/ai-usagebar/sync-token` | D2 (locked) | 3-02 (Linux/other) |
| Token resolution falls through to `gh auth token` if available | D2 (locked) | 3-02 |
| API call `GET /repos/{owner}/{repo}` returns visibility + owner | GitHub API | 3-03 (tracer) |
| Repository visibility field is checked for `private == true` | D4 (locked) | 3-03 |
| Check runs before every push attempt (not cached) | D4 (locked) | 3-04 |

## Cross-reference reconciliation

Plan 3-07 (verification) should reconcile these command names and config key against what actually shipped:
- `sync setup` and `sync status` commands
- `[sync] repo` configuration section
- `AI_USAGEBAR_SYNC_TOKEN` environment variable

If the implementation diverges (e.g., commands named differently, config structure changed), the documentation must be updated to match.

## Notes for next phase

- Phase 3-04 (security audit) should verify that a leaked token cannot create public repos (due to missing Administration permission) — this is the core threat mitigation
- Phase 3-07 should check that documentation claims match implementation exactly, especially command names and config keys
- No implementation exists yet for displaying warning when token carries Administration permission (D3 warning branch) — this is acceptable in Phase 3, but must be added if found missing in Phase 4
