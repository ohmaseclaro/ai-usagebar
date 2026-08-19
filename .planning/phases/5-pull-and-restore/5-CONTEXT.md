# Phase 5 Context — Pull and Restore

**Decisions locked by the orchestrator.** Restore writes over a *working* machine's config and
credentials, so this phase is where the feature can do real local damage. Everything here is
biased toward "refuse and explain" over "helpfully overwrite".

## Locked decisions

### D1 — Restore is dry-run by default

`sync pull` shows what would change and writes **nothing** unless `--apply` is passed (or the
user confirms at an interactive prompt). The diff is per item: added / updated / skipped, with
the reason for each skip.

This inverts the usual convenience default on purpose. A wrong push costs a re-push; a wrong
restore costs the credentials and history on the machine in front of you.

### D2 — A locally-newer item is never silently overwritten

Last-write-wins is the agreed model, but "the remote is newer" must be *established*, not
assumed from the fact that the user typed `pull`. Per item, compare the recorded modification
time; when the local copy is newer, **skip it and say so**. `--force` overrides, per item class,
and prints exactly what it is about to lose.

Credentials get the strictest treatment: a locally-newer credential is never overwritten even
under `--force` without a second explicit confirmation, because the failure mode is
silently reverting a live OAuth token to a stale one — this project has already shipped one
bug in that exact family (two stores of the same account fighting over a rotating refresh
token) and should not build a third path into it.

### D3 — Backup before the first write, reusing the existing convention

Before the first byte is written, tar the affected local trees into
`~/.claude-acc/backups/sync-restore-<YYYYmmdd-HHMMSS>.tar.gz` — the same directory and naming
shape the account switcher already uses for its rollback archives, so there is one place a user
looks for "undo", not two. Print the exact `tar -xzf … -C …` rollback command.

The backup is taken even for a partial restore, and even under `--force`. It is the last line
of defence.

### D4 — Never restore machine-bound or volatile state

Same hard-exclusion list as Phase 2's D2, enforced again on the *write* side rather than
trusted from the bundle: `bridge-state.json`, `ant-device-registry.json`, caches, lock files,
`local-agent-mode-sessions/`. A bundle produced by a future or modified client must not be able
to talk this side into writing them. Validate on read; ignore silently-unknown paths rather
than writing them.

### D5 — Path traversal is treated as hostile input

Every path in the manifest is validated before use: rejected if absolute, if it contains `..`,
or if it resolves outside its category root. The bundle is attacker-controllable in the threat
model we accepted (someone with repo write access), so a manifest entry is untrusted data —
the same posture the project already takes toward vendor API responses.

### D6 — Conflict reporting is a report, not a prompt

Per SYNC-06, the user is *told* what was overwritten; they are not asked item by item. A
restore that stops to ask 200 questions is a restore nobody finishes. The interactive
confirmation is one gate at the start (D1), and the detail lands in the summary afterwards.

### D7 — Restore is resumable and idempotent

Applying the same snapshot twice changes nothing the second time. An interrupted restore can be
re-run; already-correct items are skipped by digest, not rewritten. This falls out of
content-addressing and should be asserted by a test, not assumed.

## Constraints inherited from the codebase

- Every write atomic (tempfile + persist). A half-written `config.toml` or credential file is
  the worst possible outcome of an interrupted restore.
- Restored credential files land mode 0600; restored directories 0700. Never inherit the
  archive's recorded mode blindly.
- Tests hermetic: restore targets are injected roots, never a real `$HOME`. Round-trip tests
  (push → pull into a second temp root → compare) are the strongest available proof and should
  exist.
- Decrypted plaintext never lands at a temp path that outlives the operation (SAFE-05) — write
  into the destination's own directory and rename, so there is no window where plaintext sits
  somewhere world-readable.

## Security note for the audit (2g)

Restore is the phase where a compromised or tampered bundle turns into local writes. Worth
hunting: path traversal in the manifest, symlink targets that escape the category root, a
mode-0644 credential slipping through, and any path where a failed integrity check still leaves
partial output on disk.
