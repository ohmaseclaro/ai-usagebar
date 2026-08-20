---
phase: 06-surfaces-and-ship
plan: 05
subsystem: docs + release packaging
status: complete
tags: [docs, release, changelog, aur, srcinfo, ux-05, ux-06, d-05, fork-divergence]
requires:
  - "6-01 — `sync status --json` and the menu-bar Sync row, documented here"
  - "6-02 — the Sync submenu's confirm-and-delegate design, and `sync push` prompting at a terminal"
  - "6-03 — the widget's exit-0 contract, and its correction of D3's 'different binaries' wording"
  - "6-04 — the TUI Sync section, and the exact `categories = []` TOML quoted in the README"
  - "docs/sync-github.md, docs/sync-format.md, docs/sync-calibration.md — the shipped format and setup docs"
provides:
  - "README `## Encrypted sync` — setup, the PAT recipe, daily use, both surfaces, D-05, and the five honest limits"
  - "CHANGELOG `## [1.2.0]` — Added / Changed / Fixed / Security, both link lines"
  - "1.2.0 in Cargo.toml, Cargo.lock, manifest.json, both PKGBUILDs, both .SRCINFOs"
affects:
  - "the human who tags — see 'The tag is blocked' below; this is not a formality"
tech-stack:
  added: []
  patterns:
    - "the .SRCINFOs are makepkg output, produced in an archlinux container rather than hand-edited on a Mac"
key-files:
  created: []
  modified:
    - README.md
    - docs/sync-format.md
    - CHANGELOG.md
    - Cargo.toml
    - Cargo.lock
    - manifest.json
    - packaging/aur/PKGBUILD
    - packaging/aur/PKGBUILD-bin
    - packaging/aur/.SRCINFO
    - packaging/aur/.SRCINFO-bin
decisions:
  - "the existing `## Sync (optional)` section was rewritten in place as `## Encrypted sync` rather than adding a second section — one sync section, not two that drift"
  - "makepkg is Arch-only and this is an Apple-Silicon Mac, so it ran in `archlinux:base-devel` under `--platform linux/amd64` as a non-root user. That is makepkg output, which is what the task's precondition protects; the precondition's ban is on *hand-editing*"
  - "the CHANGELOG Security block opens by saying none of it ever shipped — the feature is new in this release — because a Security section a user reads as 'I was exposed' is a false alarm"
  - "no `Fixed` entry was invented for sync-internal defects: the whole feature is new, so those belong under Security's framing, not as fixes to something users had"
  - "the PKGBUILD `url=` was left pointing at akitaonrails/ai-usagebar. Changing it is the maintainer's call, not an executor's — see below"
metrics:
  duration: ~2h
  completed: 2026-08-20
---

# Phase 6 Plan 05: Docs and release preparation — Summary

The README says what the feature does and, in its own subsection, what it does not
promise. The CHANGELOG carries a `[1.2.0]` section whose Security block is this
milestone's own audit findings written the way a user meets them. Six version strings
agree, both PKGBUILDs are reset for CI, and both `.SRCINFO`s were regenerated with
`makepkg --printsrcinfo` **before** any tag exists.

**Nothing was tagged and nothing was pushed.** That is Task 3's blocking human
checkpoint, and there is a second reason it must stay human — below.

---

## ⚠️ THE TAG IS BLOCKED — a maintainer decision, not a formality

This is the finding a reader of this summary needs before anything else. **This fork
has diverged from upstream, and the release cannot ship as prepared without a decision
that is not an executor's to make.**

The facts, checked rather than assumed:

| | |
|---|---|
| `main` in this repo | `0198a5c` — *is* `v1.1.0`, exactly. `Cargo.toml` there reads `1.1.0` |
| `v1.2.0` | **already exists as a tag**: `e49a4f4` "Release v1.2.0", 2026-08-18 |
| `v1.2.0`, `v1.3.0`, `v1.3.1` | none is an ancestor of our `main` (`git merge-base --is-ancestor` → false for all three) |
| where they come from | `origin` **and** `upstream` both point at `github.com/akitaonrails/ai-usagebar`. `fork` points at `github.com/ohmaseclaro/ai-usagebar` |
| this milestone's base | `milestone/encrypted-sync`, branched from `v1.1.0` — it does not contain upstream's 1.2.0, 1.3.0 or 1.3.1 |

Three consequences:

1. **`git tag -a v1.2.0` will fail.** The name is taken by upstream's release. CLAUDE.md
   forbids force-moving a tag, and rightly: it would rewrite a public release pointer.
2. **The AUR source package would build the wrong code, not merely fail to fetch.**
   `packaging/aur/PKGBUILD` has `url="https://github.com/akitaonrails/ai-usagebar"` and
   `source=(…$url/archive/refs/tags/v$pkgver.tar.gz)`. At `pkgver=1.2.0` that resolves
   to **upstream's** v1.2.0 tarball, which exists and contains upstream's code — so
   `makepkg` would succeed and ship something that is not this milestone.
   `PKGBUILD-bin` has the same problem against upstream's release assets, and CI's
   `publish-aur` would pin sha256s for upstream's tarballs.
3. **CLAUDE.md step 7's `git push origin main` pushes to `akitaonrails/ai-usagebar`,**
   not to the fork.

**I deliberately did not "fix" the PKGBUILD `url=`.** Repointing it at
`ohmaseclaro/ai-usagebar` is a publishing decision with an AUR-package-ownership tail,
and silently making it would have hidden the divergence behind a green diff.

**What a maintainer must decide before any of this ships:**

- **Which repository is the release target** — the fork or upstream. If the fork, both
  PKGBUILDs' `url=` must change, and whoever owns the AUR `ai-usagebar` /
  `ai-usagebar-bin` packages has to agree, because those package names point at
  upstream today.
- **Which version number.** `1.2.0` is an orchestrator decision made under the
  assumption that 1.1.0 was the latest; it is not, upstream is at 1.3.1. Renumbering is
  a mechanical redo of this plan's Task 2 — one `sed`, one `cargo update -p ai-usagebar`,
  one container run to regenerate the `.SRCINFO`s.
- **Whether to merge or rebase onto upstream `v1.3.1` first.** This milestone sits on
  `v1.1.0` and misses two upstream releases. Releasing from it publishes a fork that has
  silently reverted whatever those contained.

Everything else in this plan is correct as written and survives any of those choices,
except the six version strings, which are one command apart from any other number.

---

## Task 1 — the README section

`## Sync (optional)` was rewritten in place as **`## Encrypted sync`** (README.md:233),
linked from `## Reference guides` the way the other guides are. Rewritten rather than
added beside, so there is one sync section rather than two that drift.

It covers, in order:

- **Setup** — the tool never creates a repository *and cannot*, because the token is
  configured without `Administration: write`; the fine-grained PAT recipe in GitHub's
  own words (**Only select repositories** → the one repo, **Contents: Read and write**,
  **Metadata: Read**, Administration left unchecked); `[sync] repo = "owner/name"`; and
  `ai-usagebar sync setup`'s five steps, including that it uploads nothing and writes
  nothing until the last one.
- **Day to day** — a table of all six subcommands, `--json` on `status`, `--dry-run` on
  `push`, and `pull` as a dry run by default. It states that `--force` does **not** grant
  `--force-credentials`, that the two are a separate consent, and that the archive and
  its undo command are printed.
- **Exit codes** — `sync` exits non-zero; the Waybar render path, *the same binary with
  no subcommand*, always exits 0 with `⚠`. **"One binary with `sync` as a subcommand,
  two deliberately opposite contracts."** 6-03's correction is honoured: the phrase
  "different binary" does not appear.
- **Surfaces** — the menu bar's Sync row and submenu (confirm, show the command, open
  Terminal.app — the menu never runs a sync), and the TUI's Sync section (toggle and
  save, no sync action). D-02 is stated flatly, and corrected against 6-02's finding:
  there is **no** "already unlocked" session key anywhere in the tool, so it is not
  "a password you have not yet unlocked" — every push, pull, prune and rekey reads the
  password fresh.
- **What has no sync surface** — GNOME, KDE and Omarchy, written as the recorded
  decision D-05 is, with the reason (independent frontends, own contract suites) so a
  reader does not file it as a bug.
- **The honest limits**, its own subsection, five of them: no password recovery;
  changing the password is not revocation, *including for data written after the
  change*; the accepted metadata leakage (total size, timing, per-sync change volume);
  GitHub's AUP **§9 excessive bandwidth use**, linked to the live anchor
  (`…/github-acceptable-use-policies#9-excessive-bandwidth-use`, verified 200 and the
  anchor id confirmed present) rather than paraphrased; and the "sync nothing" state,
  quoting 6-04's exact emitted TOML and keeping "missing key" and `categories = []`
  apart.

`docs/sync-format.md` gained the cross-link back to that README section. Its link to
`sync-calibration.md` already existed (line 554), so it was not duplicated.

**Every claim was checked against the shipped code, not against this plan's prose** —
the six subcommands and every flag against `SyncAction` in `src/widget/cli.rs` and
against `ai-usagebar sync --help` from the built 1.2.0 binary; the token stores and the
five setup steps against `docs/sync-github.md`; the `categories = []` TOML against
6-04's SUMMARY; the metadata list against `sync-format.md` §8.

---

## Task 2 — versions, CHANGELOG, packaging

**1.2.0 is in six places and they agree** (the plan's automated check passes):
`Cargo.toml`, `Cargo.lock`, `manifest.json`, `packaging/aur/PKGBUILD`,
`packaging/aur/PKGBUILD-bin`, `packaging/aur/.SRCINFO`, `packaging/aur/.SRCINFO-bin`.
The built binary reports `ai-usagebar 1.2.0`.

`Cargo.lock` was refreshed with **`cargo update -p ai-usagebar`**, never a bare
`cargo update` (T-6-46). `git diff --stat Cargo.lock` → **1 insertion, 1 deletion**: the
package's own version line and nothing else.

**Is 1.2.0 the right *kind* of bump?** Checked against the SUMMARYs rather than assumed.
6-03 proved `SyncConfig` carries `#[serde(default)]` and does **not** deny unknown
fields, so a config written by a newer build loads on an older one with its known values
intact and the widget renders normally. No existing surface's contract changed. A minor
is correct — and it is recorded in the CHANGELOG's `Changed` section rather than
smuggled.

**CHANGELOG `## [1.2.0] — 2026-08-20`**, above `[1.1.0]`, grouped Added / Changed /
Fixed / Security. `[Unreleased]` now compares from `v1.2.0`, and `[1.2.0]` was added at
the bottom comparing `v1.1.0...v1.2.0`.

The **Security** block is the six defects this milestone found and closed, written as
what could have happened to the reader rather than as finding IDs:

| written as | what it was |
|---|---|
| two machines could silently drop one machine's backup | NEW-1 / 4-08 — both publishing at one counter, the anchor reading the collision as "already seen" |
| the rollback defence was documented but never consulted, and prune then deleted the orphans for good | NEW-2 / T-4-04 — three doc comments, zero reads on the push path; the laundered pointer past `PRUNE_GRACE` |
| a password change could be quietly undone by a machine that missed it | NEW-3 / T-4-45 — the stale machine re-uploading the wrapper the rekey destroyed |
| one capital letter defeated the credential confirmation | F-1 / 5-09 — the byte-exact basename against a case-folding kernel; `--force` alone reverting a live OAuth token. F-2's device-identity leak is named in the same entry, same root cause |
| a hostile backup could rewrite what you saw on your terminal | F-3 + NEW-1 / 5-09 — `{}` on a remote string, deliverable on demand because the path had no length bound |
| sync cannot take your status bar down | 6-03 — the exit-0 contract, now asserted through the shipped render path plus the structural gate |
| the menu bar never runs a sync itself, and never holds your password | 6-02 — confirm-and-delegate instead of a subprocess passing `--apply --yes --force-credentials` |

The block opens by saying plainly that the feature is new in this release, so none of it
was ever present in a shipped build. A Security section a user reads as "I was exposed"
when they were not is a false alarm, and this project's whole documentation posture is
that an overclaim costs more than the thing it hides.

**Both PKGBUILDs**: `pkgver=1.2.0`, `pkgrel=1`, `sha256sums='SKIP'` on the source one and
**both** `sha256sums_x86_64` and `sha256sums_aarch64` `'SKIP'` on the binary one. CI's
`publish-aur` pins the real hashes.

### Both `.SRCINFO`s were regenerated with makepkg, not hand-edited

This is the v0.17.0 step, and the task's precondition says a non-Arch host must stop
rather than hand-edit — a `.SRCINFO` disagreeing with its PKGBUILD is exactly what
`verify-version` rejects (T-6-42). `makepkg` is not on this Mac. **Rather than
hand-editing or stopping, it ran in an Arch container**, which produces genuine
`makepkg --printsrcinfo` output:

```bash
docker run --rm --platform linux/amd64 -v "$SCRATCH:/w" archlinux:base-devel bash -c '
  useradd -m b && chown -R b /w
  su b -c "cd /w/src && makepkg --printsrcinfo > .SRCINFO"
  su b -c "cd /w/bin && makepkg --printsrcinfo > .SRCINFO-bin"'
```

Three details that will bite whoever repeats this: `--platform linux/amd64` is required
(the `archlinux` image has no arm64 manifest and the plain pull fails on Apple Silicon);
`makepkg` refuses to run as root, hence the `useradd`; and `PKGBUILD-bin` was copied into
its own scratch directory **named `PKGBUILD`**, per CLAUDE.md step 5, because `makepkg`
reads that filename and nothing else.

The resulting diff is four lines in `.SRCINFO` and eight in `.SRCINFO-bin` — `pkgver`,
`provides`, and the source URLs — with every other byte identical to the previous
makepkg output. `sha256sums = SKIP` is preserved in both.

### `kde-plasmoid/package/metadata.json` — verified unchanged, not assumed

The plan asked for this to be checked rather than trusted, and for the result to be
recorded so the next release knows whether the check is routine.

```
git diff --stat 0eebaa4..HEAD -- kde-plasmoid/ gnome-extension/ omarchy/   → empty
git diff --stat v1.1.0..HEAD  -- kde-plasmoid/ gnome-extension/ omarchy/   → empty
```

Empty not only for Phase 6 but for **the entire milestone since v1.1.0**. So
`KPlugin.Version` stays at `0.1.0`, and the GNOME `metadata.json` is untouched too.
D-05 held in the diff, not just in the decision record. **The check is worth keeping**:
it is the one version in this repo that moves independently, and this is the release
where it would have been bumped by reflex.

---

## The gate — every command, and what it did

| Gate | Result |
|---|---|
| `make test` | **green** — 1573 lib + 60 integration = **1633 passed, 0 failed, 16 ignored**, plus `marker logic tests passed`, `plasmoid logic tests passed`, `Omarchy model tests passed` |
| `cargo clippy --all-targets -- -D warnings` | **clean**, exit 0 |
| `cargo fmt --check` | **clean**, exit 0 |
| `cargo machete` | **clean** — "didn't find any unused dependencies" |
| `omarchy plugin validate .` | **NOT RUN — the `omarchy` CLI is not installed on this machine and is not available for macOS.** Not "passed". It must be run on the Linux box before the tag |
| `./macos/run-tests.sh` | **green** — **239 assertions**, exit 0 |
| `ai-usagebar --version` | `ai-usagebar 1.2.0` |
| version agreement (the plan's automated check) | **passes** — all six strings at 1.2.0, CHANGELOG section present |

Counts against the stated baseline: **1573 lib / 1633 total, 0 failing** — exactly the
baseline, as expected for a plan that changes no Rust. Swift **239**, also exactly the
baseline; this plan touched no Swift, and 6-01/6-02 already banked their +26 and +47.

Two notes for whoever runs this next:

- **`cargo machete` was not installed** and was installed for this gate
  (`cargo install cargo-machete --locked`). It is the tool CLAUDE.md's checklist names,
  so its legitimacy was not in question, but the install is a change to the machine's
  toolchain and is recorded rather than hidden.
- **`cargo test --lib sync::cli` hangs on an inherited open pipe** — 6-02's finding, and
  T-6-30 reproducing inside our own harness. Every test invocation here was run with
  `< /dev/null`. `make test` was too.

**The hand checks in Task 3 were not performed.** Task 3 is a `checkpoint:human-verify`
with `gate="blocking"`, and the six by-hand surface exercises (the menu bar's push with
no key, the pull dry-run dialog, the poisoned-config widget run, the TUI toggle) all
require driving a GUI and mutating the real `~/.config` and `~/.cache`. They stay with
the human, together with `omarchy plugin validate .`.

---

## Deviations from Plan

### 1. [Rule 3 — blocking] `makepkg` absent; run it in a container rather than stop

- **Found during:** Task 2, step 6.
- **Issue:** the task's precondition stops a non-Arch host, because the alternative it
  anticipated was hand-editing — which T-6-42 correctly forbids.
- **Fix:** neither. `makepkg --printsrcinfo` ran under `archlinux:base-devel`, so the
  committed files *are* makepkg output. The precondition's property — "the `.SRCINFO`
  agrees with its PKGBUILD because the same tool derived it" — holds.
- **Commit:** cffbee0

### 2. [design] The README section was rewritten in place, not added

`## Sync (optional)` already existed and already carried a partial version of this
material. A second `## Encrypted sync` section would have left two descriptions of one
feature to drift apart. The old heading is gone; the anchor `#encrypted-sync` is what
`Reference guides` and `docs/sync-format.md` now point at.

### 3. [design] No invented `Fixed` entries, and a framed `Security` block

The plan asked for four Keep-a-Changelog groups. All four have real content, but the
sync-internal defects are under **Security** with an explicit "none of this ever
shipped" framing rather than under **Fixed**, because a user installing 1.2.0 has never
had this feature and cannot have been affected. `Fixed` carries the one thing that is
genuinely a fix for a user of an existing surface (the Settings overlay's 80×24 scroll),
and `Changed` carries the `[sync]` config-section compatibility note 6-03 asked to be
carried here.

### 4. [correction] D-02 restated against 6-02's finding

The plan's wording is "an operation needing a password you have not already unlocked".
6-02 established there is **no session-key state in this codebase at all** — every
password is read fresh. The README says that, because the plan's phrasing would have
implied an unlock state a user could go looking for.

---

## Known Stubs

None. No placeholder, no TODO, and no skipped test was introduced. Two things are
**deliberately unrun rather than stubbed**, both recorded above and neither hidden:
`omarchy plugin validate .` (tool unavailable on macOS) and Task 3's six by-hand surface
checks (blocking human checkpoint).

## Threat Flags

None new. This plan adds no code, no endpoint, no auth path and no schema change; it
touches documentation, version strings and packaging metadata. Its own registered
threats resolve as follows:

| Threat | Disposition |
|---|---|
| T-6-40 overstated guarantees | mitigated — the five limits are their own subsection, stated flatly, and the read-aloud pass rewrote three hedged sentences |
| T-6-41 a stale `.SRCINFO` against a bumped PKGBUILD | mitigated — regenerated before any tag, and the six-string check passes |
| T-6-42 a hand-edited `.SRCINFO` on a non-Arch host | mitigated — makepkg in a container; nothing hand-edited |
| T-6-43 a test reading a real `$HOME` failing the AUR `check()` | mitigated — `make test` green, 0 failing, and this plan added no test |
| T-6-44 an unmovable wrong tag | **mitigated, and then some** — no tag was created, and the divergence above is precisely the wrong tag it would have been |
| T-6-47 tagging a commit the gate never ran against | mitigated — two commits, `git status` clean, the gate ran on exactly that tree |
| T-6-45 a secret in a release artifact | mitigated — the diff is docs, versions and packaging; no credential, token or key appears in it |
| T-6-46 an unrelated dependency moving | mitigated — `cargo update -p ai-usagebar`; `Cargo.lock` diff is one line |
| T-6-SC dependency surface | Phase 6 added zero crates. The **milestone** added six (`argon2`, `chacha20poly1305`, `blake3`, `zstd`, `zeroize`, `getrandom`), all pure Rust or vendoring their C, so the AUR source build still needs no system `-dev` package. `cargo machete` clean |

## Release status

- **Version prepared:** 1.2.0. **Not tagged. Not pushed. No AUR interaction.**
- **`publish-aur`:** did not run and cannot — it is triggered by a tag push. The manual
  fallback was not used either.
- **Blocking before any tag:** the divergence decision at the top of this document, then
  `omarchy plugin validate .` on a Linux box, then Task 3's hand checks.

## Self-Check: PASSED

- `README.md` — FOUND (modified, `## Encrypted sync` at line 233)
- `docs/sync-format.md` — FOUND (modified)
- `CHANGELOG.md` — FOUND (`## [1.2.0] — 2026-08-20` at line 12; both link lines at 1671–1672)
- `Cargo.toml`, `Cargo.lock`, `manifest.json` — FOUND, all at 1.2.0
- `packaging/aur/PKGBUILD`, `PKGBUILD-bin`, `.SRCINFO`, `.SRCINFO-bin` — FOUND, all at 1.2.0
- Commit `42d0f22` — FOUND
- Commit `cffbee0` — FOUND
