# GitHub Transport + Auth — design research

**Project:** ai-usagebar (Rust 1.88, edition 2024, macOS + Linux, ships via crates.io / AUR source + bin)
**Researched:** 2026-08-19
**Overall confidence:** HIGH on limits and transport, MEDIUM on two flagged items (release-asset Range GETs, GitHub-App repo creation)

---

## TL;DR — the recommendation

**Primary: GitHub REST API over the already-vendored `reqwest` 0.12 + rustls. Zero new crates.**

- **Bulk data** → **Release assets**. Concatenate content-addressed chunks into ≤ 1.9 GiB *packs*; one `POST https://uploads.github.com/...` per pack. A 115 MB bundle is **one** HTTP request, not thousands.
- **Manifest** → **Contents API `PUT` with the `sha` precondition** (compare-and-swap). This is the "flip the ref last" linearization point.
- **Auth** → fine-grained PAT scoped to the single backup repo (`Contents: read/write`), primary; **fallback** = reuse an already-present credential via `git credential fill` / `gh auth token` / `$GITHUB_TOKEN`.
- **Repo bootstrap** → **do not create the repo.** Require the user to point at an existing private repo. This deletes the `Administration: write` permission tier, the whole creation code path, and the "we created a public repo by accident" failure mode.

**Fallback design (if REST proves wrong): `git` CLI subprocess.** `git` is already in the AUR `makedepends`; moving it to `depends` is a one-word diff. No new Rust code paths for TLS, retry, or auth (the credential helper does it). Costs: 2 GiB per push, 10 GB repo cap, permanent history retention of every encrypted chunk ever written, and 6 pushes/min/repo.

**Explicitly rejected: `git2`, `gix`, `gh`-as-transport.** Rationale in §1.

---

## 1. Transport options

### 1.1 Comparison table

| | **REST + reqwest** ✅ | `git` CLI subprocess (fallback) | `gh` CLI subprocess | `git2` (libgit2) | `gix` (gitoxide) |
|---|---|---|---|---|---|
| **New Rust deps** | **0** (reqwest 0.12 already in tree; needs `stream` feature added) | 0 | 0 | `git2 0.21` → `libgit2-sys 0.18`, `openssl-sys`, `openssl-probe`, `libc`, `bitflags`, `log` | `gix 0.86` → ~45 `gix-*` sub-crates |
| **Binary size Δ** | ~0 | 0 | 0 | ≈ +1.5–3 MB (static libgit2) + OpenSSL linkage on Linux *(MEDIUM — measure with `cargo bloat`)* | Large; ~45 crates of Rust codegen |
| **Build complexity** | none | none | none | **Compiles libgit2 C at build time** (`vendored-libgit2`). HTTPS requires the `https` feature → `openssl-sys`. Breaks the project's rustls-only stance and adds `openssl` to AUR `depends` (currently just `gcc-libs`). Hurts the aarch64 cross-build in `release.yml`. | Pure Rust; can even reuse reqwest+rustls via `blocking-http-transport-reqwest-rust-tls`. But **~45-crate compile** is a real tax on the AUR source build. |
| **Runtime prerequisite on user machine** | none | `git` (~universal; already a `makedepends`) | **`gh` — not guaranteed.** 2.79.0 on this dev box only. Not in AUR deps. | none | none |
| **Cross-platform** | ✅ identical on macOS/Linux | ✅ | ✅ if installed | ⚠️ SecureTransport on macOS, OpenSSL on Linux — two TLS stacks to reason about | ✅ |
| **Many small objects** | ✅ **best** — you control batching; pack N chunks into 1 request | ✅ one packfile per push regardless of object count — but zlib on AEAD ciphertext buys ~0% and burns CPU | same as `git` | same as `git` | ❌ |
| **Resumability** | ✅ **per-pack**: list existing assets, skip what's there. Mid-file resume not possible (see §5) | ❌ re-push resends the whole pack | ❌ `gh release upload` restarts the file | ❌ | ❌ |
| **Push support** | n/a | ✅ | ✅ | ✅ | ❌ **push is not implemented** — `gix-protocol` checklist still has `[ ] push`, `[ ] send-pack / receive-pack client plumbing` |
| **Verdict** | **PRIMARY** | **FALLBACK** | credential source only | reject | reject (disqualified on push alone) |

### 1.2 Why `git`-shaped storage is wrong for this payload

The bundle is *encrypted* content-addressed chunks. Consequences:

1. **Delta compression yields nothing.** AEAD ciphertext is indistinguishable from random. Git's packfile machinery spends CPU on zlib + delta search for ~0% gain.
2. **Git never forgets.** Every chunk ever written stays in history forever. A 115 MB bundle rewritten weekly hits GitHub's **10 GB on-disk repository limit** in under two years, and there is no cheap GC — you'd need history rewrites plus a GitHub Support request to reclaim space.
3. **Chunk count fights the tree limits.** `GET /git/trees` with `recursive=1` truncates at **100,000 entries / 7 MB**, and repos cap at **3,000 entries per directory** and **50 levels deep**. Thousands of chunk files need a sharded `ab/cdef…` layout just to stay legal.

Release assets have none of these properties: immutable, content-addressed by filename, individually deletable (real GC), **2 GiB each, 1,000 per release, "no limit on the total size of a release, nor bandwidth usage."**

### 1.3 Where `gh` still earns its keep

Not as transport — as a *credential source* and a *user-facing escape hatch*:

```
gh auth token                       # borrow an existing token (never persist it)
gh repo create <name> --private     # what you tell the user to run instead of you calling POST /user/repos
```

Both are optional and degrade to a manual instruction when `gh` is absent.

---

## 2. Auth

### 2.1 Recommendation

| Rank | Mechanism | Why |
|---|---|---|
| **Primary** | **Fine-grained PAT**, "Only select repositories" → the one backup repo. Permissions: **`Contents: Read and write`** + **`Metadata: Read`** (mandatory, auto-selected). | Blast radius = one repo. This app sits next to the user's AI provider credentials; a leaked token must not be a skeleton key. |
| **Fallback A** | **Reuse an existing credential**: `$GITHUB_TOKEN`/`$GH_TOKEN` → `git credential fill` → `gh auth token`. | Zero user work when the machine is already authenticated. `git credential fill` transparently picks up gh's helper, `osxkeychain`, `libsecret`, or GCM. |
| **Fallback B (opt-in)** | **OAuth Device Flow** with the `repo` scope. | One-click, no copy-paste, no `gh` needed. But `repo` grants read+write on **every** repo the user can touch — surface that in the consent prompt, don't make it the default. |

### 2.2 Required scopes / permissions, by endpoint

| Endpoint | Fine-grained PAT | Classic scope |
|---|---|---|
| `GET /repos/{o}/{r}` (visibility gate) | `Metadata: read` | `repo` |
| `POST /repos/{o}/{r}/releases` | `Contents: write` | `repo` |
| `POST https://uploads.github.com/repos/{o}/{r}/releases/{id}/assets` | `Contents: write` | `repo` |
| `PUT /repos/{o}/{r}/contents/{path}` | `Contents: write` | `repo` |
| `POST /user/repos` (**avoid** — see §3) | `Administration: write` | `repo` (private) / `public_repo` (public) |

Note the catch that kills auto-creation: a fine-grained PAT restricted to "only select repositories" **cannot create a repo that doesn't exist yet**, and granting `Administration: write` account-wide is exactly the over-privilege the primary choice exists to avoid.

### 2.3 Device Flow mechanics (if you ship Fallback B)

Register an **OAuth App**. `client_id` is public and shippable in the binary; **no client secret is needed for device flow.** Device flow must be explicitly enabled in the app's settings.

```
POST https://github.com/login/device/code
  Accept: application/json
  { "client_id": "...", "scope": "repo" }
→ { device_code, user_code, verification_uri: "https://github.com/login/device",
    expires_in: 900, interval: 5 }

POST https://github.com/login/oauth/access_token
  Accept: application/json
  { "client_id": "...", "device_code": "...",
    "grant_type": "urn:ietf:params:oauth:grant-type:device_code" }
```

Poll at `interval` seconds. Handle: `authorization_pending` (keep polling), `slow_down` (**add 5 s** to the interval), `expired_token` (codes die at 900 s — restart), `access_denied`, `device_flow_disabled`. Ceilings: 50 user-code submissions/hour/app, 2,000 OAuth access-token requests/hour. OAuth App tokens do not expire by default; if the app opts into expiring tokens you get an 8-hour `access_token` + 6-month `refresh_token`.

**Skip the `oauth2 5.0` crate.** This is two `POST`s and two serde structs — roughly 80 lines against the reqwest client already in the tree. `oauth2` drags in its own HTTP plumbing for no benefit.

### 2.4 Storage

**macOS** — reuse the existing `security-framework` write path. `src/anthropic/keychain.rs` already establishes the invariant that matters: *reads may shell out to `security(1)`, writes go through Security.framework so the secret never enters process arguments.* Same rule applies here. Generic password item, service `ai-usagebar`, account `github:<owner>/<repo>`.

**Linux** — mode-0600 file under `$XDG_DATA_HOME/ai-usagebar/` (or the cache dir resolved by `cache::xdg_cache_dir`), matching the existing convention and what `gh` itself does when no keyring is available.

**Do not add `keyring` 4.1 or `secret-service` 5.1.** `secret-service` pulls the whole `zbus` stack, requires a running D-Bus session + unlocked keyring (fails headless / over SSH — a first-class case for a *backup restore* tool), and the project already has both halves of the platform story implemented.

**Never write the token into `config.toml`.** The existing inline-API-key convention is a deliberate user choice for read-only provider keys; a `Contents: write` GitHub token is a different class of secret.

---

## 3. Repo bootstrap and the visibility gate

### 3.1 Don't create the repo

Ask the user for `owner/repo` that already exists and is private. If it doesn't exist, print:

```
Repo <owner>/<repo> not found. Create it first:
    gh repo create <owner>/<repo> --private
  or  https://github.com/new?name=<repo>&visibility=private
```

This removes `Administration: write` from the token entirely — meaning **the app is structurally incapable of creating a public repo.** That's a stronger guarantee than any runtime check.

*(If product requirements later force auto-creation: `POST /user/repos` with `{"name": "...", "private": true}`, classic `repo` scope or account-wide `Administration: write`. Note that **GitHub Apps cannot create repos in a personal account at all** — org-only, via `POST /orgs/{org}/repos` — which is another reason not to build on a GitHub App. MEDIUM confidence: community-discussion sourced, not in the endpoint docs.)*

### 3.2 The hard safety gate

```
GET /repos/{owner}/{repo}
  Accept: application/vnd.github+json
  X-GitHub-Api-Version: 2022-11-28
```

Assert **all** of the following before any write:

- `private == true` **and** `visibility == "private"` (check both; `visibility` also carries `"internal"`, which is *not* private enough for credential-bearing data — reject it)
- `owner.login` equals the configured owner **and** `owner.id` equals the id recorded at first pairing (defeats a delete-and-resquat of the name)
- `archived == false`, `fork == false`
- 404 ⇒ hard abort with the "create it first" message; never interpret 404 as "so let's make one"

**Re-check immediately after the upload completes, before the manifest flip.** If it reads public on the second check, treat it as a security incident: delete the uploaded assets, delete the release, refuse the manifest flip, and tell the user to rotate every credential in the bundle. You cannot un-publish bytes that were public — say so plainly rather than pretending the check made it safe.

**The visibility check is defense in depth, not the control.** The control is that the bundle is encrypted before it ever touches the network. Design the code so that ordering is structurally enforced (encrypt → upload takes ciphertext only; there is no code path that hands plaintext to the uploader).

**Drift detection:** persist `{repo_id, owner_id, private, checked_at}` in the local state file. A background/`--status` check re-reads visibility and screams if it flipped. Also worth surfacing: `GET /repos/{o}/{r}` returning `"visibility": "public"` when local state says private is a *user-initiated* change, so the message should be "your backup repo is now public" not a generic error.

---

## 4. Upload efficiency — the actual numbers

### 4.1 GitHub's limits

| Limit | Value | Source |
|---|---|---|
| Single file (git) — warning | 50 MiB | large-files docs |
| Single file (git) — **hard block** | **100 MiB** | large-files docs |
| Single **push** (pack size) | **2 GB, enforced** — `remote: fatal: pack exceeds maximum allowed size` | repository-limits / 2 GB push docs |
| Repository on-disk size | **10 GB** (recommended < 1 GB, strongly < 5 GB) | repository-limits |
| Entries per directory / tree depth / branches | 3,000 / 50 / 5,000 | repository-limits |
| **Pushes per repo** | **6 per minute** | repository-limits |
| Git read ops per repo | 15 per second | repository-limits |
| **Release asset — max size** | **2 GiB per file** | about-releases |
| **Release — assets per release** | **1,000** | about-releases |
| **Release — total size / bandwidth** | **"no limit on the total size of a release, nor bandwidth usage"** | about-releases |
| Git Data blob — create/get | get documented at **100 MB**; create size not documented | rest/git/blobs |
| Contents API `GET` | ≤ 1 MB full support; 1–100 MB needs `raw`/`object` media type; **> 100 MB unsupported** | rest/repos/contents |
| Tree API `recursive=1` | truncates at **100,000 entries / 7 MB** | rest/git/trees |
| **Primary rate limit** | **5,000 req/hr** authenticated (15,000 on Enterprise Cloud) | rate-limits |
| **Secondary: concurrency** | **100 concurrent** requests (REST + GraphQL combined) | rate-limits |
| **Secondary: points** | **900 points/min**; GET/HEAD/OPTIONS = **1 pt**, POST/PATCH/PUT/DELETE = **5 pts** | rate-limits |
| **Secondary: content creation** | **80/min and 500/hour** | rate-limits |
| Secondary: CPU | 90 s CPU per 60 s real time | rate-limits |
| Git LFS quota | 10 GiB storage + 10 GiB bandwidth (Free/Pro); 250 GiB (Team/Enterprise Cloud) | billing/git-lfs |

### 4.2 What this means for a 115 MB bundle

Assume 1 MiB content-defined chunks → ~115 chunks. (At 64 KiB chunks: ~1,840.)

| Strategy | Requests | Points | Wall clock (limit-bound) | Verdict |
|---|---|---|---|---|
| One release asset **per chunk** | 115 POST | 575 | **≥ 2 min** (80 content-creations/min), and only **500/hr** — a 5,000-chunk multi-GB bundle takes **10 hours** | ❌ |
| Contents API **per chunk** | 115 PUT | 575 | same 80/min ceiling, **plus 115 commits** of permanent history | ❌❌ |
| Git Data API (blob per chunk + 1 tree + 1 commit + 1 ref) | 118 POST | 590 | same content-creation ceiling; 1 commit but 115 permanent blobs | ❌ |
| `git push` of one packfile | 1 push | n/a | instant, but 6 pushes/min/repo, 2 GB/push cap, and permanent retention | ⚠️ fallback only |
| **Pack chunks into ≤1.9 GiB assets** | **1 POST** | **5** | **network-bound, not limit-bound** | ✅ |

**The single most important design decision: never upload one object per chunk.** Concatenate chunks into packs and record `(pack_id, offset, length)` per chunk in the manifest. A multi-GB bundle becomes `ceil(size / 1.9 GiB)` requests — 3 requests for a 5 GB bundle.

Corollary: **LFS is irrelevant** and should be avoided. The 10 GiB free bandwidth quota would be exhausted by ~87 restores of a 115 MB bundle, and exceeding it *disables LFS on the whole account until the next month* — a backup tool that can brick its own restore path on a quota is not a backup tool.

### 4.3 Rate-limit handling

Treat `403` and `429` identically as backoff signals:

1. `retry-after` header present → sleep that many seconds.
2. Else `x-ratelimit-remaining: 0` → sleep until `x-ratelimit-reset` (UTC epoch seconds).
3. Else → exponential backoff with jitter, min 60 s.

Cap concurrency at **4** in-flight uploads. The documented ceiling is 100 concurrent, but with 2 GiB bodies you are bandwidth-bound long before you are request-bound, and a low cap keeps a laptop's uplink usable.

---

## 5. Failure modes and atomicity

### 5.1 The sync protocol

```
1. LOCAL      build manifest: chunks, pack layout, per-chunk AEAD nonces,
              pack names = "pack-<sha256-of-ciphertext>.bin"   (content-addressed)
2. GATE       GET /repos/{o}/{r}          → assert private/visibility/owner (§3.2)
3. RELEASE    GET /repos/{o}/{r}/releases/tags/{tag}
              else POST /repos/{o}/{r}/releases {tag_name, draft: true}
4. RESUME     GET /repos/{o}/{r}/releases/{id}/assets
              skip assets where name matches AND size matches AND state == "uploaded"
              DELETE any asset whose state != "uploaded" (torn upload) before retry
5. UPLOAD     POST https://uploads.github.com/repos/{o}/{r}/releases/{id}/assets?name=<pack>
              Content-Type: application/octet-stream
              body = streamed file (reqwest `stream` feature)
              concurrency 4, backoff per §4.3
6. GATE       GET /repos/{o}/{r}         → re-assert private. Public ⇒ INCIDENT (§3.2)
7. PUBLISH    PATCH /repos/{o}/{r}/releases/{id} {draft: false}
8. FLIP       PUT /repos/{o}/{r}/contents/manifest.json
                { message, content: <base64>, sha: <previous manifest blob sha> }
              409 ⇒ someone else pushed; re-read and re-plan
9. GC         DELETE /repos/{o}/{r}/releases/assets/{asset_id} for superseded generations
```

**Steps 1–7 are invisible to any reader.** Readers resolve chunks only through the manifest, so a half-finished upload is unreachable. Step 8 is the single linearization point — *that* is the atomicity. Step 9 must run only after a successful flip, and must only delete assets not referenced by the current manifest.

### 5.2 Specific failures

| Failure | Detection | Recovery |
|---|---|---|
| **Network drop mid-asset** | Asset appears in the list with `state != "uploaded"` or a short `size` | Delete the asset, re-upload. GitHub creates the asset record before the body finishes, so a truncated upload leaves a zombie — always check `state`, never just the name. *(MEDIUM: `state` transitions aren't fully documented; verify with an `#[ignore]`d live test.)* |
| **Partial upload / resume** | Step 4's name+size+state set difference | Free: content-addressed names mean a changed chunk gets a new name. No manifest of "what I uploaded last time" is needed. |
| **Mid-file resume** | — | **Not supported.** The upload endpoint is a single POST with no Range/multipart. This is why packs are capped at ~1.9 GiB rather than exactly 2 GiB — a failure costs at most one pack, and you want that pack re-sendable within a reasonable window. |
| **Conflicting remote state** | `409` from the Contents `PUT` (stale `sha`) | Re-`GET` the manifest, compare generations. Never blind-overwrite: another machine's newer manifest may reference assets you're about to GC. |
| **Repo went public mid-sync** | Step 6 re-check | Incident path: delete assets + release, refuse the flip, instruct credential rotation. |
| **Token revoked / expired** | `401` | Clear the stored token, re-run the auth flow. Distinguish `401` (bad token) from `403` (rate limit or missing permission) — they need opposite responses. |
| **1,000-asset ceiling per release** | Asset count at step 4 | With ~1.9 GiB packs that is ~1.9 TB per release. If you ever hit it, roll to a new tag. |
| **Repo deleted / renamed** | `404` at step 2 | Hard abort. Never auto-recreate — a 404 could be a name squat. |
| **Restore-side body cap** | — | `vendor::read_body_capped` / `MAX_BODY_BYTES` exists for small JSON responses. The bundle download path **must not** use it; stream to a temp file and `persist()` (the existing atomic-write convention). |

---

## 6. Crates

### 6.1 Recommended: zero new dependencies

| Need | Use what's already here |
|---|---|
| HTTP | `reqwest` 0.12 (rustls) — **add the `stream` feature** so `Body: From<tokio::fs::File>` and `Body::wrap_stream` are available; both are gated on `stream`. Without it a 2 GiB pack gets buffered in RAM. |
| Content addressing | `sha2` 0.11 — already present. Do **not** add `blake3`; SHA-256 is fast enough at 115 MB and free. |
| JSON / base64 / temp files / atomic writes | `serde_json`, `base64` 0.23, `tempfile` 3, `cache::atomic_write` — all present |
| macOS secret storage | `security-framework` 3 — present, and `src/anthropic/keychain.rs` already encodes the write-via-framework-not-argv rule |
| Concurrency + backoff | `tokio` (already features `rt`, `time`) — a `Semaphore` and `tokio::time::sleep` are the whole scheduler |
| Subprocess (`git credential fill`, `gh auth token`) | `tokio` `process` feature — already enabled |

### 6.2 Crates to reject, with reasons

| Crate | Current | Why not |
|---|---|---|
| `git2` 0.21 | — | Compiles libgit2 C at build time; the `https` feature pulls `openssl-sys` + `openssl-probe`, adding `openssl` to AUR `depends` (today: only `gcc-libs`) and contradicting the deliberate rustls-only posture. Note `git2`'s default features are `[]`, so HTTPS is not free — you must opt into the OpenSSL path. |
| `gix` 0.86 | — | **Push is unimplemented.** Disqualifying. Also ~45 sub-crates of compile time on every AUR source build. |
| `oauth2` 5.0 | — | Device flow is two POSTs. Brings its own HTTP abstraction for no gain. |
| `keyring` 4.1 / `secret-service` 5.1 | — | `zbus` stack; requires a live D-Bus session and unlocked keyring — fails headless and over SSH, which is exactly the restore scenario. Both platform halves already exist in this repo. |
| `octocrab` | — | Not evaluated in depth, but it wraps a *different* reqwest instance and version-pins it; you'd be carrying two HTTP stacks. The ~8 endpoints here are trivial to hand-roll against the existing client. |
| `zeroize` 1.9 | — | Optional, tiny, no build cost. Only add if a threat model explicitly requires scrubbing token buffers. Not required by anything above. |

*(Aside: `reqwest` 0.13.4 is the current max-stable. The project pins 0.12 and nothing here requires the bump; the `stream` feature exists in both.)*

### 6.3 Test hermeticity — non-negotiable per `CLAUDE.md`

Every network and filesystem entry point needs the seam the repo already uses (`kilo`, `cursor`, `kiro`, `openrouter`, `novita`, `moonshot` all have one):

```rust
#[derive(Debug, Clone)]
pub struct Endpoints {
    pub api_base: String,      // https://api.github.com
    pub uploads_base: String,  // https://uploads.github.com
}
```

- Tests point both at one `mockito::Server` — including uploads, since `uploads.github.com` is a *separate host* and hard-coding it makes the upload path untestable.
- Token resolution behind `token::read_from(&Path)` / an injected provider, never a `$HOME` resolver.
- The `git credential fill` / `gh auth token` subprocess must sit behind an injectable fn so no test ever spawns a process or touches the user's real credential helper.
- Time-dependent logic (rate-limit reset, token expiry) takes `now: DateTime<Utc>` — mirroring `antigravity::parse_cache_at` and `fetch_snapshot_at`.
- Anything touching a real repo or a real token lives in `tests/live.rs` behind `#[ignore]`.

Note the AUR build runs `cargo test` inside `makepkg`'s `check()` — a test that reaches `api.github.com` or reads a real token fails the *install* on a user's machine.

### 6.4 Items still needing live verification (write them as `#[ignore]`d tests)

1. **Range GETs on private-repo release assets.** `GET /repos/{o}/{r}/releases/assets/{id}` with `Accept: application/octet-stream` returns 200 or a 302 to a signed storage URL. Whether that URL honours `Range:` determines if a *partial* restore (fetch 3 chunks out of a 1.9 GiB pack) is possible, or whether restore must pull whole packs. **This materially affects pack sizing** — if Range works, bigger packs are strictly better; if not, pack size should track the expected restore granularity.
2. Whether release-asset creation counts against the **80/min, 500/hr content-creation** limit (assume yes; the packing design makes it moot either way).
3. `POST /git/blobs` maximum body size — undocumented on the create side. Irrelevant to the recommended design; relevant only if the Git Data API fallback is ever built.
4. `state` field transitions on a torn asset upload (§5.2).

---

## 7. Non-technical risk worth surfacing to the user

GitHub's Acceptable Use Policies §9 (Excessive Bandwidth Use): *"If we determine your bandwidth usage to be significantly excessive in relation to other users of similar features, we reserve the right to suspend your Account, throttle your file hosting, or otherwise limit your activity."* GitHub also reserves the right to remove repositories placing *"undue strain on our infrastructure."*

There is no explicit prohibition on using a private repo for backups, but a multi-GB bundle rewritten frequently is exactly the profile that draws attention. Two mitigations that are also just good engineering:

- Content-addressed packs mean an unchanged pack is never re-uploaded. Steady-state traffic should be proportional to *changed* data, not bundle size.
- GC superseded generations (step 9) rather than accumulating them.

Document the risk in user-facing docs. Do not silently make the user's account a storage tier.

---

## Sources

- [Rate limits for the REST API](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api) — 5,000/hr; 100 concurrent; 900 points/min; GET=1pt, POST/PATCH/PUT/DELETE=5pt; 80/min + 500/hr content creation; 90 s CPU / 60 s
- [Repository limits](https://docs.github.com/en/repositories/creating-and-managing-repositories/repository-limits) — 10 GB repo, 100 MB object, 2 GB push, 6 pushes/min, 15 git reads/sec, 3,000 entries/dir, 50 depth, 5,000 branches
- [About large files on GitHub](https://docs.github.com/en/repositories/working-with-files/managing-large-files/about-large-files-on-github) — 50 MiB warning, 100 MiB block, 25 MiB browser upload
- [Troubleshooting the 2 GB push limit](https://docs.github.com/en/get-started/using-git/troubleshooting-the-2-gb-push-limit) — `remote: fatal: pack exceeds maximum allowed size`
- [About releases](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases) — "Up to 1000 release assets… Each file… under 2 GiB. There is no limit on the total size of a release, nor bandwidth usage."
- [REST: Release assets](https://docs.github.com/en/rest/releases/assets) — `POST https://uploads.github.com/repos/{o}/{r}/releases/{id}/assets?name=`, `Accept: application/octet-stream` for private downloads
- [REST: Repository contents](https://docs.github.com/en/rest/repos/contents) — 1 MB / 100 MB `GET` thresholds, `PUT` with `sha`
- [REST: Git trees](https://docs.github.com/en/rest/git/trees) — 100,000 entries / 7 MB recursive limit, `base_tree` semantics
- [REST: Git blobs](https://docs.github.com/en/rest/git/blobs) — `GET` up to 100 MB
- [REST: Repos (create/get)](https://docs.github.com/en/rest/repos/repos) — `repo` scope for private repo creation
- [Permissions required for fine-grained PATs](https://docs.github.com/en/rest/authentication/permissions-required-for-fine-grained-personal-access-tokens) — Administration:write for `POST /user/repos`; Contents:write for git-database + contents writes; Metadata:read for `GET /repos`
- [Authorizing OAuth apps (device flow)](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps) — endpoints, 900 s expiry, `slow_down` +5 s, 50 codes/hr, `device_flow_disabled`
- [Git LFS billing](https://docs.github.com/en/billing/concepts/product-billing/git-lfs) — 10 GiB Free/Pro, 250 GiB Team/Enterprise; LFS disabled on bandwidth overage
- [GitHub Acceptable Use Policies](https://docs.github.com/en/site-policy/acceptable-use-policies/github-acceptable-use-policies) — §9 Excessive Bandwidth Use
- [gitoxide crate-status.md](https://github.com/GitoxideLabs/gitoxide/blob/main/crate-status.md) — `[ ] push`, `[ ] send-pack / receive-pack client plumbing`
- [git2 0.21 on crates.io](https://crates.io/crates/git2/0.21.0) — default features `[]`; `https` → `openssl-sys` + `openssl-probe`
- [gix 0.86 on crates.io](https://crates.io/crates/gix/0.86.0) — `blocking-http-transport-reqwest-rust-tls` feature; ~45 `gix-*` deps
- [reqwest 0.12 `Body`](https://docs.rs/reqwest/0.12.24/reqwest/struct.Body.html) — `wrap_stream` and `From<tokio::fs::File>` gated on the `stream` feature
- [GitHub CLI: `gh auth setup-git`](https://cli.github.com/manual/gh_auth_setup-git) — `gh auth git-credential` as a git credential helper
- Community discussion: [GitHub App cannot create a repo in a personal account](https://github.com/orgs/community/discussions/171040) — MEDIUM confidence, not in official docs
