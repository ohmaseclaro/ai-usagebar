# GitHub sync setup and authentication

The sync feature will back up your usage history and configuration to a private GitHub
repository, encrypted with a password only you hold. **This release pairs with the
repository and verifies it; `sync push` is what uploads** — see "What this release
does" below. This guide covers repository creation, token setup, and what the tool
checks before it will touch your data.

## Create your backup repository

The tool never creates repositories on your behalf. This is by design: the GitHub token deliberately holds no repository-creation permission, which makes it structurally impossible for the token to create a public repository, even by accident. Naming the repository is a one-time, explicit act you perform once.

You can create the repository either way:

**Via GitHub CLI (recommended):**

```bash
gh repo create <owner>/<name> --private
```

Replace `<owner>` with your GitHub username or organization, and `<name>` with your chosen repository name (e.g. `gh repo create alice/ai-usagebar-backup --private`).

**Via GitHub web form:**

1. Go to https://github.com/new
2. Name your repository
3. Select **Private** visibility
4. Leave other options at their defaults
5. Click **Create repository**

Tell ai-usagebar the repository's location by adding it to `~/.config/ai-usagebar/config.toml`:

```toml
[sync]
repo = "owner/name"
```

A missing or unset `repo` value is an actionable error that prints the exact command to create one. The tool will not guess a repository name, so a missing repo always means you need to create one first.

## Create a fine-grained personal access token

The token must be scoped to your single backup repository with exactly two permissions. This prevents a leaked token from being a skeleton key across everything you own.

**On GitHub:**

1. Go to **Settings → Developer settings → Personal access tokens → Fine-grained tokens**
2. Click **Generate new token**
3. **Name:** something descriptive (e.g. `ai-usagebar-sync`)
4. **Expiration:** choose a duration (e.g. 90 days)
5. **Repository access:** Select **Only select repositories**, then choose your backup repository from the list
6. **Repository permissions:**
   - **Contents:** Check **Read and write**
   - **Metadata:** **Read-only** (checked by default, cannot be removed)
7. **Do NOT grant Administration permissions.** This field must remain unchecked. The lack of this permission is what structurally prevents the token from creating or modifying repository settings — it is the whole enforcement, and the unchecked box is the control. There is deliberately **no warning** if you grant it anyway: `GET /repos/{owner}/{repo}` reports the authenticated *user's* role on the repository rather than the token's granted permissions, and since you create the repository yourself you are always its admin — so the warning would fire on every legitimate install, and a warning that always fires trains you to ignore it. Whether a fine-grained token narrows that field is an open question tracked in `docs/sync-format.md` §7; if it turns out to report the grant, the warning goes back in.
8. Click **Generate token** and copy the token value immediately (GitHub only displays it once)

Your token carries exactly two permissions:
- **Contents: read/write** — needed to push and pull your data
- **Metadata: read** — needed to verify the repository's visibility and settings

Nothing else.

## Where the token is stored

The tool looks for the token in this order:

1. **`AI_USAGEBAR_SYNC_TOKEN` environment variable** — Use this for CI environments and for headless restores over SSH. No persistent storage, no file-system reads. This is the override.

2. **macOS Keychain item** (macOS only) — Service name `ai-usagebar-sync-token`. The tool reads it via `security(1)` and writes via Security.framework, so the token never appears in process arguments or shell history.

3. **`sync-token` file beside `config.toml`** — normally `~/.config/ai-usagebar/sync-token`, mode 0600 (read/write by owner only), matching the existing credential-file convention. This is where `sync setup` *writes* the token when you are not on macOS. It is *read* on every platform, so a token file copied from another machine still works on a Mac.

4. **`gh auth token`** (convenience fallback) — If the GitHub CLI is installed and logged in, the tool can use its authentication. This is never required; it is a convenience so you do not need to manage a separate token.

If GitHub rejects the token with a 401, the tool clears **only the store the rejected token came from** — the Keychain item if it came from the Keychain, the `sync-token` file if it came from the file, and nothing at all if it came from `AI_USAGEBAR_SYNC_TOKEN` or from `gh`. Those two are not the tool's to delete, and a 401 on one of them says nothing about a stored token that was never sent. The message names which of them to change instead: while the environment variable is set, a replacement written anywhere else is never reached.

The tool never writes the token into `config.toml`. Inline API keys there are a deliberate choice for read-only provider keys (Claude, OpenAI, Z.AI) because a leaked key is a minor risk on a read-only endpoint. A token that can write to a repository is a different class of secret, so it lives in a separate, mode-0600 location (or in the Keychain on macOS).

### Why not D-Bus secret service?

The `keyring` and `secret-service` packages provide a unified secrets interface via D-Bus. They require a live D-Bus session and an unlocked keyring, which fails in headless environments and over SSH — exactly the scenario this sync feature exists to serve. The project already has platform-specific credential handling (Keychain on macOS, local files on Linux), so those are reused here instead of adding a new runtime dependency.

## Repository verification

Before any sync operation, the tool verifies the repository meets these conditions:

- **Private visibility** — the repository is marked private, not internal or public
- **Owned by the configured user** — the repository's owner numeric ID matches the one recorded at first pairing
- **Not archived** — archived repositories reject pushes
- **Not a fork** — a fork shares its upstream's object network, so objects pushed to it can be reachable from the public parent

This check runs **immediately before every push**, not once at setup. A repository can be made public from the web interface at any moment, so the tool re-checks every time.

### If the repository is found public

If the tool detects the repository has become public after previously being private:

1. The operation aborts and prints an error
2. You are told to make the repository private again
3. **You must rotate any credentials that may already have been pushed**, because a previous push may have succeeded while the repository was public, and bytes that have been published cannot be un-published

This is not optional. If you backed up credentials before you made the repository public, those credentials are compromised.

## What `sync setup` does and does not do

`sync setup` authenticates, resolves the repository, and verifies it is private. It performs **zero uploads** — pairing and pushing are separate commands, so verifying the pairing never costs bandwidth.

It is a guided, five-step flow:

1. **The categories.** Shows what gets bundled and lets you toggle each one, `credentials` included and explicit. This comes first because it is an *input to the gate*: whether `credentials` is in the bundle is what decides whether a public repository is refused outright or merely warned about, and the gate has to be asked the question you actually answered.
2. **The repository and the gate.** Resolves the token, fetches the repository's details, and refuses unless it passes every condition above, judged against the categories you just chose. Every refusal stops *here* — you are never asked to choose a sync password for a repository that is about to be rejected. On a first pairing it names the repository and owner ids it is pairing with; if this machine was already paired, that line means the pairing record went missing, which is worth investigating.
3. **The sync password.** Offers a generated 20-character passphrase (press Enter to take it) or accepts your own, subject to a length floor. There is no recovery: the password is the only thing that can open the bundle. If a keyfile is already at `<config dir>/sync/keyfile.json`, setup stops before anything else — overwriting one makes every bundle written under the old password permanently unreadable.
4. **The size.** Runs the same planner `sync push --dry-run` runs and shows its figures — files, raw bytes, and what a first push would actually send — then asks you to confirm.
5. **Everything that persists.** Writes the keyfile at mode 0600, saves your category choices back into `config.toml` with comments and key order preserved, stores the token where only you can read it (the Keychain on macOS, the mode-0600 file elsewhere), and records the pairing. Nothing before this point writes anything, so declining at step 4 leaves the machine exactly as it was and the command can simply be re-run.

Once setup succeeds, `ai-usagebar sync push` uploads. It re-checks that the repository is private before the first byte and again before publishing, uploads the encrypted packs as release assets, verifies each one reads back correctly, and only then publishes the snapshot pointer with a compare-and-swap precondition. **Interrupting a push before that last step leaves the previous snapshot exactly as it was** — the uploaded packs are referenced by nothing and are collected later.

Two more commands round it out: `ai-usagebar sync prune` deletes remote data no kept snapshot still references (nothing younger than a day, so it cannot race another machine's in-flight push), and `ai-usagebar sync rekey` changes the sync password. A password change rewraps the master key and moves no pack bytes — and it is **not revocation**: anyone who already holds a copy of the old keyfile can still open it with the old password.

Check the status of your pairing with:

```bash
ai-usagebar sync status
```

It prints the category listing, then the repository:

```
  repo:      owner/name
  visible:   private
  token:     present (env)
  verified:  2026-08-19T12:00:00+00:00
```

The `token:` line reports the token's **source**, never its value. The four labels are `env`, `Keychain`, `file`, and `gh`, matching the resolution order above. `verified:` is when this machine last confirmed the pairing.

The repository half and the category listing fail independently: an expired token still leaves the listing visible, so you can always see what *would* be sent — but any repository-section failure is a non-zero exit.

## Bandwidth caveat

GitHub's acceptable-use policy reserves the right to throttle or suspend accounts for bandwidth use significantly out of line with comparable users. A frequently-rewritten multi-gigabyte bundle could fit that profile if pushed many times. There is no rule against backing up to a private repository, but you should know the shape of the risk rather than discover it by surprise. Monitor your GitHub bandwidth usage if you plan to push frequently.
