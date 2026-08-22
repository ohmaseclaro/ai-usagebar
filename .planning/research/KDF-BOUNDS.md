# Bounded KDF parameters and available-memory preflight

**Scope:** the maintainer's blocking finding on encrypted sync — keyfile KDF
parameters are consumed before AEAD authentication, memory is capped at 4 GiB,
`t` and `p` have no application ceiling, and `check_memory_budget` has no
production call sites.

**Researched:** 2026-08-21, against `milestone/encrypted-sync` at `c33cda3`,
`argon2` 0.5.3 as vendored in this machine's cargo registry.

**Recommendation in one line:** add two ceilings (`t`, `p`), lower the third
(`m`, 4 GiB → 2 GiB), all inside the existing `check_kdf_ceiling`; and **delete**
`check_memory_budget`/`available_memory_kib` rather than wire them up.

---

## 0. What was verified, and what was not

Everything in §1–§5 marked **[V]** was read out of source on this machine or
fetched from the upstream project's own repository. Everything marked **[I]** is
an inference from a specification or a documented platform model, not an
observation. Nothing here rests on a timing I took — see §7 for why.

### Confirmed in this repo

| Claim | Evidence |
|---|---|
| `MAX_KDF_MEMORY_KIB = 4 * 1024 * 1024` | `src/sync/crypto.rs:121` **[V]** |
| No `MAX_KDF_TIME`, no `MAX_KDF_PAR` | grep of `crypto.rs`; `derive_kek` checks `m_kib` only, `crypto.rs:176` **[V]** |
| `check_memory_budget` has zero production call sites | 3 test calls (`crypto.rs:1265,1272,1274`) + 1 doc mention (`crypto.rs:119`); nothing else in `src/` **[V]** |
| `available_memory_kib` has zero production call sites | one caller, `tests/live.rs:644`, inside an `#[ignore]`d test **[V]** |
| Defaults are m=1 GiB, t=3, p=1 | `KdfParams::default`, `crypto.rs:76-83` **[V]** |
| Params are read before AEAD | `unwrap_master_key` → `derive_kek(pw, &salt, self.kdf.params())` at `crypto.rs:479`, AEAD `decrypt` at `:481` **[V]** |

### Confirmed in `argon2` 0.5.3

| Claim | Evidence |
|---|---|
| `MAX_M_COST = u32::MAX`, `MAX_T_COST = u32::MAX` | `params.rs:49,58` **[V]** |
| `MAX_P_COST = 0xFFFFFF`, and `Params::new` requires `m_cost >= p_cost * 8` | `params.rs:67,119-121,133` **[V]** |
| The block buffer is allocated with an infallible `vec![]` | `lib.rs:230` — `let mut blocks = vec![Block::default(); self.params.block_count()];` **[V]** |
| A block is 1 KiB, so `block_count() ≈ m_kib` | `block.rs:55` — `pub const SIZE: usize = 1024` **[V]** |
| **The crate has no `parallel`/rayon feature.** Complete feature list: `alloc`, `default`, `rand`, `simple`, `std` | vendored `Cargo.toml` `[features]` **[V]** |
| `hash_password_into_with_memory` accepts a caller-owned buffer, and is *not* feature-gated | `lib.rs:242-251` **[V]** |
| `argon2::Block` is exported unconditionally (works with `default-features = false, features = ["alloc","zeroize"]`) | `lib.rs:101-107` **[V]** |

### Three defects found in passing that the planner should fold in

1. **`--kdf-memory` does not exist.** Six places assume it does —
   `crypto.rs:397`, `crypto.rs:776`, the live refusal text at `crypto.rs:788`
   (`"re-run with a lower --kdf-memory"`), the test asserting that text at
   `crypto.rs:1268`, `passphrase.rs:18`, `passphrase.rs:166`, plus
   `docs/sync-format.md:82,582,591`. There is no such CLI argument anywhere in
   `src/`. **[V]** `docs/sync-format.md:591` states outright that a constrained
   machine "gets an actionable refusal naming `--kdf-memory` rather than an OOM
   kill" — the refusal is unreachable and the flag is imaginary. This is the
   same class of defect as the finding under review.
2. **`available_memory_kib` on macOS reads `sysctl -n hw.memsize`** — total
   installed physical memory, a constant (`crypto.rs:812-818`). **[V]** On this
   host it returns 37,748,736 KiB whether 30 GiB is free or 200 MiB is.
   `check_memory_budget(1 GiB, that)` therefore passes unconditionally. The
   macOS half of the preflight cannot fail.
3. **There is no Windows `available_memory_kib`.** `#[cfg(not(any(linux,
   macos)))]` returns `None` (`crypto.rs:823`). **[V]** Wiring the preflight up
   as written ships a Unix-only defence — precisely the shape of the
   maintainer's *first* blocking finding.

### The compatibility set is exactly one point

This decides §1. `SetupPrompt::kdf()` defaults to `KdfParams::default()`
(`src/sync/github/setup.rs:124-126`) and the **only** override in the tree is
the test double at `:1006-1012`. There is no CLI flag. `Keyfile::create` is
reached from production at exactly one site, `setup.rs:698`. **[V]**

Therefore every keyfile any released `ai-usagebar` has ever written carries
exactly `{ m_kib: 1_048_576, t: 3, p: 1 }`. Sync shipped in v1.4.0 (2026-08-20,
tagged), so field bundles exist — but all of them at that one point. A ceiling
at or above `(1 GiB, 3, 1)` strands nobody. The task's constraint that "the
ceiling must admit every bundle a correct writer could have produced" is
satisfied by any ceiling ≥ the default.

---

## 1. Recommended ceilings

### `MAX_KDF_MEMORY_KIB = 2_097_152` (2 GiB) — lowered from 4 GiB

1. **RFC 9106 §4's first recommended option is `m = 2^21` KiB (2 GiB)**, t=1,
   p=4 — the largest memory value any published recommendation asks for. (The
   second option is m=2^16 = 64 MiB, t=3, p=4.) **[V]** A conforming writer
   never needs more, so this ceiling costs interoperability nothing.
2. It is **2× the only value this build writes**, leaving room for a future
   `--kdf-memory` to double the default without a format change.
3. **4 GiB was never a bound, only a bigger number.** Its doc comment
   (`crypto.rs:114-120`) argues 4 GiB "costs a legitimate bundle nothing" — true
   — but omits that 4 GiB is a guaranteed OOM on the 4 GB class of machine this
   project ships aarch64 binaries for. `docs/sync-format.md` names that class
   explicitly. A ceiling that still kills the smallest supported target is not
   doing the job the finding asks of it.
4. **Against the alternative:** Bitwarden's accepted maximum is 1024 MiB (§4),
   which here would exactly equal the shipped default and leave zero headroom.
   2 GiB is the smallest defensible value above the default that a standard
   actually endorses.

### `MAX_KDF_TIME = 16` — new

1. **`t` is a pure CPU multiplier with no compensating allocation limit.**
   Argon2's work is exactly linear in `t` by construction: the memory is filled
   `t` times (RFC 9106 §3.2). **[V]** Today `t` is bounded only by
   `MAX_T_COST = u32::MAX`, i.e. ~1.43 × 10⁹ times the shipped cost. This is the
   sharper half of the finding — sharper than memory, because it needs no
   allocation at all.
2. **16 is strictly above every published recommendation**, so it cannot refuse
   a parameter set any standards-following writer emits:

   | Source | `t` |
   |---|---|
   | RFC 9106 §4, first option | 1 |
   | RFC 9106 §4, second option | 3 |
   | OWASP Password Storage Cheat Sheet (five configs) | 1, 2, 3, 4, 5 |
   | borg `ARGON2_ARGS` | 3 |
   | Bitwarden accepted range | 2 – 10 |
   | **this build** | **3** |

   16 is the next power of two above all of them. **[V]**
3. **Paired with the memory ceiling it bounds total forced work.** Argon2 work
   is proportional to `m × t`. Worst case becomes 2048 MiB × 16 = **32,768
   MiB-passes**, against the shipped default's 1024 × 3 = 3,072 — a bounded
   **10.7×**. Today the same product is unbounded. Wall-clock projection is
   deliberately *not* stated here; see §7.

### `MAX_KDF_PAR = 16` — new, and it is **not** a cost bound

Be honest about this one, because a bound with the wrong rationale is the same
defect the maintainer is objecting to.

1. **`p` is not a DoS amplifier in this build.** `argon2` 0.5.3 has no
   `parallel`/rayon feature **[V]**, so lanes are filled sequentially. Total
   block operations are `m × t` regardless of `p`; memory is `m` regardless of
   `p`. An attacker gains nothing from raising it.
2. **`p` is already structurally capped.** `Params::new` rejects
   `p > 0xFFFFFF` and requires `m_kib >= 8p` **[V]**, so with `m` capped at
   2 GiB, `p` cannot exceed 262,144 even with no explicit bound.
3. **The bound is a tamper signal, not a cost bound.** This build writes `p = 1`
   only. No recommendation exceeds 4 (RFC 9106, borg) or 16 (Bitwarden's
   maximum). 16 costs one integer comparison and closes the "no comparable
   application ceiling" half of the finding without pretending to buy safety it
   does not buy.
4. **Why not require `p == 1`?** Because the format stores per-bundle parameters
   precisely so a future writer can change them, and `p == 1` would refuse
   RFC 9106's own two recommended options (both p=4).

### Explicitly considered and rejected: a work-product ceiling

A `m_mib × t ≤ W` ceiling is more precise — it is exactly the quantity an
attacker maximises — but the two flat ceilings already bound that product at
10.7× the shipped cost. A third constant buys precision nobody needs and a third
thing to explain. Recorded here so a reviewer can see it was weighed, not
missed.

### Where the code goes

All three checks belong in the existing `check_kdf_ceiling`, which `derive_kek`
already calls before `Params::new` (`crypto.rs:176`). That function's own doc
comment already makes the argument for why the guard lives in the shared
function rather than at call sites, and that argument is correct — nothing about
the structure needs to change. **No new call sites, no new plumbing, no
platform code, no new dependency, no `unsafe`.**

`MIN_KDF_MEMORY_KIB` stays write-only. The existing asymmetry (floor on write,
ceiling on both) is right and its rationale is already documented at
`crypto.rs:123-128`.

---

## 2. Available physical memory, per platform, without a crate

Answered as asked, then argued against in §3.

### Linux — 6 lines, `std` only, and the one platform where it earns its keep

`std::fs::read_to_string("/proc/meminfo")`, take `MemAvailable:`. Already
written and correct at `crypto.rs:798-810`. **[V]**

This is the only platform where a preflight has genuine value, because Linux
overcommits: the allocation *succeeds* and the OOM killer arrives later during
`fill_blocks`. A SIGKILL is not something any Rust code can turn into the
exit-0 fallback the project's hard invariant requires. **[I]** — the overcommit
model is documented, but the specific outcome under this ceiling was not
observed (§7).

**Cost: zero.** No crate, no subprocess, no `unsafe`.

### Windows — four options, and the recommendation is "none of them"

| Option | Cost | Verdict |
|---|---|---|
| Hand-declared `unsafe extern "system" { fn GlobalMemoryStatusEx(*mut MEMORYSTATUSEX) -> i32 }` against `kernel32`, plus a 9-field `#[repr(C)]` struct (`dwLength`, `dwMemoryLoad`, then 7 `u64`s); read `ullAvailPhys` | ~25 lines, one `unsafe`, instant, exact | Works, but the repo has **no** hand-rolled FFI today and `src/nous/credentials.rs:154` records an explicit preference against introducing one **[V]** |
| `C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe -NoProfile -Command "(Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory"` | zero `unsafe`; matches the repo's established absolute-System32-path idiom (`src/sync/restore/backup.rs:58` does exactly this for `tar.exe`) **[V]**; but a PowerShell cold start costs on the order of hundreds of ms **[I], unmeasured** | Viable fallback if a preflight is mandated |
| `wmic OS get FreePhysicalMemory` | — | **Do not.** Deprecated and absent by default on Windows 11 24H2 and Server 2025 |
| A crate | — | See below — treat as a finding |

**If a crate is genuinely unavoidable**, it is *not* `sysinfo` (large, many
transitive deps, carries refresh machinery this needs none of). The smallest
correct one is:

```toml
[target.'cfg(windows)'.dependencies]
windows-sys = { version = "0.59", features = ["Win32_System_SystemInformation"] }
```

Transitive tree: `windows-sys` → `windows-targets` → the per-arch
`windows_x86_64_msvc` / `windows_aarch64_msvc` / `_gnu` import-library crates,
which contain binary link stubs and no Rust code. Target-gated it never touches
the Linux or macOS build — the same shape the repo already uses for
`security-framework` (`Cargo.toml`, `[target.'cfg(target_os = "macos")'
.dependencies]`) **[V]**.

**But treat that as a finding, not a default:** it is a new dependency, on the
one platform where §3 argues the check is redundant anyway.

### macOS — no honest cheap answer, and none needed

- `hw.memsize` is **total**, not available — the current implementation's bug.
- `sysctl vm.page_free_count` excludes the compressor and purgeable pages, so it
  understates by design, usually by an order of magnitude.
- `vm_stat` requires summing free + inactive + speculative and multiplying by
  page size, still ignores compressible memory, and costs a subprocess.
- `host_statistics64` is FFI.

And macOS **swaps rather than kills** a CLI process **[I]**, so the failure mode
is slow, not fatal. Every cheap number available is wrong in the conservative
direction, which would produce false refusals on machines that would have
succeeded.

**Recommendation: no macOS preflight**, and delete the `hw.memsize` version
rather than leave a check that structurally cannot fail.

---

## 3. Preflight versus ceiling — is the second needed?

**Recommendation: no. Delete `check_memory_budget` and `available_memory_kib`.**

Reasons, strongest first:

1. **The macOS implementation cannot answer the question it documents.**
   §0 defect 2. It is a no-op wearing a check's clothing.
2. **There is no Windows implementation.** §0 defect 3. Shipping it repeats the
   Unix-only shape of the first blocking finding.
3. **It names a flag that does not exist.** §0 defect 1. Wiring it up ships an
   error message whose remedy the user cannot perform.
4. **It is inherently racy, and not academically so.** Memory measured at T is
   not memory available at T+1. This host, while researching, sat at load
   average 59.6 with a dozen competing processes; "available" swung by gigabytes
   between consecutive reads. **[V]**
5. **It is redundant against a ceiling low enough to matter, and useless against
   one that isn't.** No preflight lets a 1 GB Pi open a 1 GiB bundle. What the
   user needs is a refusal naming the actual constraint, and the ceiling gives
   that without reading the machine at all.
6. **Zero production call sites**, so deleting is a strictly smaller diff than
   wiring up — and removes two `#[cfg]` branches, a subprocess spawn, and three
   tests.

### What deleting loses, stated plainly

On Linux, and only on Linux, the preflight would predict an OOM kill that the
ceiling cannot prevent. If the maintainer wants that back, take the Linux arm
only — it is six lines of `std` and the one platform where overcommit makes the
allocation succeed and the kill arrive later. Do **not** take macOS (cannot
fail) or Windows (needs FFI or a crate for a case §4 says is already covered).

An asymmetric per-platform preflight is defensible here **because the platforms
are genuinely asymmetric**, not because Unix got attention first — and that
distinction is what should go in the code comment.

---

## 4. `Vec::try_reserve` — measured, and it is not what one hopes

Measured on this host (Darwin 25.5.0, 36 GiB physical), via
`argon2::Params::block_count()` and `Vec::<argon2::Block>::try_reserve_exact`:

```
try_reserve_exact(4194304 blocks = 4096 MiB) => Ok
try_reserve_exact(4294967292 blocks = 4194303 MiB) => Ok
```

**4 TiB reserved successfully on a 36 GiB machine. [V]** macOS and Linux hand
out address space, not memory. `try_reserve` catches nothing there.

**Therefore: `try_reserve` is not a substitute for the ceiling and must not be
documented as one.** This is worth stating explicitly because it is the obvious
fix and it is wrong on two of three platforms.

What it does buy, for about eight lines:

- On **Windows** and on **32-bit targets**, where commit charge / address space
  is charged up front, it converts `handle_alloc_error`'s abort into a
  `Result` — the abort being the specific thing that violates the project's
  hard invariant. **[I]** — follows from the Windows commit-charge model, not
  observed; no Windows host (§7).
- It is mechanically available: `Argon2::hash_password_into_with_memory` takes a
  caller-owned `impl AsMut<[Block]>` and `argon2::Block` is exported
  unconditionally. **Verified working on a `try_reserve_exact`'d buffer on this
  machine. [V]**

```rust
let n = argon.params().block_count();
let mut blocks: Vec<Block> = Vec::new();
blocks.try_reserve_exact(n).map_err(|_| /* actionable refusal */)?;
blocks.resize(n, Block::new());
argon.hash_password_into_with_memory(pw, salt, out.as_mut(), &mut blocks[..])
```

Note: argon2's own `vec![Block::default(); n]` does **not** use zeroed
allocation — `Block` is a plain `[u64; 128]` newtype and does not implement
std's internal `IsZero` marker, so the generic clone path allocates uninitialised
and writes every page immediately. **[I]** The `resize` above behaves
identically; the only difference is that the *reservation* is fallible.

**This is optional.** The smallest diff that closes the review is the three
ceiling constants and nothing else. `try_reserve` is the Windows half of
"Windows matters as much as Unix" done without a dependency and without
`unsafe` — which is why it is worth offering, but it should be a decision, not
an assumption.

---

## 5. Prior art

Every row read from the project's own source unless noted.

| Project | Params in the file? | Ceiling on read? | Value | Refusal |
|---|---|---|---|---|
| **age** (Go, `scrypt.go`) | yes — `log_n` in the scrypt stanza | **yes** | `maxWorkFactor: 22` ("15s on a modern machine"); write default `workFactor: 18` ("1s"); setters clamp to 1..30 | `scrypt work factor too large: %v` |
| **rage** (Rust, `age/src/native/scrypt.rs`) | yes | **yes, calibrated to the device** | `let max_work_factor = target_work_factor + 4;` — comment: *"Place bounds on the work factor we will accept (roughly 16 seconds)."* `target_scrypt_work_factor()` benchmarks scrypt from `log_n = 10` upward until it exceeds 1 s; falls back to 18 when unmeasurable | `DecryptError::ExcessiveWork { required, target }` |
| **Bitwarden server** (`src/Core/KeyManagement/Kdf/KdfConstants.cs`) | server-supplied at prelogin — same shape, remote-controlled | **yes** | `ARGON2_ITERATIONS = new(2, 10, 6)`; `ARGON2_MEMORY = new(15, 1024, 32)` (MiB); `ARGON2_PARALLELISM = new(1, 16, 4)`; `PBKDF2_ITERATIONS = new(600_000, 2_000_000, 600_000)` — `(min, max, default)` | `Argon2 memory must be between 15mb and 1024mb.` |
| **borg** (`src/borg/crypto/key.py`, `constants.py`) | yes — `argon2_time_cost`, `argon2_memory_cost`, `argon2_parallelism`, `argon2_type` | **no** — `decrypt_key_file_argon2` passes all four straight into `argon2.low_level.hash_secret_raw` | writes `ARGON2_ARGS = {"time_cost": 3, "memory_cost": 2**16, "parallelism": 4, "type": "id"}` (64 MiB) | none |
| **restic** (`internal/repository/crypto/kdf.go`, `internal/repository/key.go`) | yes — `N`, `R`, `P` in the key file | **no application ceiling** — only `sscrypt.Params.Check()`, i.e. scrypt *algorithmic* validity (N a power of two, `r*p < 2^30`), not a cost bound | `Calibrate(timeout, memory)` at write time | none |
| **KeePassXC** (`src/crypto/kdf/Argon2Kdf.cpp`) | yes — KDBX4 header | **spec range only** | `setMemory`: `kibibytes >= 8 && kibibytes < (1ULL << 32)` — i.e. 8 KiB .. ~4 TiB. `setParallelism`: `1 .. (1 << 24) - 1`. Out of range silently reverts to default and `processParameters` returns false | database refuses to open |
| **1Password** | n/a — PBKDF2-HMAC-SHA256 over Secret Key + master password; iteration count is account metadata, not a per-file header field | n/a | — | — **not independently verified** (closed source); the weakest row, included only to record that its threat model differs |

### Reading of the table

- **The two projects that decided — age and rage — both bound decrypt work as a
  multiple of the write default expressed in *time*** (16×, "roughly 15–16
  seconds"), and both name the offending value in the error. That is direct
  support for §1's shape and for §5's message design.
- **rage goes further and calibrates the bound to the machine**, so its ~16 s
  budget holds on fast and slow hardware alike. That is the strongest available
  answer to "a fixed ceiling means 16 s here and 64 s on a Pi" — and it costs a
  ~1 s benchmark on *every* open, which against a 1.5 s derivation is a ~66%
  tax for a better error message. **Recommend against it here**, but the
  tradeoff should be the maintainer's, and rage's own doc comment on
  `set_max_work_factor` is the best-written statement of the risk in the whole
  table.
- **The two backup tools with the closest threat model to this one — borg and
  restic — do not bound at all.** That is corroboration that the maintainer's
  finding is a real and commonly-missed defect rather than a stylistic
  preference, and it is worth citing in the PR.
- **Bitwarden is the only one publishing an explicit numeric range for Argon2id
  specifically**, and its `(min, max, default)` triple per parameter is the
  shape to copy. Note its max/default ratios: memory 32×, iterations 1.67×,
  parallelism 4×.

**Sources:** [age `scrypt.go`](https://github.com/FiloSottile/age/blob/main/scrypt.go) ·
[rage `age/src/native/scrypt.rs`](https://github.com/str4d/rage/blob/main/age/src/native/scrypt.rs) ·
[Bitwarden `KdfConstants.cs`](https://github.com/bitwarden/server/blob/main/src/Core/KeyManagement/Kdf/KdfConstants.cs) ·
[borg `key.py`](https://github.com/borgbackup/borg/blob/master/src/borg/crypto/key.py) ·
[borg `constants.py`](https://github.com/borgbackup/borg/blob/master/src/borg/constants.py) ·
[restic `kdf.go`](https://github.com/restic/restic/blob/master/internal/repository/crypto/kdf.go) ·
[KeePassXC `Argon2Kdf.cpp`](https://github.com/keepassxreboot/keepassxc/blob/develop/src/crypto/kdf/Argon2Kdf.cpp) ·
[RFC 9106](https://www.rfc-editor.org/rfc/rfc9106.html) ·
[OWASP Password Storage Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html)

---

## 6. The correct refusal

### Is the leak acceptable? Yes.

1. **The bundle's existence is not secret from this attacker.** The keyfile
   lives in a repository the attacker — by the threat model that produced this
   finding — can already read and write. The refusal tells them nothing they did
   not themselves supply.
2. **It is not a password oracle.** The check is on `KdfParams` alone and runs
   before `pw` is touched (`derive_kek`, `crypto.rs:176`). Its outcome cannot
   vary with the password. **[V]**
3. **It must not disturb the existing collapse.** `crypto.rs:487-490`
   deliberately gives a wrong password and a downgraded `m_kib` the same message,
   because there is nothing useful to distinguish. That collapse is right and
   must stay. The ceiling refusal is a different class — it concerns a public,
   cleartext field, and distinguishing it is correct.
4. The existing `check_kdf_ceiling` doc comment already makes exactly this
   argument ("The parameters are public — they live in cleartext in the keyfile
   — so naming them is safe"). It needs extending to `t` and `p`, not rewriting.

### The message

Project rule: an error names its fix and never leaks a secret. The current text
("refusing before the allocation rather than aborting inside it") explains the
*implementation* rather than the remedy, and its sibling in `check_memory_budget`
names a flag that does not exist.

Proposed:

```
this keyfile asks for {m} MiB of key-derivation memory, t={t} passes and
p={p} lanes; this build accepts at most 2048 MiB, t=16 and p=16. A keyfile
written by ai-usagebar carries m=1024, t=3, p=1 — if this one does not, the
remote copy has been altered. Fetch the keyfile again from a machine you
trust, or re-run `sync setup` against a repository you control.
```

Why this shape:

- Names the offending value **and** the bound — both public, both attacker-supplied.
- States what a genuine keyfile looks like, so the user can distinguish tampering
  from a version skew. This is the actionable part, and it is only sayable
  because §0 established the compatibility set is a single point.
- Gives two concrete actions, neither of which is "lower a flag".
- Leaks nothing: every value it prints came from the attacker.

**Hard constraint for the planner:** do not ship any message saying "re-run with
a lower `--kdf-memory`" until that flag exists.

---

## 7. What remains unmeasured

1. **Wall-clock at the proposed ceiling — not obtained, and my attempts are
   discarded.** This host is an M3 Max / 36 GiB / 14 cores, but ran at **load
   average 59.6** with a `cargo test` and several Node processes competing.
   Identical repeated probes drifted **−56%**, and m=512 MiB measured *twice* the
   time of m=1024 MiB, which is impossible. Every timing my probe produced is
   unusable and none of §1's rationale rests on one. **Calibration needed:**
   `cargo test --release --test live -- --ignored --nocapture
   cal3_argon2id_timing_at_production_parameters` on an idle machine, extended
   to the ceiling point (2048 MiB, t=16), on (a) an idle M-series Mac and (b) a
   slow aarch64 Linux box — the same aarch64 measurement `docs/sync-format.md`
   already records as never obtained after four phases.
2. **The repo's own 1492–1548 ms figure is single-sourced** to one M3 Max run.
   It is the best number available, and §1's rationale deliberately leans on the
   *ratio* (10.7× the default work) rather than on any absolute, precisely
   because of this.
3. **Windows `try_reserve` behaviour is inferred, not observed.** §4's claim
   that Windows commit charge makes `try_reserve_exact` fail for an over-large
   Argon2 buffer follows from the documented memory model but was not run — no
   Windows host. **Calibration:** on Windows, `try_reserve_exact` a buffer larger
   than RAM + pagefile and assert `Err`. If this fails, §4's entire value
   proposition evaporates and the recommendation becomes "ceilings only".
4. **Linux OOM-killer behaviour under the proposed ceiling** was not observed.
   **Calibration:** in a 1 GB-memory container, open a bundle at m=2 GiB and
   record whether the process takes SIGKILL (the Linux preflight has value) or
   an allocation error (it does not).
5. **PowerShell cold-start cost on Windows** — §2 asserts "hundreds of
   milliseconds". Not measured.
6. **`p` lane cost.** An interleaved best-of-3 probe gave p=4 at **1.09×** p=1,
   corroborating the existing "~10% worse" doc comment at `crypto.rs:72-75`. An
   earlier *sequential* run gave 2.37×, which I attribute to contention and am
   deliberately **not** reporting as a finding. Under load average 59 even the
   1.09× is weak. Worth one clean re-measurement before anyone cites either.
7. **macOS behaviour when the allocation genuinely cannot be served** was not
   tested (would require exhausting a 36 GiB machine). §2's "swaps rather than
   kills" is inference.

---

## 8. Suggested phase structure

| Phase | Content | Size | Blocks the PR? |
|---|---|---|---|
| **A — the fix** | `MAX_KDF_TIME = 16`, `MAX_KDF_PAR = 16`, `MAX_KDF_MEMORY_KIB` 4 GiB → 2 GiB, all inside `check_kdf_ceiling`; new refusal text (§6); tests at each boundary ±1 and one asserting the shipped default still passes | ~40 lines, no deps, no platform code, no `unsafe` | **yes — A alone closes the review** |
| **B — cleanup** | Delete `check_memory_budget` + `available_memory_kib` + their three tests; correct the six `--kdf-memory` references and `docs/sync-format.md:591`, **or** implement the flag | net-negative diff | no, but it removes a false claim and should not wait |
| **C — Windows parity (optional)** | `try_reserve_exact` + `hash_password_into_with_memory` (§4) | ~8 lines | no — a decision for the maintainer, not an assumption |
| **D — calibration debt** | The seven items in §7, especially (1) and (3) | — | no |

**Ordering rationale.** A is self-contained and is the whole of the maintainer's
finding; nothing else gates it. B is a negative diff that removes documentation
asserting more than the code does — the pattern the reviewer is already watching
for — so it should ship with A rather than after. C is genuinely optional and its
value depends on D(3). D can trail.

**Phases likely to need deeper research:** none. A is three constants and a
message. C's only open question is D(3), which is a single Windows test, not a
research task.
