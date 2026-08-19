---
phase: 06-surfaces-and-ship
plan: 05
type: execute
wave: 3
depends_on: [06-01, 06-02, 06-03, 06-04]
files_modified:
  - README.md
  - CHANGELOG.md
  - Cargo.toml
  - Cargo.lock
  - manifest.json
  - docs/sync-format.md
  - packaging/aur/PKGBUILD
  - packaging/aur/PKGBUILD-bin
  - packaging/aur/.SRCINFO
  - packaging/aur/.SRCINFO-bin
autonomous: false
requirements: [UX-05, UX-06]
must_haves:
  truths:
    - "The README documents the fine-grained PAT recipe and states plainly that there is no password recovery and that changing the password is not revocation."
    - "The README records the accepted metadata leakage and GitHub's AUP §9 excessive-bandwidth clause, so a user who gets throttled is not surprised by it."
    - "The README documents that GNOME, KDE and Omarchy have no sync surface and that the CLI is the path there — a recorded decision, not a silent absence (D-05)."
    - "`Cargo.toml` and the root Omarchy `manifest.json` carry the same version, and it matches the tag."
    - "Both PKGBUILDs are bumped with `pkgrel=1` and `sha256sums` reset, and **both `.SRCINFO`s are regenerated before tagging** — v0.17.0 never shipped for exactly this omission."
    - "`make test`, `cargo clippy --all-targets -- -D warnings`, `cargo machete`, and `omarchy plugin validate .` are all clean, and `./macos/run-tests.sh` is clean on a Mac."
    - "The tag is pushed by a human, never by an agent — tags are immutable and a wrong one cannot be moved."
  artifacts:
    - "The README sync section: setup, the PAT recipe, the surfaces, and the honest limits"
    - "A CHANGELOG `## [X.Y.Z] — YYYY-MM-DD` section with Added / Changed / Fixed / Security, and both link lines updated"
    - "Matched versions in Cargo.toml, Cargo.lock, and manifest.json"
    - "Bumped packaging/aur/PKGBUILD and PKGBUILD-bin, and regenerated packaging/aur/.SRCINFO and .SRCINFO-bin"
  key_links:
    - "The `.SRCINFO` regeneration is what the release workflow's `verify-version` job checks; skipping it fails the tag after it is already immutable"
    - "`PKGBUILD-bin` must be copied to a scratch dir named `PKGBUILD` for `makepkg --printsrcinfo`, then the output copied back as `.SRCINFO-bin`"
    - "`make test` covers cargo plus the GNOME, KDE and Omarchy contract suites; those suites are untouched by this phase, so any failure there is a regression, not an expected diff (D-05)"
    - "`./macos/run-tests.sh` is not part of `make test` — it needs `swiftc`. It is a separate, mandatory gate for a phase that changed Swift"
    - "`kde-plasmoid/package/metadata.json`'s `KPlugin.Version` is bumped only if that tree changed; D-05 guarantees it did not, and this plan verifies that rather than assuming it"
---

<objective>
Ship it. Write the documentation the feature cannot be used or trusted without, then take the
release through `CLAUDE.md`'s checklist in full.

Two things make this plan worth its own slot. First, the honest limits: no password recovery,
rekey is not revocation, the metadata that leaks even when everything works, and GitHub's
bandwidth clause. A backup tool that oversells its guarantees is worse than one that has fewer
of them, because the user calibrates their behaviour to what the README claimed.

Second, the checklist has a step this project has already been burned by. v0.17.0 never
shipped because the `.SRCINFO`s were not regenerated before tagging, and the tag could not be
moved. That step is a task here, not a bullet in a comment.

Purpose: turn a working feature into a released one, with claims that hold.
Output: README, CHANGELOG, matched versions, both PKGBUILDs, both `.SRCINFO`s, and a green gate.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/06-surfaces-and-ship/6-CONTEXT.md
@.planning/phases/06-surfaces-and-ship/6-01-SUMMARY.md
@.planning/phases/06-surfaces-and-ship/6-02-SUMMARY.md
@.planning/phases/06-surfaces-and-ship/6-03-SUMMARY.md
@.planning/phases/06-surfaces-and-ship/6-04-SUMMARY.md
@CLAUDE.md
@README.md
@CHANGELOG.md
@docs/sync-format.md
</context>

<source_audit>
This plan covers the ROADMAP's docs and release deliverables and closes out both phase
requirements. The phase-wide audit is in 6-01. `6-CONTEXT.md` numbers its decisions `D1`…`D5`,
cited here as `D-01`…`D-05`.

| Source | Item | Covered by |
|---|---|---|
| ROADMAP | "Docs — README sync section, the PAT recipe, and the honest limits: no password recovery, password change is not revocation, the accepted metadata leakage, and GitHub's AUP §9 excessive-bandwidth clause" | this plan |
| ROADMAP | "Release checklist per `CLAUDE.md`: versions matched, CHANGELOG section, both PKGBUILDs bumped, **both `.SRCINFO`s regenerated before tagging**, then `make test` + clippy + `cargo machete` + `omarchy plugin validate .`" | this plan |
| ROADMAP | Success criterion 4: all four gate commands clean | this plan |
| ROADMAP | Success criterion 5: the README documents the PAT recipe and states plainly that there is no password recovery and that changing the password is not revocation | this plan |
| REQ | UX-05 / UX-06 — implemented in 6-01…6-04; this plan is where they reach a user, through documentation and a shipped artifact | 6-01…6-04, this plan |
| CONTEXT | D-05 GNOME, KDE and Omarchy out of scope — recorded as a decision, not an oversight | this plan documents it in the README; their contract suites run as regression gates |
</source_audit>

<tasks>

<task type="auto">
  <name>Task 1: The README sync section, and the limits stated plainly</name>
  <precondition>Phases 3–5 shipped the setup command, the PAT recipe, and the rekey semantics; `docs/sync-format.md` and `docs/sync-calibration.md` already record the format and the measured numbers. Read all three plus every Phase 6 SUMMARY first, and document what shipped — never a shape inferred from this plan.</precondition>
  <files>README.md, docs/sync-format.md</files>
  <action>
Add a `## Encrypted sync` section to `README.md`, placed with the other feature sections and
linked from `## Reference guides` the way the existing guides are. Keep it a *usage* section:
the on-disk format stays in `docs/sync-format.md`, and this section links there rather than
restating it.

Cover, in this order:

**Setup.** The guided command as it actually shipped, the fine-grained PAT recipe verbatim from
Phase 3's documentation — "Only select repositories" → the one repo, `Contents: Read and write`
plus `Metadata: Read`, nothing else — and the fact that the tool will not create the repository
for you, because withholding that permission is what makes a push to a public repo structurally
impossible rather than merely discouraged.

**Day to day.** `sync status`, `sync push`, `sync pull`, `--dry-run` on both, and `--json` for
scripting. Note that these exit non-zero on failure, and that the Waybar render path — the same
`ai-usagebar` binary invoked with no subcommand — has the opposite contract: it always exits 0,
because Waybar hides modules that do not. One binary, two deliberately opposite exit contracts.
Do not write "a different binary"; `6-CONTEXT.md`'s D3 uses that phrasing and it is wrong, as
6-03's SUMMARY records. The contract split it describes is right.

**Surfaces.** The macOS menu bar's Sync submenu and the TUI's Sync section, each with what it
can and cannot do. State D-02 explicitly: a surface cannot prompt, so an operation needing a
password you have not already unlocked will tell you to run it in a terminal. That is a
designed behaviour and a user who meets it should recognise it as one.

**What is not covered (D-05).** GNOME, KDE and Omarchy have no sync surface. The CLI is the
path on those desktops. Write it as the deliberate decision it is — each is an independent
frontend with its own contract suite, and the CLI already covers the need — so a reader does
not file it as a bug.

**The honest limits.** A subsection of its own, not footnotes:
- There is no password recovery. None. Lose the password and the backup is unreadable, by design.
- Changing the password is not revocation. It rewraps the master key; anyone who already cloned
  the repo keeps what they took. Say so in those words.
- The accepted metadata leakage: total bundle size, sync timing, and per-sync change volume are
  visible to anyone who can see the repository, even though the contents are not.
- GitHub's AUP §9 excessive-bandwidth clause — the repo is a backup store, and a user hammering
  it can be throttled. Link it rather than paraphrasing the terms.
- The "sync nothing" state: an empty category selection is legal and is preserved, quoting the
  exact TOML 6-04's SUMMARY recorded.

Add the two cross-links `docs/sync-format.md` needs: to this README section, and to
`docs/sync-calibration.md` if that link is not already there.

Documentation that describes behaviour no plan shipped is worse than none, because the next
reader trusts it. Every claim here must be one you can execute against the built binary.
  </action>
  <verify>
    <automated>node -e "const t=require('fs').readFileSync('README.md','utf8');const need=['## Encrypted sync','Contents: Read and write','Metadata: Read'];const missing=need.filter(s=>!t.includes(s));if(missing.length){console.error('missing:',missing);process.exit(1)}console.log('README sync section present')"</automated>
    <human-check>Read the limits subsection aloud. If any sentence hedges a guarantee the tool does not make, rewrite it flatter.</human-check>
  </verify>
  <done>The README has a sync section covering setup, the PAT recipe, daily use, both surfaces, the out-of-scope frontends, and the five limits — every claim checkable against the built binary.</done>
</task>

<task type="auto">
  <name>Task 2: Versions, CHANGELOG, both PKGBUILDs, and both `.SRCINFO`s</name>
  <precondition>`makepkg` is Arch-only. On any other host, `.SRCINFO` regeneration cannot run locally and this task must stop and hand off rather than hand-editing the files — a hand-edited `.SRCINFO` that disagrees with its PKGBUILD is exactly what the release workflow rejects.</precondition>
  <files>Cargo.toml, Cargo.lock, manifest.json, CHANGELOG.md, packaging/aur/PKGBUILD, packaging/aur/PKGBUILD-bin, packaging/aur/.SRCINFO, packaging/aur/.SRCINFO-bin</files>
  <action>
The version is `1.2.0` — a feature milestone on 1.1.0, no breaking change to any existing
surface. Confirm that against the SUMMARYs before writing it; if 6-03 had to change the config
types in a way an older build cannot read, that is a major, and it must be argued in the
CHANGELOG rather than smuggled into a minor.

1. `Cargo.toml` `version`, and refresh `Cargo.lock` with `cargo update -p ai-usagebar` — not a
   full `cargo update`, which would move unrelated dependencies inside a release commit.
2. The root Omarchy `manifest.json` `version`, matching exactly.
3. `CHANGELOG.md`: a new `## [1.2.0] — <today>` section above `## [1.1.0]`, entries grouped
   Added / Changed / Fixed / Security per Keep a Changelog. The exit-0 work from 6-03 and the
   non-interactive refusal from 6-02 belong under **Security**: one prevents a denial of service
   on the user's status bar, the other closes a path where a background process could sit
   holding a worker forever. Update the `[Unreleased]` compare link and add the new release link
   at the bottom.
4. `packaging/aur/PKGBUILD`: `pkgver=1.2.0`, `pkgrel=1`, `sha256sums` reset to `'SKIP'`.
5. `packaging/aur/PKGBUILD-bin`: same `pkgver`, `pkgrel=1`, and **both**
   `sha256sums_x86_64` and `sha256sums_aarch64` reset to `'SKIP'`. CI pins the real hashes later.
6. Regenerate both `.SRCINFO`s **now, before any tag exists**:
   ```
   cd packaging/aur && makepkg --printsrcinfo > .SRCINFO
   t=$(mktemp -d) && cp PKGBUILD-bin "$t/PKGBUILD" && \
     (cd "$t" && makepkg --printsrcinfo > .SRCINFO-bin) && \
     cp "$t/.SRCINFO-bin" .SRCINFO-bin && rm -rf "$t"
   ```
   The scratch-directory dance is not optional: `makepkg` reads a file named `PKGBUILD` and
   nothing else.

Then confirm what did *not* change: `kde-plasmoid/package/metadata.json`'s `KPlugin.Version` is
bumped only when that tree changes, and D-05 kept it out of the phase. Verify with
`git diff --stat` against the phase's base rather than trusting the decision, and record the
result. It is the one version in this repo that moves independently, and it is therefore the
one most easily bumped by reflex when it should not be.
  </action>
  <verify>
    <automated>node -e "const fs=require('fs');const v=fs.readFileSync('Cargo.toml','utf8').match(/^version\s*=\s*\"([^\"]+)\"/m)[1];const m=JSON.parse(fs.readFileSync('manifest.json','utf8')).version;const g=s=>(fs.readFileSync(s,'utf8').match(/^\s*pkgver\s*=\s*(\S+)/m)||[])[1];const all={cargo:v,manifest:m,pkgbuild:g('packaging/aur/PKGBUILD'),pkgbuildbin:g('packaging/aur/PKGBUILD-bin'),srcinfo:g('packaging/aur/.SRCINFO'),srcinfobin:g('packaging/aur/.SRCINFO-bin')};const bad=Object.entries(all).filter(([,x])=>x!==v);if(bad.length){console.error('version mismatch',all);process.exit(1)}if(!fs.readFileSync('CHANGELOG.md','utf8').includes('## ['+v+']')){console.error('CHANGELOG has no section for '+v);process.exit(1)}console.log('all versions agree at '+v)"</automated>
  </verify>
  <done>Six version strings agree, the CHANGELOG has its section and both links, both PKGBUILDs are reset to `'SKIP'`, both `.SRCINFO`s were regenerated from those PKGBUILDs, and the plasmoid version is confirmed unchanged.</done>
  <reversibility rating="reversible">Everything here is a file edit before any tag exists. The one-way step is Task 3.</reversibility>
</task>

<task type="checkpoint:human-verify" gate="blocking">
  <name>Task 3: The gate, the hand checks, and the tag</name>
  <what-built>Phase 6 in full: `sync status --json` and the menu-bar state row (6-01), the menu-bar push/pull triggers and the CLI's non-interactive refusal (6-02), the widget exit-0 gate (6-03), the TUI Sync section (6-04), and this plan's docs plus release preparation. **Task 2's version, CHANGELOG and packaging edits are already committed as the release commit** — this checkpoint runs the gate and tags that commit. Nothing is tagged yet.</what-built>
  <how-to-verify>
Run the full gate. On a Mac, `omarchy plugin validate .` will not be available; run it on the
Linux box, or state that it was skipped and why.

```
make test                                   # cargo test + GNOME, KDE and Omarchy contract suites
cargo clippy --all-targets -- -D warnings
cargo machete
cargo fmt --check
omarchy plugin validate .
./macos/run-tests.sh                        # macOS only — not part of `make test`
```

Then exercise the surfaces by hand, because none of the above can:
1. `ai-usagebar sync status --json | python3 -m json.tool` — one object, `last_sync` and the
   pending summary present.
2. Open the menu bar. The Sync row shows a last-sync time. The Sync submenu offers push and pull.
3. Trigger a push with no unlocked key. It must report "run it in a terminal" **within a
   second** and must not hang. This is the D-02 check and it is the one worth doing slowly.
4. Trigger a pull. Confirm the dry-run output appears in the dialog *before* anything is written.
5. Break the config (`printf 'not toml' >> config.toml`), run the widget, and confirm it prints
   one line of JSON with `⚠` and `echo $?` is 0. Restore the config.
6. Open the TUI, press `s`, toggle a sync category, Ctrl-S, and confirm `config.toml` changed,
   its comments survived, and it is still mode 0600.

Then, and only then, tag — by hand, not through an agent. Task 2 already made the release
commit, so do **not** commit again here; a second commit would leave the tag pointing at an
empty change or, worse, at a commit the gate never ran against. `git status` should be clean
before you tag; if it is not, whatever is uncommitted has not been through the gate.

```
git status                                    # must be clean
git tag -a v1.2.0 -m "v1.2.0 — encrypted GitHub sync"
git push origin main && git push origin v1.2.0
```

**Tags are immutable.** Do not force-move one once pushed; cut a patch instead. CI builds both
architectures and publishes the release; `publish-aur` pins the real sha256s and pushes to both
AUR repos when `AUR_SSH_KEY` is configured. If it is not, follow `CLAUDE.md`'s manual fallback —
including the `git fetch && git reset --hard origin/master` in each AUR clone before touching it.
  </how-to-verify>
  <resume-signal>Reply "tagged v1.2.0" once CI is green, or describe what failed.</resume-signal>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|---|---|
| README claims → user behaviour | A user calibrates how they treat an unrecoverable password to what the documentation promised |
| PKGBUILD / `.SRCINFO` → an installer's machine | These files drive a build and a `check()` that runs on someone else's computer |
| repo → immutable public tag | A pushed tag cannot be corrected |
| CI secrets → AUR push | The release workflow holds an SSH key that writes to two public package repositories |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|---|---|---|---|---|---|
| T-6-40 | Repudiation | overstated guarantees in the README | high | mitigate | The five limits are their own subsection, stated flatly: no recovery, rekey is not revocation, the named metadata leakage, AUP §9, and the "sync nothing" state. A human read-aloud check catches hedging that a grep cannot |
| T-6-41 | Tampering | a stale `.SRCINFO` shipped against a bumped PKGBUILD | high | mitigate | Both are regenerated with `makepkg --printsrcinfo` *before* tagging, and an automated check asserts all six version strings agree. This is the v0.17.0 failure, made a gate |
| T-6-42 | Tampering | a hand-edited `.SRCINFO` on a non-Arch host | high | mitigate | The task's precondition stops and hands off rather than hand-editing; a `.SRCINFO` that disagrees with its PKGBUILD is what the release workflow rejects |
| T-6-43 | Denial of service | a test that reads a real `$HOME` failing the AUR `check()` | high | mitigate | Every Phase 6 plan forbids it in its own verification; `make test` here is the last gate before the tag, run once over all of it |
| T-6-44 | Tampering | an unmovable wrong tag | high | mitigate | Tagging is a blocking human checkpoint, never an agent action, and the checkpoint restates that tags are immutable and that the fix is a new patch version |
| T-6-47 | Tampering | tagging a commit the gate never ran against | medium | mitigate | Task 2 makes the release commit; the checkpoint tags it and does not commit. `git status` must be clean before tagging, so anything uncommitted is visibly outside what was gated |
| T-6-45 | Information disclosure | a real API key or token in a release artifact | critical | mitigate | `git diff` review at the checkpoint plus the repo's standing rule that no real key is ever committed; the release commit touches only docs, versions and packaging |
| T-6-46 | Tampering | an unrelated dependency moving inside a release commit | medium | mitigate | `cargo update -p ai-usagebar` only, never a bare `cargo update`; `cargo machete` runs in the gate |
| T-6-SC | Tampering | dependency surface for the whole phase | medium | mitigate | Phase 6 adds **zero** new crates and zero new Swift dependencies, so no package-legitimacy audit applies. `cargo machete` and the AUR source build's "no system `-dev` package" constraint are both re-checked at the gate, and `Cargo.toml` appears in this plan's `files_modified` for the version line only |
</threat_model>

<verification>
- `make test` (cargo plus the GNOME, KDE and Omarchy contract suites) is green. Those three
  suites are untouched by this phase (D-05), so a failure there is a regression.
- `cargo clippy --all-targets -- -D warnings`, `cargo machete`, and `cargo fmt --check` are clean.
- `omarchy plugin validate .` is clean, or its absence is stated with a reason.
- `./macos/run-tests.sh` is green on a Mac. It is not part of `make test` — it needs `swiftc` —
  and a phase that changed Swift cannot skip it.
- The version check in Task 2 passes: six strings agree and the CHANGELOG has its section.
- `git diff --stat` for the whole phase shows nothing under `gnome-extension/`, `kde-plasmoid/`,
  or `omarchy/`, and `kde-plasmoid/package/metadata.json` is unchanged.
- No tag is created by an agent.
</verification>

<success_criteria>
The README documents the fine-grained PAT recipe and states plainly that there is no password
recovery and that changing the password is not revocation. All four gate commands plus the Swift
harness are clean. Six version strings agree, both `.SRCINFO`s were regenerated before tagging,
and a human tagged and pushed the release.
</success_criteria>

<output>
Create `.planning/phases/06-surfaces-and-ship/6-05-SUMMARY.md` when done.

Record the shipped version, the gate results (including anything skipped and why), and whether
`publish-aur` ran or the manual AUR fallback was needed. Record explicitly that
`kde-plasmoid/package/metadata.json` was verified unchanged — the next release will look here to
learn whether that check is routine.
</output>
