---
phase: 01-encrypted-bundle-core
plan: 10
subsystem: crypto
tags: [security-remediation, aead, nonce-reuse, associated-data, rollback-anchor, kdf-bounds, passphrase-policy]

# Dependency graph
requires: [1-02, 1-04, 1-05, 1-06, 1-07, 1-09]
provides:
  - "Keys::seal derives the chunk nonce from the bytes it encrypts and stores it inline — nonce ↔ message injectivity, not nonce ↔ pre-image"
  - "Keys::seal is pub(crate), with the (id, message) binding stated as a safety contract"
  - "Keys::{seal_root,open_root} and Root::{seal,open} take the bundle's repo_id; it is bound as AEAD associated data"
  - "crypto::MAX_KDF_MEMORY_KIB (4 GiB), checked inside unwrap_master_key before derive_kek"
  - "crypto::MIN_KDF_MEMORY_KIB (8 MiB), checked inside Keyfile::wrap, so it covers create and rewrap"
  - "Keyfile::{create_with_floor,rewrap_with_floor} — the cheap-KDF seam for the new floor"
  - "passphrase::check(pw, k) — below KdfParams::default's memory it accepts nothing short of generated strength"
  - "The anchor's keying precondition, written in anchor.rs and docs/sync-format.md §9"
affects: [phase-2, phase-3, phase-4, phase-5]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "A derived nonce is derived from the message, never from an address that merely names it — the address is authenticated as AAD instead"
    - "An AAD literal that is constant across every bundle in the world is scoped with the bundle's own identifier, supplied by the caller from local configuration"
    - "Attacker-supplied allocation parameters are bounded inside the function that would allocate for them, not by a caller who might forget"
    - "A policy floor whose enforcement would break the hermetic-test seam takes the floor value as an argument, exactly as Cache::at takes a path"

key-files:
  created: []
  modified:
    - src/sync/crypto.rs
    - src/sync/chunk.rs
    - src/sync/model.rs
    - src/sync/anchor.rs
    - src/sync/passphrase.rs
    - src/sync/pack.rs
    - tests/sync_vectors.rs
    - tests/sync_adversarial.rs
    - docs/sync-format.md

key-decisions:
  - "F-1 fixed by the audit's preferred remediation: nonce = derive_key(CTX_NONCE, keyed_hash(name_key, framed))[..24], stored inline. The id is untouched, so dedup keys on the same address and every id pin holds"
  - "The three ciphertext pins were re-pinned; the two id pins were verified not to move first, which is the distinction 1-07's pin labels exist to make legible"
  - "Root::open additionally rechecks the repo_id inside the authenticated plaintext against the caller's expectation — the same belt and braces open_chunk applies to a chunk id"
  - "F-4's floor is enforced in Keyfile::wrap as the audit asked, but with the floor value as an argument. A hard 8 MiB floor with no seam would put every sync test through a real Argon2id derivation, which the hermetic-test invariant exists to prevent"
  - "Reads are bounded above (4 GiB) and deliberately unbounded below: a bundle written before a floor existed must stay openable, or raising a floor destroys data"
  - "The Root.kdf flag was resolved by deleting the sentence rather than implementing the comparison — src/sync/ is pure and has no warning channel, and an error would contradict 'the keyfile wins'"

patterns-established:
  - "Deterministic sealing and nonce safety are two properties, not one; a comment claiming the first must not be read as claiming the second"
  - "Where a precondition rests on a caller a later phase will write, it is stated in the module doc and in the format document, not assumed"

requirements-completed: []

coverage:
  - id: D1
    description: "Two different framings of one plaintext are sealed under different nonces — the regression guard for the whole of F-1, simulating a zstd divergence directly rather than installing two zstd builds"
    requirement: CRYPTO-01
    verification:
      - kind: unit
        ref: "cargo test --lib sync::crypto — two_framings_of_one_plaintext_are_sealed_under_different_nonces"
        status: pass
    human_judgment: false
  - id: D2
    description: "The chunk id pins did not move: the id is still keyed_hash(name_key, plaintext), so dedup keys on an unchanged address and no existing bundle is re-addressed"
    requirement: CRYPTO-01
    verification:
      - kind: unit
        ref: "cargo test --test sync_vectors — the_chunk_id_of_a_fixed_plaintext_is_pinned_and_must_survive_a_zstd_upgrade, the_multi_chunk_bundles_manifest_id_is_pinned_and_must_survive_a_zstd_upgrade"
        status: pass
    human_judgment: false
  - id: D3
    description: "The sealed bytes are still reproducible from the documented format, now including the inline nonce; and the nonce is pinned as a function of the framed bytes rather than only as a prefix of a literal"
    requirement: CRYPTO-01
    verification:
      - kind: unit
        ref: "cargo test --test sync_vectors — the_chunk_subkey_is_pinned_and_is_the_key_the_seal_path_uses, the_sealed_chunk_ciphertext_is_pinned_and_is_zstd_sensitive"
        status: pass
    human_judgment: false
  - id: D4
    description: "Sealing is still deterministic within a build, so dedup still works and a re-sync of unchanged data creates no new remote objects"
    requirement: CRYPTO-01
    verification:
      - kind: unit
        ref: "cargo test --lib sync::chunk — two_seals_of_one_input_are_byte_identical; cargo test --test sync_adversarial — sealing_one_fixture_twice_produces_identical_packs_and_identical_chunk_ids"
        status: pass
    human_judgment: false
  - id: D5
    description: "A chunk shorter than a nonce and a tag errors rather than panicking — the inline nonce added a new attacker-controlled length to slice on"
    requirement: CRYPTO-05
    verification:
      - kind: unit
        ref: "cargo test --lib sync::crypto — a_chunk_shorter_than_a_nonce_and_a_tag_errors_instead_of_panicking"
        status: pass
    human_judgment: false
  - id: D6
    description: "A snapshot root belonging to another bundle fails the Poly1305 tag under the same master key, at both the crypto and the model layer"
    requirement: CRYPTO-05
    verification:
      - kind: unit
        ref: "cargo test --lib sync — a_root_from_another_bundle_fails_the_tag_under_the_same_master_key, a_root_from_another_bundle_is_refused_even_under_the_right_key; cargo test --test sync_vectors — the_root_subkey_is_pinned_and_is_the_key_the_snapshot_root_uses"
        status: pass
    human_judgment: false
  - id: D7
    description: "A keyfile demanding more Argon2id memory than the ceiling is refused before the allocation, so an edited integer is an error rather than a handle_alloc_error abort"
    requirement: CRYPTO-02
    verification:
      - kind: unit
        ref: "cargo test --lib sync::crypto — a_keyfile_demanding_more_memory_than_the_ceiling_is_refused_before_allocating"
        status: pass
    human_judgment: false
  - id: D8
    description: "A new keyfile below the memory floor is refused through both create and rewrap, while an existing bundle below it still opens"
    requirement: CRYPTO-02
    verification:
      - kind: unit
        ref: "cargo test --lib sync::crypto — a_new_keyfile_below_the_memory_floor_is_refused_through_both_entry_points"
        status: pass
    human_judgment: false
  - id: D9
    description: "Below the shipped KDF memory cost, passphrase::check refuses anything short of generated strength, and generated strength still clears it at any cost"
    requirement: CRYPTO-06
    verification:
      - kind: unit
        ref: "cargo test --lib sync::passphrase — a_lowered_kdf_cost_accepts_nothing_short_of_a_generated_passphrase"
        status: pass
    human_judgment: false
  - id: D10
    description: "The nine adversarial refusals still fire, still carry nine distinct messages, and the honest bundle still round-trips — the format change did not make the pipeline refuse everything"
    requirement: CRYPTO-03
    verification:
      - kind: unit
        ref: "cargo test --test sync_adversarial — all 13"
        status: pass
    human_judgment: false

# Metrics
duration: 70min
completed: 2026-08-19
status: complete
---

# Phase 1 Plan 10: Security Audit Remediation Summary

**Two blockers, two mediums and one documentation flag from `1-SECURITY.md`. The blocking one
changes the on-disk format: the chunk nonce was derived from a *pre-image* of the message it
protected, so two zstd builds could seal two distinct messages under one `(chunk_key, nonce, aad)`.
The nonce is now derived from the bytes actually encrypted and travels inline. Every **id** pin
holds; the three **ciphertext** pins moved and were re-pinned.**

## Performance

- **Duration:** ~70 min
- **Findings closed:** 5/5 (2 blocker, 2 medium, 1 flag)
- **Files modified:** 9 (six modules, two test binaries, the format document)
- **Commits:** `dad498d` (F-1), `10c6c76` (F-3), `3602ef6` (F-2), `fcbc509` (F-4), `a70fd88` (flag),
  `ef5c694` (doc links)

## Test counts

| | before (`ed56b39`) | after |
|---|---|---|
| `cargo test` passing | 1,095 | 1,102 |
| ignored (live smoke) | 11 | 11 |
| failing | 0 | 0 |

Seven tests added, none removed or weakened. `cargo clippy --all-targets -- -D warnings` and
`cargo fmt --check` are clean; `make test` (the GNOME, KDE and Omarchy contract suites) passes.
`cargo doc` warnings are unchanged from before — the four this change introduced were removed by
un-linking three now-private `Keys::seal` references.

---

## F-1 (blocker) — the nonce bound to a pre-image, not to the message

`seal_chunk` computed `id = chunk_id(plaintext)` and then encrypted `frame(plaintext)` under
`nonce_for(id)`. A derived nonce is sound only under **nonce ↔ message** injectivity, and this had
nonce ↔ *pre-image* injectivity. `f_zstd` is not a function of the plaintext alone — upstream
guarantees format compatibility across versions, not byte-identical output — so one id could cover
two distinct messages, sealed under one `(chunk_key, nonce, aad)`. That is a solvable Poly1305
one-time key, and `C_A ⊕ C_B = F_A ⊕ F_B` with an identical 4-byte `true_len` prefix as free known
keystream.

The audit's preferred remediation, applied verbatim:

```text
nonce  = derive_key(CTX_NONCE, keyed_hash(name_key, framed))[..24]
sealed = nonce ‖ XChaCha20Poly1305(chunk_key).encrypt(nonce, framed, aad = id)
```

- **`id` is untouched.** Still `keyed_hash(name_key, plaintext)`, computed before framing. Dedup
  keys on the same address, and both id pins in `sync_vectors.rs` hold.
- **The nonce is stored inline**, the same framing `seal_root` already used, because the reader
  cannot re-derive a keyed hash of the plaintext it has not decrypted yet. Cost: 24 bytes per
  chunk, 0.009% at 256 KiB.
- **Determinism is preserved.** Same plaintext → same frame → same nonce → byte-identical output.
  `sealing_one_fixture_twice_produces_identical_packs_and_identical_chunk_ids` still passes on a
  multi-megabyte fixture.
- **`Keys::seal` is `pub(crate)`**, with the `(id, message)` binding written as a safety contract:
  the nonce may be derived only from the exact bytes passed as `message`; the id may be anything
  that addresses them, because it is authenticated rather than trusted to be unique.
- `Keys::open` gained a length check before the split. The inline nonce added a new
  attacker-controlled length to slice on, and a truncated chunk must be an error, not a panic.

### The pins that moved, and the pins that did not

This is exactly the distinction 1-07 built into the pin labels, so it is worth stating plainly.

| Pin | Kind | Moved? |
|---|---|---|
| `the_chunk_id_of_a_fixed_plaintext_is_pinned…` | id | **no** |
| `the_multi_chunk_bundles_manifest_id_is_pinned…` (and the three payload ids) | id | **no** |
| `the_sealed_chunk_ciphertext_is_pinned…` | ciphertext | yes — re-pinned |
| `the_multi_chunk_bundles_pack_address_is_pinned…` | ciphertext | yes — re-pinned |
| `the_chunk_subkey_is_pinned_and_is_the_key_the_seal_path_uses` | ciphertext (reconstruction) | yes — reconstruction updated |
| KEK, three subkeys, keyfile wrap | — | no |

The id pins were checked **first**, and their holding is what says this was a sealing change rather
than a re-addressing event. The ciphertext pin now also asserts the nonce as a *property* —
`ciphertext[..24] == derive_key(CTX_NONCE, keyed_hash(name_key, framed))[..24]` — so a future change
that re-derived it from the id fails there, naming the reason, instead of merely moving bytes.

### The three "harmless" statements, corrected rather than deleted

`chunk.rs:16`, `docs/sync-format.md:156`, and the ciphertext-pin note in `sync_vectors.rs` all said
two zstd builds producing different ciphertext for one id was harmless. Each now says which half is
true: harmless **for dedup** — both decrypt to identical plaintext and the first upload wins — and
*not* harmless for nonce safety, since one id covering two distinct messages is precisely the input
that reuses a nonce. `docs/sync-format.md`'s claim that "a nonce is never reused across distinct
messages under `chunk_key`" was false as written and is now true, with the reasoning spelled out
next to it.

---

## F-3 (blocker) — anchor keying and root repo scoping

**Anchor keying** is a precondition on a caller a later phase will write, so it is stated in
`anchor.rs`'s module doc and in `docs/sync-format.md` §9: the anchor path must be keyed to the
**remote** (URL or account) and never to the remote's self-declared `repo_id`. `accept(None, …)`
returns `Ok(())` before comparing `repo_id`, so an `anchors/<repo_id>.json` sharding scheme — the
obvious way to hold several bundles, and unavoidable eventually since one `Anchor` holds exactly one
`repo_id` — would make a `repo_id` this machine has never seen resolve to an absent file, read as
first contact, and be accepted. Stated the other way round, which is the form worth remembering:
first contact is a property of *this machine and that remote*, and nothing the remote says may
manufacture it.

**Root scoping** is code. `aad = ROOT_AAD ‖ repo_id`, with the expected `repo_id` supplied by the
caller from local configuration:

```rust
pub fn seal_root(&self, plaintext: &[u8], repo_id: &str) -> Result<Vec<u8>>;
pub fn open_root(&self, framed: &[u8], repo_id: &str) -> Result<Zeroizing<Vec<u8>>>;
pub fn open(keys: &Keys, framed: &[u8], expect_repo_id: &str) -> Result<Root>;   // Root::open
```

A repository swap now fails the Poly1305 tag rather than depending on local state that an attacker
with local write access can delete, and cross-bundle root replay closes with it — two bundles
sharing a master key no longer open each other's roots. `Root::open` also rechecks the `repo_id`
inside the authenticated plaintext against the caller's expectation, the same belt and braces
`open_chunk` applies to a chunk id: the AAD proves the writer meant this bundle, the recheck proves
the two copies were not written to disagree.

Taking that expectation from the remote would make the binding say nothing, so the adversarial
suite's `restore_gated` passes its own `REPO_ID` constant with a comment saying why, rather than
reading `root.repo_id` back out.

---

## F-2 (medium) — a memory ceiling before the allocation

`check_memory_budget` had zero production call sites, so `unwrap_master_key` handed an
attacker-supplied `m_kib` straight to `derive_kek`. `argon2` 0.5.3 sets `MAX_M_COST = u32::MAX` and
allocates with an infallible `vec![]`, so one edited integer in a served keyfile is a
`handle_alloc_error` abort or an OOM kill — reached *before* the AAD binding gets its chance to make
the unwrap fail, and against the project's hard invariant that the widget always exits 0.

`MAX_KDF_MEMORY_KIB = 4 GiB` is now checked inside `unwrap_master_key`, beside the version gate and
ahead of any cryptographic work. Pure, clock-free, env-free. `check_memory_budget` keeps its job —
the actionable *does this fit on this machine* pre-flight for whichever surface owns the CLI — and
its doc now says that is what it is, rather than implying it is the safety net.

---

## F-4 (medium) — the passphrase floor coupled to the KDF cost

Two halves, because a floor alone only moves the line.

- **`MIN_KDF_MEMORY_KIB = 8 MiB`, enforced in `Keyfile::wrap`**, which covers `create` and `rewrap`
  — a rewrap is the other way to choose the parameters a bundle lives at, so a floor guarding only
  one of them would be a floor with a documented way around it. Argon2's own `8 * p` KiB floor is
  the smallest input the algorithm is *defined* for, not a security parameter.
- **`passphrase::check(pw, k)` takes the KDF parameters.** Below `KdfParams::default`'s memory it
  returns `Rejected` for anything under generated strength (20 characters, 100 bits), which is
  uncrackable at any KDF cost. Lowering `--kdf-memory` now trades against password strength rather
  than against security.

Reads are deliberately **not** floored. A bundle written before the floor existed must stay
openable; refusing to read it would destroy data to enforce a policy its owner cannot retroactively
satisfy.

---

## `Root.kdf` vs the keyfile's KDF (flag) — sentence deleted

`model.rs` said a mismatch "is a signal worth reporting". Nothing reported it. The sentence is
replaced by what is true: a reader uses the keyfile's parameters, the two copies are not compared,
and a disagreement is not something a remote can manufacture — `Root.kdf` sits inside the root's
authenticated plaintext and the keyfile's copy is AAD-bound, so only the key holder's own writer can
produce one. `docs/sync-format.md` §5 carries the same correction.

Deleted rather than implemented, deliberately: `src/sync/` is pure and has no warning channel, an
*error* would contradict "the keyfile wins" by making a cosmetic disagreement unreadable, and a
returned flag nobody consumes is the same zero-call-site failure F-2 was about. Whichever phase
gains a warning channel may add the comparison there.

---

## Judged differently from the audit

**One deviation, in F-4.** The audit says `MIN_KDF_MEMORY_KIB` is "enforced in `Keyfile::wrap`". It
is — but `wrap` takes the floor as an argument, and `Keyfile::create_with_floor` /
`rewrap_with_floor` are the test seam beside `create` / `rewrap`.

A hard 8 MiB floor with no seam would put every `Keyfile::create` in the repository — roughly
fifteen call sites across four modules and two integration binaries — through a real Argon2id
derivation a thousand times more expensive than the cheap seam's. The AUR `check()` runs
`cargo test` during `makepkg` on an installer's machine, and the hermetic-test invariant exists
precisely to keep that cheap. The shape is the project's existing one: `Cache::at` beside
`Cache::for_vendor`, `Manifest::open_with_ceiling` beside `open`. Production calls `create` and
`rewrap`, which pass the constant; the seam's doc says a caller wanting a lower floor wants
`MIN_KDF_MEMORY_KIB` lowered, where the argument for it can be read.

**One addition beyond the audit.** `Keyfile::wrap` also checks the *ceiling*, not only the floor. A
user typing `--kdf-memory 99999999` would otherwise hit the same infallible allocation F-2 is about,
from the write side, where no attacker is needed at all.

**Two things the audit did not ask for and were still done**, both one-liners falling out of the
change: `Keys::open`'s truncation check (the inline nonce created the slice that needed it), and
`Root::open`'s `repo_id` recheck against the plaintext copy.

## Carried forward

- **The anchor's path resolver does not exist yet.** F-3's first half is a stated precondition, and
  the phase that writes the config-directory wrapper is the one that can be tested against it. It is
  written in two places a future implementer will actually read: the module they will call, and the
  format document a re-implementer works from.
- **`check_memory_budget` still has no production call site.** That is now correct rather than a
  gap — the hard ceiling is inside the unwrap, and the budget check belongs to whichever surface
  owns `--kdf-memory` and knows how to print an actionable refusal.
- **Pin provenance.** The three re-pinned ciphertext vectors were produced by running the
  implementation once against this tree, exactly as 1-07 produced the originals. The crate versions
  they were pinned against are unchanged: `argon2` 0.5.3, `chacha20poly1305` 0.11.0, `blake3` 1.8.6,
  `zstd` 0.13.3.
