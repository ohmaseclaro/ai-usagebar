---
phase: 01-encrypted-bundle-core
plan: 08
subsystem: sync
tags: [calibration, argon2id, range-requests, format-documentation, residual-risk, tofu]

# Dependency graph
requires: [1-01, 1-02, 1-03, 1-04, 1-05, 1-09]
provides:
  - "tests/live.rs — cal3_argon2id_timing_at_production_parameters, an #[ignore]d timing probe at the shipped KDF parameters plus two steps down"
  - "tests/live.rs — cal1_range_on_private_release_asset, an #[ignore]d, credential-gated, cleanly-skipping probe for ranged private-release-asset reads"
  - "docs/sync-format.md — the on-disk format written down: key hierarchy, keyfile, frame, pack, object graph, versioning, calibrations, accepted leakage, honest limits"
  - "A measured Argon2id curve (1 GiB / 512 MiB / 256 MiB) with the machine named"
affects: [1-06, 1-07, phase-2, phase-3, phase-4, phase-5]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "A calibration probe asserts its own parameters against the shipped default, so it cannot drift into measuring something nobody runs"
    - "A credential-gated live probe skips with a printed message rather than failing, and never follows a redirect that would replay its token to a third host"

key-files:
  created:
    - docs/sync-format.md
  modified:
    - tests/live.rs
    - README.md

key-decisions:
  - "CAL-3 is measured on Apple M3 Max / aarch64 macOS / release, and the document says so — the aarch64 Linux number the roadmap wanted was not obtained and is stated as not obtained, in those words"
  - "No Docker aarch64 Linux run: a Linux VM on this same M3 Max silicon answers the OS question, not the slow-hardware question CAL-3 exists to ask, and recording it risks being read as a clearance for constrained targets"
  - "CAL-1 was left unrun rather than half-run — it needs a real token and a throwaway private repo, and a fabricated answer is worse than the recorded fallback. The probe and its exact invocation are in place"
  - "CAL-1 disables reqwest's automatic redirect following: the hop to signed storage is the thing being measured, and following it silently would replay the GitHub token to a storage host that neither needs nor should see it"
  - "Only the storage host is printed on the redirect, never the signed URL — its query string is a credential and the probe's output goes to a terminal"
  - "The document describes the format as built, not as the plan imagined it: multi-chunk manifest and index (1-09), plaintext-addressed chunk ids, and the keyed header id in the pack trailer"

patterns-established:
  - "Format documentation states which numbers were measured and which are fallbacks, using the word 'not measured' rather than quoting an estimate"

requirements-completed: [CRYPTO-02, CRYPTO-06]

coverage:
  - id: D1
    description: "Both calibration probes exist, are #[ignore]d, and are listed by the ignored-test listing, so the checkpoint's instructions are runnable as written"
    requirement: CRYPTO-02
    verification:
      - kind: command
        ref: "cargo test --test live -- --ignored --list | grep -c -E 'cal(1|3)_' -> 2"
        status: pass
    human_judgment: false
  - id: D2
    description: "Plain `cargo test --test live` runs neither probe — 0 passed, 11 ignored"
    requirement: CRYPTO-02
    verification:
      - kind: command
        ref: "cargo test --test live -> test result: ok. 0 passed; 0 failed; 11 ignored"
        status: pass
    human_judgment: false
  - id: D3
    description: "A measured Argon2id timing exists at the shipped m=1 GiB / t=3 / p=1, with machine, architecture, and build profile named, plus the 512 MiB and 256 MiB steps down"
    requirement: CRYPTO-06
    verification:
      - kind: command
        ref: "cargo test --release --test live -- --ignored --nocapture cal3_argon2id_timing_at_production_parameters -> 1503/1492/1548 ms at 1 GiB across three runs"
        status: pass
    human_judgment: false
  - id: D4
    description: "The CAL-3 probe cannot calibrate parameters the build does not ship — its first row is asserted equal to KdfParams::default()"
    requirement: CRYPTO-06
    verification:
      - kind: unit
        ref: "tests/live.rs — assert_eq!(PRODUCTION, KdfParams::default())"
        status: pass
    human_judgment: false
  - id: D5
    description: "CAL-1 skips cleanly and prints why when GSD_CAL1_TOKEN is absent, and never hangs — 30 s client timeout, no automatic redirect following"
    requirement: CRYPTO-02
    verification:
      - kind: manual
        ref: "deferred live verification — see 1-08-SUMMARY 'Deferred live verification (CAL-1)'"
        status: deferred
    human_judgment: true
  - id: D6
    description: "docs/sync-format.md documents the key hierarchy, keyfile, frame, pack including the keyed header id, object graph, the at-or-below version rule, both calibrations, the accepted leakage, and the honest limits"
    requirement: CRYPTO-02
    verification:
      - kind: command
        ref: "test -s docs/sync-format.md && grep -qi 'trust-on-first-use' && grep -qi 'no password recovery|no recovery' && grep -q 'fixed-256k'"
        status: pass
    human_judgment: false
  - id: D7
    description: "No estimate is presented as a measurement: CAL-1 is stated as not measured and its fallback named as a fallback"
    requirement: CRYPTO-02
    verification:
      - kind: manual
        ref: "docs/sync-format.md §7 — 'This was not run' / 'No aarch64 Linux measurement was obtained'"
        status: pass
    human_judgment: true

# Metrics
duration: 40min
completed: 2026-08-19
status: complete
---

# Phase 1 Plan 08: Calibrations and the Format Document Summary

**CAL-3 is measured — 1 GiB / t=3 / p=1 costs ~1.5 s on an Apple M3 Max, and the curve down to
256 MiB is recorded so a user on constrained hardware can lower the parameter knowingly. CAL-1 was
not run and says so in those words; its 32 MiB fallback was already baked into `PACK_TARGET`, so
nothing is blocked. `docs/sync-format.md` records the format as built.**

## Performance

- **Duration:** ~40 min
- **Tasks:** 3/3 (Task 2 is the deferred checkpoint, below)
- **Files:** 1 created (`docs/sync-format.md`, 612 lines), 2 modified (`tests/live.rs` +269,
  `README.md` +1)
- **Commits:** `90736f3` (test), `9f260b8` (docs)

## CAL-3 — measured

Three runs, release profile, on **Apple M3 Max, 36 GiB, macOS (Darwin 25.5.0), aarch64,
rustc 1.96.0**:

| Memory | t | p | Run 1 | Run 2 | Run 3 |
|---|---|---|---|---|---|
| 1024 MiB | 3 | 1 | 1503 ms | 1492 ms | 1548 ms |
| 512 MiB | 3 | 1 | 701 ms | 816 ms | 779 ms |
| 256 MiB | 3 | 1 | 336 ms | 376 ms | 380 ms |

The research figure this calibration existed to check was **1582 ms** on an M3 Max. The
implementation measures 1492–1548 ms on the same class of machine, so the estimate was sound — and
it remains an M3 Max number. Cost is close to linear in the memory parameter, which is the
actionable part: halving `--kdf-memory` roughly halves both the wait and the attacker's cost per
guess.

**No aarch64 Linux number was obtained**, and the document says so rather than implying one. No
slow aarch64 Linux machine was reachable during this phase. Docker is installed on this host and
could have produced an aarch64 Linux timing, and that was deliberately *not* done: a Linux VM on
this same M3 Max silicon answers a question nobody asked (does the Linux toolchain differ?) rather
than the one CAL-3 exists to ask (what does this cost on slow hardware?), and publishing it beside
the macOS row invites it to be read as a clearance for constrained targets.

The plan's documented fallback therefore applies verbatim and is recorded as a fallback: `m = 1 GiB`
stays the default, `KdfParams` travels in the keyfile and is settable at initialisation, and
`crypto::check_memory_budget` refuses actionably (naming `--kdf-memory`) rather than letting Argon2
OOM. Scaling the table, a target four times slower derives in ~6 s at 1 GiB and ~1.5 s at 256 MiB.

## Deferred live verification (CAL-1)

**Not run. This does not block the phase** — the 32 MiB fallback is already the value in
`src/sync/pack.rs` (`PACK_TARGET`), and `1-03` shipped with it, so an unrun CAL-1 changes no code
and leaves no gap. If the probe later returns `206 Partial Content`, a future phase may *raise* the
pack target; nothing else in the format moves.

The probe is written, `#[ignore]`d, and credential-gated. Collect this into the milestone's deferred
live-verification checklist:

```bash
# Setup, all throwaway:
#   1. Create a private GitHub repository.
#   2. Publish one release carrying an asset a little over 1 MiB
#      (large enough that a whole-body 200 is unmistakable, small enough
#      to download inside the probe's 30 s timeout).
#   3. Mint a fine-grained PAT scoped to that repo, Contents: read.

GSD_CAL1_TOKEN=<fine-grained read-only PAT> \
GSD_CAL1_REPO=owner/throwaway-repo \
GSD_CAL1_ASSET=payload.bin \
  cargo test --test live -- --ignored --nocapture \
    cal1_range_on_private_release_asset

# Then: delete the throwaway repository and revoke the token.
```

**What to report:** the printed status code and `Content-Range`.

- `206` with a `Content-Range` header → ranged reads work; `PACK_TARGET` may be raised in Phase 3,
  and `docs/sync-format.md` §7 should be updated from "not measured" to the observed result.
- `200` with the whole body → the recorded fallback was right; update §7 to say it was confirmed.

The probe prints its own verdict line in both cases. It **skips** with a printed message when
`GSD_CAL1_TOKEN` is unset or blank, so it is never a hard failure, and it asserts that the release
lookup itself succeeded — a 401 or 404 from a broken setup fails loudly rather than being silently
recorded as "Range is unsupported".

## The probes

`cal3_argon2id_timing_at_production_parameters` — plain `#[test]`, prints
`{arch}/{os}`, the build profile, and the reported available memory, then times three memory
settings. Two details keep it honest:

```rust
const PRODUCTION: KdfParams = KdfParams { m_kib: 1_048_576, t: 3, p: 1 };
assert_eq!(PRODUCTION, KdfParams::default(),
           "the first row must be the parameters this build actually ships");
```

…so the probe cannot drift into measuring parameters nobody runs, and it prints a loud warning under
`cfg!(debug_assertions)` because a debug-build Argon2 number measures the optimiser, not the KDF.

`cal1_range_on_private_release_asset` — `#[tokio::test]`, three environment variables
(`GSD_CAL1_TOKEN`, `GSD_CAL1_REPO`, `GSD_CAL1_ASSET`), 30 s timeout, and
`redirect::Policy::none()`. Redirects are deliberately *not* followed automatically: the hop to
signed storage is the thing being measured, and reqwest following it silently would replay the
GitHub token to a storage host that neither needs nor should see it. The second hop is issued
without an `Authorization` header, and only the storage **host** is printed — a signed URL carries
its credential in the query string and this output goes to a terminal.

## docs/sync-format.md

Nine sections, in the plan's order: key hierarchy, keyfile JSON, chunking, pack layout, object
graph, versioning and evolution, calibrations, accepted leakage, honest limits.

It documents the format **as built**, which in several places is not the format the plan first
imagined:

- **Chunk ids address the raw plaintext**, never the compressed frame, with the reason stated: a
  zstd bump would otherwise re-id every chunk in every user's bundle — full re-upload, zero dedup.
  The corollary is documented too: two zstd versions may produce different *ciphertext* for one id,
  which is harmless.
- **Nothing is ever sealed under an unkeyed address.** `content_address` appears exactly once, as
  the pack's file name over bytes that are already public ciphertext.
- **The pack trailer is `<sealed header><32-byte header id><u32 LE header length>`**, and the
  document says both why the id is keyed (a pack header is the most guessable object in the format)
  and why it is nevertheless stored in the clear (`read_header` needs it as AAD and as the nonce
  source before it can decrypt, and a keyed hash reveals nothing).
- **Version checks are at-or-below a per-object ceiling**, never equality, with the write/ceiling
  table for all five objects and the reason spelled out. The chunker is checked by set membership
  for the same reason.
- **`Root` carries `chunker` and `kdf` as informational duplicates** of the keyfile's AAD-bound
  authoritative copy, so a reader can refuse before fetching anything; a disagreement between the
  two is a reportable signal, not something to silently resolve.
- **Manifest and index objects are multi-chunk** (1-09), with 1-09's measured numbers rather than an
  estimate: the default bundle is 1,558 entries / 448 KiB against a 256 KiB chunk, ~294 bytes per
  entry; at a representative 229 bytes/entry, 1,000 entries → 1 chunk, 1,600 → 2, 5,700 → 5.
- **Argon2id m = 1 GiB / t = 3 / p = 1**, with `p = 1` explained as a real decision: the `argon2`
  0.5.3 crate has no threading, so `p > 1` costs the defender (~10% worse) while handing a
  wide-SIMD attacker free intra-hash parallelism.

The pack header's own single-chunk ceiling is documented as the deliberate asymmetry it is: it is
bounded at 256 KiB of JSON, but a 32 MiB pack of 256 KiB chunks holds ~128 entries, so it is slack
rather than a limit.

Linked from `README.md`'s "Reference guides" list, alongside the four sibling docs.

## Residual risk, as recorded in §9

**First-contact TOFU on the rollback anchor is accepted risk, not mitigated risk** (flagged by 1-05
for this plan). A rollback is the one attack that produces something authenticating perfectly: an
attacker with remote write access serves an *older* root, genuinely produced by the real key.
Nothing inside the bundle can detect it — only the local monotonic counter can. A machine that has
never seen the bundle has no counter, so an attacker already controlling the remote at the moment of
the very first fetch is believed. Every fetch afterwards is protected. Closing it would require the
user to carry a counter out of band, which is a different trade than this design makes.

Two caller obligations are recorded with it:

- **The anchor lives in the config directory, never the wipeable cache** — a wiped anchor is a free
  rollback. A local attacker can delete it anyway; that is the same residual. A present-but-corrupt
  anchor is an error, never a reset.
- **`anchor::accept` decides but does not persist.** Advancing the high-water mark is the caller's
  job and must follow verification. Advancing on a *claim* means a forged high counter locks the
  user out of their own real bundle — a denial of service built out of the protection itself.

Also in §9: no password recovery; the offline, unrate-limited attack on the password; password
change is not revocation while any old keyfile survives (including in git history); and the 1 GiB
working set is not `mlock`ed, together with the honest note that the allocating AEAD API leaves a
`Vec` holding the unwrapped master key which is wiped explicitly but may have left an unreachable
copy behind after a realloc.

## §8 — what the format does not hide

Stated as a decision rather than omitted: total bundle size, sync timing, per-sync change volume,
and the approximate chunk count are all visible to anyone who can see the objects. Hiding them needs
constant-rate cover traffic, which is absurd for a usage monitor's state backup. The mitigations
that *are* in place — keyed chunk ids, power-of-two tail padding after compression, a sealed
manifest with no paths in the clear, pack headers with no names at all — are listed so nobody
mistakes them for accidents.

## Deviations from the plan

1. **Task 2's checkpoint was not taken interactively.** This run is autonomous and Phase 1
   deliberately holds no GitHub credential, so CAL-1 was left unrun with its probe in place and its
   exact invocation recorded above for the milestone's deferred live-verification checklist. This is
   the plan's own "skip" branch, which it states the phase proceeds under unchanged.

2. **CAL-3 was run on macOS/aarch64 rather than aarch64 Linux**, and the available-Docker option was
   consciously declined — reasoning in the CAL-3 section above. This is the plan's documented
   fallback, applied verbatim and labelled as a fallback.

3. **A third environment variable.** The plan named "a repository and asset from two further
   variables"; those are `GSD_CAL1_REPO` (owner/name) and `GSD_CAL1_ASSET` (the asset's file name),
   which together with `GSD_CAL1_TOKEN` make three in total. No deviation in substance.

## Test Approach

Nothing in the default `cargo test` set changed: `cargo test --test live` reports **0 passed, 11
ignored**, and both new probes are among the ignored. The only network-touching code in Phase 1 is
CAL-1, and it is `#[ignore]`d, credential-gated, and skips when unset — so the AUR `check()` still
never reaches the network, `$HOME`, or the Keychain.

Per the plan's scope, only what this plan touches was verified — no full build, no `make test`.

| Check | Result |
|---|---|
| `cargo test --test live -- --ignored --list \| grep -c -E 'cal(1\|3)_'` | 2 |
| `cargo test --test live` | 0 passed, 0 failed, 11 ignored |
| `cargo test --release --test live -- --ignored --nocapture cal3_…` | passed, 3 runs, numbers above |
| `cargo clippy --all-targets -- -D warnings` | clean |
| `cargo fmt -- --check` | clean |
| Task 3's verify (`test -s` + three greps) | pass |

## Threat Flags

- **T-08-02 (an estimate quoted as a measurement)** — mitigated. §7 says "This was not run" for
  CAL-1 and "No aarch64 Linux measurement was obtained" for CAL-3's missing half, in those words.
- **T-08-01 (the CAL-1 token)** — mitigated by construction, and one step further than the plan
  required: the probe never follows a redirect automatically, never sends the token on the second
  hop, and never prints the signed URL.
- **T-08-03 (aggregate size and sync timing)** — accepted and documented in §8.
- **T-08-04 (a 1 GiB derivation on a constrained target)** — the shipped default now rests on a
  measured number and a measured curve; `check_memory_budget` and the configurable parameter were
  already in place from 1-01.
- **T-05-04 (first-contact TOFU)** — carried forward from 1-05 and recorded in §9 as accepted risk,
  as that plan asked.

## User Setup Required

**None for the phase.** One optional, deferred item: the throwaway private repository and read-only
PAT for CAL-1, detailed under "Deferred live verification" above. The phase ships on the documented
32 MiB fallback if it is never run.

## Next Phase Readiness

**Ready.**

- **1-07** can point its known-answer vectors at a written format rather than at the source: the
  three context strings, the AAD byte order, the frame layout, and the pack trailer are all spelled
  out in `docs/sync-format.md`.
- **Phase 3** owns the CAL-1 follow-up. If the probe comes back `206`, raise `PACK_TARGET` and
  update §7 from "not measured" to the observed result; if `200`, mark the fallback confirmed.
- **Phase 2/3** must honour the recorded caller obligation on the anchor: advance the high-water
  mark only after a snapshot verifies.

## Self-Check: PASSED

`docs/sync-format.md` is 612 lines on disk and linked from `README.md`; `tests/live.rs` carries both
probes and both are listed as ignored; commits `90736f3` and `9f260b8` are in git on `gsd/1-08`;
`src/sync/` is untouched by this plan (`git diff 2feb643..HEAD --stat` lists `README.md`,
`docs/sync-format.md`, and `tests/live.rs` only).
