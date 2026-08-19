---
phase: 1-encrypted-bundle-core
plan: 08
type: execute
wave: 3
depends_on: [1-01, 1-02, 1-03, 1-04, 1-05]
files_modified:
  - tests/live.rs
  - docs/sync-format.md
autonomous: false
requirements: [CRYPTO-02, CRYPTO-06]
user_setup:
  - service: github
    why: "CAL-1 only — probing whether a private-repo release asset honours a Range request. Optional; the phase ships on the documented fallback if it is not run."
    env_vars:
      - name: GSD_CAL1_TOKEN
        source: "A throwaway fine-grained PAT scoped to one throwaway private repo, Contents: read"
    dashboard_config:
      - task: "Create a throwaway private repo with one release carrying a >1 MiB asset"
        location: "github.com -> New repository (private) -> Releases"
must_haves:
  truths:
    - "The shipped KDF default rests on a measured number, with the machine it was measured on named."
    - "A user on constrained hardware can lower the parameters knowingly instead of being locked out."
    - "The on-disk format is written down well enough that a reader could implement it without the source."
    - "The metadata this design does not hide is stated plainly rather than omitted."
  artifacts:
    - docs/sync-format.md
    - "#[ignore]d calibration probes appended to tests/live.rs"
  key_links:
    - "docs/sync-format.md is the only place the frame layout, pack layout, and object graph are written down together"
---

<objective>
Close the two calibrations the roadmap scheduled into this phase, and write the format down.

**CAL-3** — real Argon2id timing at m = 1 GiB, t = 3, p = 1. The 1582 ms in the research is an Apple
M3 Max number; the shipped default and the memory floor should not rest on it alone. This is a task,
not an assumption.

**CAL-1** — whether a private-repo release asset honours a `Range:` request after the redirect to
signed storage. It decides pack sizing. The roadmap's fallback is explicit and must not block the
phase: assume no, pack at 32 MiB.

Then `docs/sync-format.md`, recording the on-disk format, both calibration numbers, and the metadata
leakage this design accepts. **D-02 (D2)** makes the format evolvable; a written format is what makes
that promise usable by the next reader.

Purpose: turn two estimates into measurements, or into documented fallbacks.
Output: `tests/live.rs` additions, `docs/sync-format.md`.
</objective>

<execution_context>
@$HOME/.claude/gsd-core/workflows/execute-plan.md
@$HOME/.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/phases/1-encrypted-bundle-core/1-CONTEXT.md
@.planning/research/SUMMARY.md
@.planning/research/encryption.md
@.planning/phases/1-encrypted-bundle-core/1-01-SUMMARY.md
@.planning/phases/1-encrypted-bundle-core/1-02-SUMMARY.md
@.planning/phases/1-encrypted-bundle-core/1-03-SUMMARY.md
@.planning/phases/1-encrypted-bundle-core/1-04-SUMMARY.md
@tests/live.rs
@docs/configuration.md
@CLAUDE.md
</context>

<tasks>

<task type="auto">
  <name>Task 1: CAL-3 — measure Argon2id at production parameters</name>
  <files>tests/live.rs</files>
  <action>
Append an `#[ignore]`d benchmark to `tests/live.rs`, alongside the existing live vendor probes, named
`cal3_argon2id_timing_at_production_parameters`. It derives a KEK at m = 1 GiB, t = 3, p = 1, timing
it with `std::time::Instant`, and prints the elapsed milliseconds together with the target triple and
the reported available memory. It must stay `#[ignore]`d: it allocates a gibibyte and takes seconds,
and the AUR `check()` runs `cargo test` on other people's machines.

Also time m = 512 MiB and m = 256 MiB in the same run, so a user who must lower the parameter has a
curve rather than a single point.

Run it in release mode — a debug-build Argon2 number is meaningless — on whatever aarch64 Linux
target is available: a real machine first; the project's own aarch64 release runner second. If
neither is reachable, apply the fallback from `1-CONTEXT.md` verbatim: keep m = 1 GiB, record the
measured macOS number with the machine named, and rely on `KdfParams` already being a parameter
everywhere plus `crypto::check_memory_budget` refusing actionably below the requirement. Do not
record an emulated timing as if it were native; if you use emulation, label it an upper bound and
say which emulator.

Record every number obtained — value, machine, architecture, and build profile — in the summary.
Task 3 writes them into the document.
  </action>
  <verify>
    <automated>cargo test --release --test live -- --ignored --nocapture cal3_argon2id_timing_at_production_parameters</automated>
  </verify>
  <done>A measured timing exists for at least one machine with its architecture named, or the documented fallback is applied and the reason recorded. The probe is `#[ignore]`d.</done>
</task>

<task type="checkpoint:human-verify" gate="blocking">
  <name>Task 2: CAL-1 — does a private-repo release asset honour a Range request?</name>
  <what-built>An `#[ignore]`d CAL-1 probe in `tests/live.rs` that issues a ranged request against a release asset on a private repository and reports whether the server honoured it, plus a manual `curl` recipe in the summary for running the same check by hand.</what-built>
  <how-to-verify>
CAL-1 needs credentials this phase deliberately does not have — a GitHub token and a throwaway
private repository with a release asset over 1 MiB. Phase 1 is otherwise entirely offline, so this is
the one thing a human must decide about.

Two acceptable outcomes, and the phase proceeds either way:

**Run it.** Create a throwaway private repo, publish a release with an asset larger than 1 MiB, put a
fine-grained read-only PAT in `GSD_CAL1_TOKEN`, and run:
`cargo test --test live -- --ignored --nocapture cal1_range_on_private_release_asset`.
Report whether the response was `206 Partial Content` with a `Content-Range` header, or `200` with the
whole body. Then delete the throwaway repo and revoke the token.

**Skip it.** Reply "skip" and the roadmap's stated fallback applies unchanged: assume no `Range`
support, pack at 32 MiB, and Phase 3 may revisit it once HTTP plumbing exists. The pack constants
in `src/sync/pack.rs` already carry that value, so nothing changes in the code.
  </how-to-verify>
  <resume-signal>Reply with the observed status code, or "skip" to take the documented 32 MiB fallback</resume-signal>
</task>

<task type="auto">
  <name>Task 3: docs/sync-format.md</name>
  <files>docs/sync-format.md</files>
  <action>
Write `docs/sync-format.md`, matching the tone and depth of the existing files in `docs/`. Someone
holding this document and no source should be able to write a reader for the format.

Cover, in this order:

**Key hierarchy** — password, salt, Argon2id parameters, KEK, the wrapped random master key, and the
three BLAKE3 `derive_key` subkeys with their exact context strings. State that the canonical
serialization of the format and KDF fields is bound as associated data, and that this is what makes a
parameter downgrade fail rather than succeed weakly.

**Keyfile JSON** — every field, its type, and its encoding.

**Chunking** — fixed 256 KiB offset-aligned per file plus an explicit tail. The frame layout exactly
as `src/sync/chunk.rs` implements it: the true-length prefix, the compressed-length prefix, the zstd
frame, and the padding rule. Note that chunk ids are computed over the compressed frame, so a zstd
change is a format change — which is why `format_version` and the chunker identifier exist.

**Pack layout** — blob region, sealed header, trailing header length, the content-addressed sharded
name, and the size constants with the CAL-1 result or fallback that set them.

**Object graph** — root, manifest, index object, and the `supersedes` list. Draw the chain from root
to manifest id to manifest to chunk ids to chunks, and state that every hop's identifier is bound as
associated data into the object it names.

**Versioning and evolution** — what `format_version` and the chunker identifier permit, and the rule
that a reader refuses a version it does not know rather than guessing. Say why: this project has
already been bitten by a format that could not evolve.

**Calibrations** — the CAL-3 numbers from Task 1 with each machine, architecture, and build profile
named, and the CAL-1 outcome from Task 2 or its fallback. If a measurement was not obtained, say so
in those words rather than quoting the research estimate as if it were measured.

**Accepted leakage** — total bundle size, sync timing, and per-sync change volume all remain visible
to anyone who can see the objects, and hiding them would need constant-rate cover traffic. State this
as a decision. Note the mitigations that are in place: keyed chunk ids, so read access does not
permit confirming a guessed file; tail padding; and a sealed manifest carrying no paths in the clear.

**Honest limits** — there is no password recovery; changing the password is not revocation while an
old keyfile survives anywhere; the 1 GiB Argon2 working set is not `mlock`ed, because it cannot be
under a default memory-lock limit and half-measures there are theatre; and trust-on-first-use is a
real residual gap on a machine with no rollback anchor yet.

Link the document from `docs/` however the sibling files are indexed.
  </action>
  <verify>
    <automated>test -s docs/sync-format.md && grep -qi 'trust-on-first-use' docs/sync-format.md && grep -qi 'no password recovery\|no recovery' docs/sync-format.md && grep -q 'fixed-256k' docs/sync-format.md</automated>
  </verify>
  <done>`docs/sync-format.md` documents the key hierarchy, keyfile, frame, pack, object graph, versioning rule, both calibrations, the accepted leakage, and the honest limits. No estimate is presented as a measurement.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|---|---|
| CAL-1 probe → github.com | The only network call anywhere in Phase 1, `#[ignore]`d and credential-gated |
| documentation → user expectations | An overstated guarantee in a document is a security failure of its own |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|---|---|---|---|---|---|
| T-08-01 | Information disclosure | the CAL-1 token | high | mitigate | A throwaway read-only PAT on a throwaway repository, supplied through the checkpoint and revoked afterwards; the probe is `#[ignore]`d so it never runs in the AUR `check()` |
| T-08-02 | Repudiation | quoting an estimate as a measurement | medium | mitigate | The document must say when a number was not measured, in those words |
| T-08-03 | Information disclosure | aggregate size and sync timing | low | accept | Documented as accepted leakage; hiding it needs constant-rate cover traffic, which is absurd for this feature |
| T-08-04 | Denial of service | a 1 GiB derivation on a constrained target | medium | mitigate | CAL-3 sets the shipped default; `check_memory_budget` refuses actionably and the parameter is configurable at initialisation |
</threat_model>

<verification>
- The CAL-3 probe runs and prints a timing, or the fallback is applied and the reason recorded.
- Both probes are `#[ignore]`d — `cargo test` with no flags runs neither.
- `docs/sync-format.md` exists and covers every section listed above.
</verification>

<success_criteria>
1. A measured Argon2id timing exists with its machine and architecture named, or the documented fallback is applied and said to be a fallback.
2. CAL-1 is either answered with an observed status code or explicitly skipped, with the 32 MiB fallback recorded.
3. `docs/sync-format.md` describes the format completely enough to implement a reader from it.
4. The accepted metadata leakage and the honest limits are stated plainly, with no guarantee the design does not provide.
</success_criteria>

<output>
Create `.planning/phases/1-encrypted-bundle-core/1-08-SUMMARY.md` when done. Record every calibration
number with its machine, and state clearly which calibrations were measured and which fell back.
</output>
