---
phase: 02-bundle-scope-local-index-dry-run-planning
plan: 06
subsystem: infra
tags: [sync, calibration, zstd, sha2, measurement, live-tests]

requires:
  - phase: 02-bundle-scope-local-index-dry-run-planning
    provides: "`sync::scope::collect` for all five categories and `SyncRoots::resolve` (plans 2-01/2-02/2-04) — CAL-4 measures the real bundle through the real collectors"
  - phase: 01-encrypted-bundle-format
    provides: "`sync::chunk::frame` (zstd level 3 + power-of-two pad) and `CHUNK_SIZE` — CAL-4 measures the bytes that would really be stored"
provides:
  - "`docs/sync-calibration.md` — the measured CAL-4 bundle size per category, and CAL-2's capture-frequency finding plus its deferred second half"
  - "`cal4_default_bundle_compressed_size` — re-runnable measurement of this machine's real compressed bundle, with `AI_USAGEBAR_CAL4_ALL=1` to include opt-in transcripts"
  - "`cal2_desktop_state_chunk_stability` — snapshot-and-diff chunk-stability probe for `desktop-state/`, safe to run at any time, never restarts or switches anything"
affects: [2-07-dry-run, phase-4-push]

tech-stack:
  added: []
  patterns:
    - "A calibration probe *records* a snapshot per invocation and diffs against the previous one, so the state change it measures stays the user's to make and on their schedule — no test ever quits, restarts or switches a live app"
    - "A measurement that cannot distinguish 'no change' from 'nothing happened' prints INCONCLUSIVE rather than a reassuring zero"
    - "Read buffers holding collected file bodies are `zeroize::Zeroizing` — the `credentials` category is literally a pile of OAuth tokens"

key-files:
  created:
    - docs/sync-calibration.md
  modified:
    - tests/live.rs
    - docs/sync-format.md

key-decisions:
  - "CAL-2's premise was wrong and is corrected rather than measured around. The bundle does not carry Claude Desktop's live data dir; it carries `~/.claude-acc/profiles/<label>/desktop-state/`, a snapshot copy that `claude_desktop::snapshot_profile` stages and renames into place only on an account switch, with the app already quit. An app restart does not move one synced byte, so measuring across a restart would have reported 0% churn and meant nothing by it."
  - "CAL-4 reports zstd output and *stored* bytes as separate columns. `chunk::frame` rounds each sealed chunk up to a power of two (T-02-02), which costs ~40% on top of zstd's output — 8.56 MiB on the default bundle, 312 MiB on transcripts. Quoting the zstd column at a user would understate the push by 40% in the user's favour, the one direction an estimate must never be wrong in."
  - "`AI_USAGEBAR_CAL4_ALL=1` forces every category on so the opt-in transcripts whale can be calibrated without editing the user's real `config.toml`. Without it the probe measures exactly what config selects — the real default bundle, as the plan required."
  - "CAL-2 uses `sha2`, not Phase 1's keyed BLAKE3, per the plan's key_link: it measures whether the same offsets still hold the same bytes, a property of the boundaries rather than of the naming function, so it stays independent of the sync key material entirely."
  - "CAL-2 baselines were recorded to `~/.cache/ai-usagebar-cal2/<label>.json` rather than the plan's `/tmp/cal2-a.json`. The gap between the two runs is now days-to-weeks (it waits on an account switch, not a restart), which is longer than /tmp can be relied on; that directory is also not a sync root, so the snapshots can never be collected."

patterns-established:
  - "Every calibration figure carries its date, its machine, and the exact command that produced it, so a later reader can tell a stale number from a current one."
  - "A probe whose precondition is absent prints what it needs and returns. Neither of these two can fail or hang, and a plain `cargo test` runs neither — the AUR `check()` never reads an installer's profiles."

requirements-completed: [SCOPE-03]

coverage:
  - id: D1
    description: "CAL-4 reports the real zstd-compressed and sealed size of this machine's default bundle, per category and in total, measured through the product's own collectors and Phase 1's own framing"
    requirement: SCOPE-03
    verification:
      - kind: manual
        ref: "cargo test --release --test live -- --ignored --nocapture cal4_ — 1649 files, 99.42 MiB raw, 21.51 MiB zstd, 30.07 MiB stored (3.31x)"
        status: pass
    human_judgment: false
  - id: D2
    description: "CAL-4 also measures the opt-in transcripts category, whose selection matches plan 2-04's byte-bound finding rather than a 30-day prediction"
    requirement: SCOPE-03
    verification:
      - kind: manual
        ref: "AI_USAGEBAR_CAL4_ALL=1 … cal4_ — 2077 files / 1.99 GiB selected, 999.80 MiB stored (2.04x), 2135 files / 1.66 GiB left behind by the bounds"
        status: pass
    human_judgment: false
  - id: D3
    description: "CAL-2 records a chunk-digest snapshot on each invocation and diffs against the previous one, without ever quitting or restarting Claude Desktop"
    requirement: SCOPE-03
    verification:
      - kind: manual
        ref: "Baselines recorded for all four profiles (99 files / 179 windows / 24.95 MB); an immediate second run on `toptal` reported 52/52 windows unchanged"
        status: pass
    human_judgment: false
  - id: D4
    description: "An identical tree is reported as INCONCLUSIVE, not as 0% churn, because `snapshot_profile` always renames a freshly staged copy into place — so byte-identical means no capture happened"
    requirement: SCOPE-03
    verification:
      - kind: manual
        ref: "Second run on an un-recaptured profile printed `CAL-2 = INCONCLUSIVE: … do not record 0% as the answer`"
        status: pass
    human_judgment: false
  - id: D5
    description: "Neither probe runs during a plain `cargo test`, so the AUR `check()` never reads an installer's profiles or real bundle (T-2-27)"
    requirement: SCOPE-03
    verification:
      - kind: manual
        ref: "cargo test --test live cal — 0 passed, 4 ignored (cal1..cal4)"
        status: pass
    human_judgment: false
  - id: D6
    description: "Both probes skip cleanly with a printed message when their preconditions are absent — never fail, never hang"
    requirement: SCOPE-03
    verification:
      - kind: manual
        ref: "cal2_ with both vars unset, and with a non-existent profile dir, each printed its requirement and returned ok"
        status: pass
    human_judgment: false
  - id: D7
    description: "Probe output and the committed document carry counts, digests, ratios, chip and OS only — no file body, no path outside the injected root, no hostname or account identifier (T-2-25, T-2-26)"
    requirement: SCOPE-03
    verification:
      - kind: manual
        ref: "CAL-2 prints paths stripped to the injected root; docs/sync-calibration.md names profiles only by their existing labels from 2-CONTEXT and records `Apple M3 Max, macOS 26.5.2`"
        status: pass
    human_judgment: true

duration: 45min
completed: 2026-08-19
status: complete
---

# Phase 2 / Plan 06: CAL-2 and CAL-4 Summary

**CAL-4 is measured: the default bundle is 99.42 MiB raw → 30.07 MiB stored, 3.31x, and the
per-category ratios differ enough (3.93x for `chat_index`, 2.20x for `credentials`) that a
blended figure would mislead. CAL-2's premise turned out to be wrong — the synced bytes are a
capture-time snapshot copy, not the live app's LevelDB, so an app restart cannot move them —
and the part of it that gates a decision is answered without needing any restart at all.**

## Does CAL-2 change the recommendation for the `credentials` default?

**No. `credentials` stays on, and nothing here gates Phase 4's push.**

The category cannot dominate *daily* sync cost, for a reason that is measured rather than
argued: it does not change daily. `~/.claude-acc/profiles/<label>/desktop-state/` is written
only by `claude_desktop::snapshot_profile`, which runs when an account is switched away from.
As of 2026-08-19 the four profiles were last captured on 2026-08-02, 08-04, 08-07 and 08-17 —
four captures in seventeen days. On the large majority of days the category contributes zero
new chunks whatever its churn rate is, and its worst case is bounded too: a capture rewrites
one profile, so the ceiling is 4.1–8.0 MB raw, under 4 MB stored at the measured 2.20x, not the
full 24 MB.

## Task Commits

- `67a449e` — test(2-06): CAL-2 chunk-stability and CAL-4 compressed-size probes
- `acda9cf` — test(2-06): CAL-2's trigger is a capture, not an app restart
- `e7123fc` — docs(2-06): the measured CAL-4 bundle size and CAL-2's capture-driven churn

## CAL-4 — measured 2026-08-19, Apple M3 Max / macOS 26.5.2, release

```
cargo test --release --test live -- --ignored --nocapture cal4_
```

| category | files | raw | zstd | stored | raw→stored |
|---|---:|---:|---:|---:|---:|
| `config` | 1 | 183 B | 115 B | 168 B | 1.09x |
| `credentials` | 107 | 23.81 MiB | 8.03 MiB | 10.84 MiB | 2.20x |
| `routines` | 8 | 12.43 KiB | 5.57 KiB | 9.44 KiB | 1.32x |
| `chat_index` | 1533 | 75.60 MiB | 13.47 MiB | 19.22 MiB | 3.93x |
| **TOTAL** | **1649** | **99.42 MiB** | **21.51 MiB** | **30.07 MiB** | **3.31x** |

With `AI_USAGEBAR_CAL4_ALL=1`, which forces the opt-in category on without touching the user's
config:

| category | files | raw | zstd | stored | raw→stored |
|---|---:|---:|---:|---:|---:|
| `transcripts` | 2077 | 1.99 GiB | 687.16 MiB | 999.80 MiB | 2.04x |
| **TOTAL** | **3726** | **2.09 GiB** | **708.67 MiB** | **1.01 GiB** | **2.08x** |

**The padding is the headline for 2-07.** zstd alone reaches 4.62x on the default bundle,
squarely inside the research's assumed 4–5x. The *stored* figure is 3.31x, because
`chunk::frame` rounds every sealed chunk up to a power of two so a ciphertext length cannot
leak how compressible its plaintext was (T-02-02). That costs 8.56 MiB on the default bundle
and 312 MiB on transcripts — roughly +40% and +45% on top of zstd's output. A dry-run that
quoted the zstd column would understate the push by 40%, in the user's favour, which is the one
direction it must never be wrong in. **2-07 must project from `frame(window).len() + 40`, not
from a compression ratio.**

The transcripts row is consistent with plan 2-04 and does not restate it differently: 2077
files / 1.99 GiB selected today against 2-04's 2073 / 1.989 GiB, with 2135 files / 1.66 GiB
left behind by the bounds. `excluded_*` counts everything the bounds dropped, in-window and
out, so 2077 + 2135 = the whole 3.65 GiB archive. The byte budget still binds before the day
window; user-facing text reports what was selected and never says "30 days".

## CAL-2 — baseline recorded 2026-08-19, second half deferred

### Why the plan's protocol could not have worked

The plan (and 2-CONTEXT) framed CAL-2 as "does Claude Desktop's LevelDB compaction rewrite the
24 MB profile between app restarts", to be measured across a quit-and-relaunch. Reading
`src/claude_desktop/mod.rs` before measuring showed that premise does not describe what the
`credentials` category carries. `scope::collect` walks
`~/.claude-acc/profiles/<label>/desktop-state/`, which is a *snapshot copy*:
`snapshot_profile` → `snapshot_desktop_state` stages fresh copies of `Cookies`,
`Cookies-journal`, `Local Storage`, `Session Storage` and `IndexedDB` into a tempdir and renames
it into place — only when an account is switched away from, and only with the app already quit.

So a restart moves none of the synced bytes. Two runs around one would have printed 0% churn,
and recording that as "compaction is partial, dedup carries the category" would have been a
confident wrong answer. The probe now refuses it: an identical tree prints `INCONCLUSIVE`
naming the missing capture.

### What was measured now, without interrupting anything

Baselines for all four profiles — 99 files, 179 windows, 24.95 MB — at
`~/.cache/ai-usagebar-cal2/<label>.json`, plus the capture-recency table above. That is the
half of CAL-2 that decides a default, and it is answered.

### Deferred verification — the churn rate on a capture day

Still unmeasured: on the day a capture *does* happen, what fraction of the rewritten tree is
genuinely new bytes. It only bounds how big that few-megabyte day is, so it changes no default.
After Claude Desktop has been used on an account and then **switched away from** — the switch
is the capture — run, with `<label>` one of `gmail`, `hotmail`, `struct`, `toptal`:

```bash
AI_USAGEBAR_CAL2_PROFILE=~/.claude-acc/profiles/<label>/desktop-state \
AI_USAGEBAR_CAL2_SNAPSHOT=~/.cache/ai-usagebar-cal2/<label>.json \
  cargo test --release --test live -- --ignored --nocapture cal2_
```

It prints total windows, unchanged, changed, bytes changed, and any file that appeared or
disappeared. Nothing needs to be quit or restarted for the probe's sake — it only ever reads.
`INCONCLUSIVE` means no capture happened between the two runs and the number is not usable.

Until then, D-CAL-2's named fallback stands and is recorded in `docs/sync-calibration.md`:
report the category's real churn in `sync status` from index data after a week of use, rather
than predicting it. The phase is not blocked.

## Deviations

- **CAL-2's trigger.** Measured across a capture, not an app restart, because the app restart
  does not touch the synced tree. The plan's checkpoint protocol is superseded by the one in
  `docs/sync-calibration.md`; the probe itself is exactly what the plan specified.
- **Snapshot location.** `~/.cache/ai-usagebar-cal2/<label>.json` rather than `/tmp/cal2-a.json`
  — the gap between runs is now days, and that directory is not a sync root.
- **CAL-4 reports two compressed columns, not one.** The plan asked for zstd output plus 40
  bytes of AEAD per window. That figure would have been 40% under the truth, because
  `chunk::frame` also pads. The probe runs the real `frame` and reports both, so the ratio is
  still readable and the storable size is honest.
- **`AI_USAGEBAR_CAL4_ALL` added.** One env read, so the opt-in transcripts category can be
  calibrated without editing the user's `config.toml`. Default behaviour is unchanged.

## Notes for later plans

- **2-07 (dry-run):** project stored bytes as `frame(window).len() + 40` per window. Do not
  apply a compression ratio to raw bytes — the padding makes any single ratio wrong, and wrong
  low.
- **2-07 (dry-run):** render `excluded_files` / `excluded_bytes` for transcripts only, as
  2-CONTEXT already required; the other four categories are structurally zero.
- **Phase 4 (push):** `credentials` stays on by default. `sync status` has no need to warn that
  it dominates daily cost, because it does not change daily — it changes on an account switch.
