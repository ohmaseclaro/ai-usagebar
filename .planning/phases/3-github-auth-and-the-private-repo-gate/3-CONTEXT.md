# Phase 3 Context — GitHub Auth and the Private-Repo Gate

**Decisions locked by the orchestrator** on the user's instruction to decide using the
codebase's existing mindset and their real usage. Downstream agents honour these.

## Locked decisions

### D1 — The repo is named explicitly by the user; nothing is guessed

`[sync] repo = "owner/name"` in `config.toml`. No default, no derived name, no search of the
user's repos. A missing or unset value is an error that prints the exact command to create one:

```
gh repo create <owner>/<name> --private
```

Rationale: the app deliberately holds no repo-creation permission (REPO-03), so guessing a
name could only ever produce a confusing 404. Naming it is a one-time, explicit act.

### D2 — Token resolution order

1. `AI_USAGEBAR_SYNC_TOKEN` env var (explicit override, useful for CI and for the user's
   headless/SSH restore case)
2. macOS Keychain item, service `ai-usagebar-sync-token` — read via `security(1)`, written via
   Security.framework so the token never appears in process arguments. This is exactly the
   split `src/anthropic/keychain.rs` already uses, for the same reason.
3. Linux/other: `~/.config/ai-usagebar/sync-token`, mode 0600, matching the existing
   credential-file convention.
4. `gh auth token`, if `gh` is present — convenience only, never required.

**Rejected:** `keyring`/`secret-service`. It needs a live D-Bus session and fails over SSH,
which is precisely the headless-restore case this feature exists to serve. The project already
has both platform halves; use them.

### D3 — Required token permissions, and refusing more

Fine-grained PAT, scoped to the **single** sync repo: `Contents: read/write` +
`Metadata: read`. Setup documents exactly this and no more.

If the token turns out to carry `Administration` on the repo, **warn** (do not fail): the
whole point of REPO-03 is that lacking that permission makes creating a public repo
structurally impossible, so a token that has it silently weakens the guarantee and the user
should know. Warn rather than refuse, because we cannot reliably enumerate a token's
permissions without extra calls and a false refusal would be worse than an unheeded warning.

### D4 — The private gate is checked immediately before every push, not cached

`GET /repos/{owner}/{repo}` → require `private == true`. Re-checked before *every* push, not
once at setup: a repo can be flipped to public at any time from the web UI.

On finding it public when credentials are in the bundle: **abort before uploading a single
byte**, and tell the user to (a) make the repo private again and (b) rotate any credential
that may already have been pushed — since a previous push may have landed while it was
public. Rotation advice is not optional politeness; it is the only correct response.

If the `credentials` category is *off*, a public repo is allowed but still warned about
(chat indexes and config are personal data even without tokens).

### D5 — Zero bytes leave the machine in this phase

Phase 3 authenticates, resolves the repo, and verifies visibility. It performs **no upload**.
That is what makes the gate provably prior to the first byte rather than merely sequenced
before it in a plan. `sync setup` and `sync status` are the only surfaces here.

### D6 — Failure messages name the fix

Every failure path prints what to do: 401 → how to re-issue the token; 403 with rate-limit
headers → when it resets; 404 → repo missing or token not scoped to it (both, since we cannot
distinguish them — GitHub deliberately 404s unauthorised private repos); network → retry
guidance. Non-zero exit in all cases. The widget's exit-0 invariant applies to the *widget*,
never to `sync`.

## Constraints inherited from the codebase

- Never place a token in process arguments or environment of a spawned child.
- Never log a token, not even a prefix; report presence and source only
  (`"token: present (Keychain)"`).
- Tests hermetic: no test may touch the real Keychain, the real token file, or the network.
  Mock the HTTP layer (`mockito` is already a dev-dependency); live checks go in `tests/live.rs`
  behind `#[ignore]`, as the existing vendor tests do.
- Reuse `reqwest` 0.12 with rustls — no new HTTP stack.

## Calibration owed by this phase

None owed here, but Phase 4 depends on **CAL-1** (do private-repo release assets honour
`Range:` after the 302 to signed storage?). If this phase's HTTP plumbing lands early enough,
run CAL-1 here as an `#[ignore]`d live test against the user's own repo so Phase 4 starts with
the answer. *Fallback:* assume no `Range:` support and size packs so a full asset fetch is
acceptable.
