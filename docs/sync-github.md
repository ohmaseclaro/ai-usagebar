# GitHub sync setup and authentication

The sync feature will back up your usage history and configuration to a private GitHub
repository, encrypted with a password only you hold. **This release pairs with the
repository and verifies it; it does not upload anything yet** — see "What this release
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
7. **Do NOT grant Administration permissions.** This field must remain unchecked. The lack of this permission is what structurally prevents the token from creating or modifying repository settings. The tool cannot check this for you — GitHub's repository endpoint reports *your* role on the repository, not your token's grant, so there is nothing it can read that would tell the difference (see `docs/sync-format.md` §7). Leaving the box unchecked here is the whole enforcement.
8. Click **Generate token** and copy the token value immediately (GitHub only displays it once)

Your token carries exactly two permissions:
- **Contents: read/write** — needed to push and pull your data
- **Metadata: read** — needed to verify the repository's visibility and settings

Nothing else.

## Where the token is stored

The tool looks for the token in this order:

1. **`AI_USAGEBAR_SYNC_TOKEN` environment variable** — Use this for CI environments and for headless restores over SSH. No persistent storage, no file-system reads. This is the override.

2. **macOS Keychain item** (macOS only) — Service name `ai-usagebar-sync-token`. The tool reads it via `security(1)` and writes via Security.framework, so the token never appears in process arguments or shell history.

3. **`~/.config/ai-usagebar/sync-token` file** (Linux and other platforms) — Mode 0600 (read/write by owner only), matching the existing credential-file convention. This is where the token lives if you are not on macOS.

4. **`gh auth token`** (convenience fallback) — If the GitHub CLI is installed and logged in, the tool can use its authentication. This is never required; it is a convenience so you do not need to manage a separate token.

The tool never writes the token into `config.toml`. Inline API keys there are a deliberate choice for read-only provider keys (Claude, OpenAI, Z.AI) because a leaked key is a minor risk on a read-only endpoint. A token that can write to a repository is a different class of secret, so it lives in a separate, mode-0600 location (or in the Keychain on macOS).

### Why not D-Bus secret service?

The `keyring` and `secret-service` packages provide a unified secrets interface via D-Bus. They require a live D-Bus session and an unlocked keyring, which fails in headless environments and over SSH — exactly the scenario this sync feature exists to serve. The project already has platform-specific credential handling (Keychain on macOS, local files on Linux), so those are reused here instead of adding a new runtime dependency.

## Repository verification

Before any sync operation, the tool verifies the repository meets these conditions:

- **Private visibility** — the repository is marked private, not internal or public
- **Owned by the configured user** — the repository's owner numeric ID matches the one recorded at first pairing
- **Not archived** — archived repositories reject pushes
- **Not a fork** — forks have limited API quotas and cannot receive certain types of data

This check runs **immediately before every push**, not once at setup. A repository can be made public from the web interface at any moment, so the tool re-checks every time.

### If the repository is found public

If the tool detects the repository has become public after previously being private:

1. The operation aborts and prints an error
2. You are told to make the repository private again
3. **You must rotate any credentials that may already have been pushed**, because a previous push may have succeeded while the repository was public, and bytes that have been published cannot be un-published

This is not optional. If you backed up credentials before you made the repository public, those credentials are compromised.

## What `sync setup` does and does not do

`sync setup` authenticates, resolves the repository, and verifies it is private. It performs **zero uploads** in this release.

The command chain looks like this:

- Authenticates using the token (checks it is valid and has the right scopes)
- Fetches the repository details (verifies it exists and belongs to the right owner)
- Checks visibility (must be private)
- Records the owner's numeric ID for future pairing checks

Pushing to the repository arrives in a later release. If you need to back up your data today, this phase verifies the foundation is in place; it does not yet upload anything.

Check the status of your pairing with:

```bash
ai-usagebar sync status
```

## Bandwidth caveat

GitHub's acceptable-use policy reserves the right to throttle or suspend accounts for bandwidth use significantly out of line with comparable users. A frequently-rewritten multi-gigabyte bundle could fit that profile if pushed many times. There is no rule against backing up to a private repository, but you should know the shape of the risk rather than discover it by surprise. Monitor your GitHub bandwidth usage if you plan to push frequently.
