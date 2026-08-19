# Phase 1 — Deferred live verification

Everything in Phase 1 that a machine could verify **has been verified**: 1095 tests pass,
clippy and fmt are clean, and the verifier independently re-ran the gates and enumerated
consumers rather than trusting the claims. The two items below need something this phase
deliberately does not have — a real GitHub repo with a token, and slow non-Apple hardware —
so they were built, left `#[ignore]`d, and deferred rather than allowed to block the run.

**Neither is a blocker.** Both have a shipped fallback already in the code.

---

## CAL-1 — Do private-repo Release assets honour `Range:`?

**Why it matters:** it decides pack sizing. If a client can fetch byte ranges from a release
asset, packs can be large and a restore pulls only the chunks it needs. If not, a restore must
download whole packs, and they should stay small.

**Current fallback, already shipped:** `PACK_TARGET` is 32 MiB, chosen to make a full-asset
fetch acceptable. If `Range:` works, Phase 4 may raise it — **and if it does, re-check the pack
header's single-chunk ceiling** (see `.planning/phases/04-…/4-CONTEXT.md`).

**To run it:**

```bash
# A private repo you own, with one release asset of at least a few MiB.
export GSD_CAL1_TOKEN='<fine-grained PAT, Contents: read, that repo only>'
export GSD_CAL1_REPO='<owner>/<repo>'
export GSD_CAL1_ASSET='<asset id or name>'
cargo test --test live -- --ignored cal1_range_on_private_release_asset --nocapture
```

The probe prints the HTTP status, any `Content-Range`, and the bytes returned. It deliberately
**does not follow redirects**, so your token is never replayed to signed storage, and it prints
only the storage *host* — a signed URL's query string is itself a credential. It skips cleanly
with a message if the token is absent.

**What to look for:** `206 Partial Content` with a `Content-Range` header = ranges work.
`200 OK` with the whole asset = they don't.

---

## CAL-3 — Argon2id timing on slow aarch64 Linux

**Measured on this machine** (Apple M3 Max, 36 GiB, release build), three runs at the shipped
parameters:

| Memory | Run 1 | Run 2 | Run 3 |
|---|---|---|---|
| **1024 MiB (shipped)** | 1503 ms | 1492 ms | 1548 ms |
| 512 MiB | 701 ms | 816 ms | 779 ms |
| 256 MiB | 336 ms | 376 ms | 380 ms |

This confirms the research estimate (1582 ms). **No aarch64 Linux number was obtained.**

Docker was available and deliberately not used: a Linux VM on this same M3 Max silicon answers
the *operating system* question, not the *slow hardware* question CAL-3 exists to ask, and the
number would invite being read as a clearance for constrained targets. That refusal is the
correct call, not an omission.

**Current fallback, already shipped:** KDF parameters are stored per-bundle and configurable, so
a user on constrained hardware can lower them knowingly rather than being locked out. The
at-or-below version ceiling means a bundle written with lower parameters still opens everywhere.

**To run it, on a genuinely slow aarch64 Linux box** (a Raspberry Pi 4/5, a small ARM VPS —
*not* a VM on Apple silicon):

```bash
cargo test --release --test live -- --ignored cal3_argon2id_timing_at_production_parameters --nocapture
```

**What to look for:** if the shipped 1 GiB / t=3 configuration exceeds roughly 5 seconds, the
default is too aggressive for that class of hardware and the docs should say so — the parameters
stay configurable either way.

---

*Both probes live in `tests/live.rs` and are excluded from the default test set, so the AUR
`check()` never touches the network on an installer's machine.*
