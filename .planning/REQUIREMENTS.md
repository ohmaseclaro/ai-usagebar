# Requirements: ai-usagebar — Encrypted GitHub Sync

**Defined:** 2026-08-17
**Core Value:** Answer "how much quota do I have left, and on which account?" instantly and
correctly — without the user opening a browser, and without ever mis-reporting one
account's usage as another's.

**Milestone goal:** Let a user carry their ai-usagebar state — settings, Claude account
credentials, routines, chat indexes — between machines through a **private GitHub repo**,
encrypted end-to-end with a password only they know, syncing only what changed.

## v1 Requirements

### Bundle scope (what gets synced)

- [ ] **SCOPE-01**: The bundle covers, as independently toggleable categories: app config,
      Claude Desktop account credentials/profiles, routines/scheduled tasks, chat session
      indexes, and (opt-in) chat transcripts.

- [ ] **SCOPE-02**: Every category except chat transcripts is enabled by default; the user
      can uncheck any of them.

- [ ] **SCOPE-03**: Chat transcripts are **off** by default and, when enabled, are bounded
      (by age and/or total size) with the resulting bundle size shown before the first push.

- [ ] **SCOPE-04**: The user can preview exactly what a push would include — per category,
      file count and byte size — without pushing (`--dry-run`).

- [ ] **SCOPE-05**: Category selection persists in `config.toml` and is itself part of the
      synced config, so a second machine inherits the same choices.

### Encryption

- [ ] **CRYPTO-01**: The whole bundle is encrypted client-side with a key derived from a
      user-set password; the remote never receives plaintext or the password.

- [ ] **CRYPTO-02**: Password-derived keys use a memory-hard KDF with parameters stored
      alongside the data, so parameters can be raised in future versions without breaking
      existing bundles.

- [ ] **CRYPTO-03**: A wrong password fails cleanly and unambiguously — never partial or
      garbage output.

- [ ] **CRYPTO-04**: The user can change the sync password without re-uploading the entire
      bundle.

- [ ] **CRYPTO-05**: Tampering with, reordering, truncating, or rolling back remote data is
      detected on pull and refuses to restore.

- [ ] **CRYPTO-06**: Password strength is enforced at set time, with the offline-attack risk
      explained in plain language.

- [ ] **CRYPTO-07**: Key material is zeroized after use and never appears in process
      arguments, environment variables, logs, or error messages.

### Safety gates

- [ ] **SAFE-01**: A push whose bundle contains credentials is **refused** unless the target
      repo is verified private, checked immediately before every push.

- [ ] **SAFE-02**: If a previously-private target repo is found to be public, the push aborts
      and the user is told to rotate the affected credentials.

- [x] **SAFE-03**: Restoring never silently overwrites local credentials that are newer than
      the remote copy; the user is told what would change first.

- [x] **SAFE-04**: A local backup is taken before the first restore writes anything, and the
      command to roll it back is printed.

- [x] **SAFE-05**: No plaintext of any synced file is ever written to a temporary path that
      outlives the operation.

### Sync mechanics

- [ ] **SYNC-01**: Only data that actually changed since the last successful sync is
      uploaded.

- [ ] **SYNC-02**: A sync with no local changes completes near-instantly and uploads nothing.
- [ ] **SYNC-03**: Appending to a large file uploads roughly the appended bytes, not the whole
      file.

- [ ] **SYNC-04**: An interrupted push leaves the remote in its previous consistent state —
      never a half-written snapshot a pull could read.

- [ ] **SYNC-05**: A resumed push after interruption reuses what already uploaded.
- [x] **SYNC-06**: When two machines have both changed the same item, the most recent wins
      per item and the user is told what was overwritten.

- [ ] **SYNC-07**: Remote storage does not grow without bound; superseded data can be pruned.

### GitHub integration

- [ ] **REPO-01**: The user supplies a GitHub token scoped to the **single** sync repo, with
      no permission to create or administer repositories.

- [ ] **REPO-02**: The GitHub token is stored with the same protection as existing
      credentials (macOS Keychain / mode-0600 file), never in a tracked file.

- [ ] **REPO-03**: The tool **never creates a repository**. The user points it at a private
      repo they already own; a missing repo is an actionable error, not an auto-fix.
      *(Research finding: withholding `Administration: write` makes the app structurally
      incapable of creating a public repo — a stronger guarantee than SAFE-01's runtime
      check, which remains as defence in depth.)*

- [ ] **REPO-04**: An existing token from the environment or the user's git/gh credential
      helper can be reused so setup needs no new secret.

- [ ] **REPO-05**: Network, auth, and rate-limit failures produce an actionable message and a
      non-zero exit — never a silent partial success.

- [ ] **REPO-06**: Bulk data is uploaded as a small number of large objects, never one
      request per chunk, staying inside GitHub's content-creation limits (80/min, 500/hour).

- [ ] **REPO-07**: The snapshot pointer is published with a compare-and-swap precondition, so
      two machines pushing concurrently cannot interleave into a corrupt state.

### Commands and surfaces

- [x] **UX-01**: `ai-usagebar sync push` and `ai-usagebar sync pull` (or equivalent) perform
      the two directions, with `--dry-run` on both.

- [ ] **UX-02**: `ai-usagebar sync status` reports what is configured, when the last sync ran,
      and what would change now.

- [ ] **UX-03**: First-time setup is guided end to end: choose repo, set password, choose
      categories, confirm the size, push.

- [ ] **UX-04**: Progress is visible for a long first push (bytes/objects, not a frozen
      terminal).

- [ ] **UX-05**: The macOS menu bar exposes sync state and can trigger a push/pull, reusing
      the existing non-interactive-subprocess conventions.

- [ ] **UX-06**: The widget's exit-0 invariant holds — a sync failure never takes the status
      bar down.

## v2 Requirements

Deferred. Tracked, not in this roadmap.

### Automation

- **AUTO-01**: Scheduled/background sync on an interval
- **AUTO-02**: Sync-on-change triggered by config or credential writes

### Portability

- **PORT-01**: Non-GitHub remotes (generic git, S3-compatible object storage)
- **PORT-02**: Export/import a bundle as a single local file, no remote at all

### Recovery

- **REC-01**: Browse and restore an *older* snapshot, not just the latest
- **REC-02**: Selective restore of a single category or account

## Out of Scope

| Feature | Reason |
|---------|--------|
| Public repos for credential-bearing bundles | An encrypted blob in a public repo is an unlimited offline attack on the password, permanently archived by forks/mirrors |
| Chat transcripts in the default bundle | 4.0 GB / 4110 files measured; far past GitHub's ~1 GB repo guidance |
| A hosted sync service or backend | The project has no server and should not grow one; the user's own repo is the store |
| Password recovery / escrow | Zero-knowledge by design — a recoverable password defeats the threat model |
| Migrating Cowork (agent-mode) sessions | Their transcript path embeds the owning account UUID plus an unreconstructable suffix |
| Real-time multi-machine collaboration | Sync is snapshot-based; last-write-wins per item is the agreed model |
| Creating the GitHub repo for the user | Withholding repo-creation permission is what makes a public-repo push structurally impossible; auto-create would require the very permission we refuse |
| Git LFS | 10 GiB/month free bandwidth ≈ 87 restores, then LFS is disabled account-wide for the rest of the month |
| Storing bundle data as git objects/commits | AEAD ciphertext gets zero delta compression, git retains deleted blobs permanently, and the 10 GB repo cap arrives within ~2 years of weekly syncs |
| `keyring`/`secret-service` for the token | zbus needs a live D-Bus session and fails over SSH — exactly the headless restore case; the project's existing Keychain + mode-0600 convention already covers both platforms |

## Traceability

| Requirement | Phase | Status |
|-------------|-------|--------|
| SCOPE-01 | Phase 2 | Pending |
| SCOPE-02 | Phase 2 | Pending |
| SCOPE-03 | Phase 2 | Pending |
| SCOPE-04 | Phase 2 | Pending |
| SCOPE-05 | Phase 2 | Pending |
| CRYPTO-01 | Phase 1 | Pending |
| CRYPTO-02 | Phase 1 | Pending |
| CRYPTO-03 | Phase 1 | Pending |
| CRYPTO-04 | Phase 4 | Pending |
| CRYPTO-05 | Phase 1 | Pending |
| CRYPTO-06 | Phase 1 | Pending |
| CRYPTO-07 | Phase 1 | Pending |
| SAFE-01 | Phase 3 | Pending |
| SAFE-02 | Phase 3 | Pending |
| SAFE-03 | Phase 5 | Complete |
| SAFE-04 | Phase 5 | Complete |
| SAFE-05 | Phase 5 | Complete |
| SYNC-01 | Phase 2 | Pending |
| SYNC-02 | Phase 2 | Pending |
| SYNC-03 | Phase 2 | Pending |
| SYNC-04 | Phase 4 | Pending |
| SYNC-05 | Phase 4 | Pending |
| SYNC-06 | Phase 5 | Complete |
| SYNC-07 | Phase 4 | Pending |
| REPO-01 | Phase 3 | Pending |
| REPO-02 | Phase 3 | Pending |
| REPO-03 | Phase 3 | Pending |
| REPO-04 | Phase 3 | Pending |
| REPO-05 | Phase 3 | Pending |
| REPO-06 | Phase 4 | Pending |
| REPO-07 | Phase 4 | Pending |
| UX-01 | Phase 5 | Complete |
| UX-02 | Phase 2 | Pending |
| UX-03 | Phase 3 | Pending |
| UX-04 | Phase 4 | Pending |
| UX-05 | Phase 6 | Pending |
| UX-06 | Phase 6 | Pending |

**Coverage:**

- v1 requirements: 37 total
- Mapped to phases: 37 ✓
- Unmapped: 0

*(The earlier "33 total" count was a miscount: SCOPE 5 + CRYPTO 7 + SAFE 5 + SYNC 7 + REPO 7 + UX 6 = 37.)*

---
*Requirements defined: 2026-08-17 · Traceability filled during roadmap creation: 2026-08-19*
