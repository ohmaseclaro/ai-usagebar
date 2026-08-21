---
phase: 01-encrypted-bundle-core
plan: 11
subsystem: crypto
tags: [security-remediation, kdf-bounds, api-visibility, associated-data, passphrase-policy, documentation-accuracy]

# Dependency graph
requires: [1-05, 1-06, 1-07, 1-09, 1-10]
provides:
  - "crypto::MAX_KDF_MEMORY_KIB checked inside derive_kek itself, so every caller in and out of the crate is bounded"
  - "Keyfile::{create_with_floor,rewrap_with_floor} are pub(crate) — Keyfile::create is the only exported constructor and it enforces MIN_KDF_MEMORY_KIB"
  - "root_aad refuses an empty repo_id, on seal and on open"
  - "passphrase docs, WEAKENED_KDF message, crypto docs and docs/sync-format.md §1/§9 describe the lowered-KDF band as the length rule it is"
  - "The missing object-type domain separator in the chunk AAD, recorded as a known gap owned by phase 2"
affects: [phase-2, phase-3, phase-4, phase-5]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "A bound on attacker-supplied input belongs in the shared function every caller routes through, not in the callers that remembered it — one guard is a smaller diff than a guard per call site, and it is the one a future caller cannot forget"
    - "A test seam that takes a policy value as an argument must be pub(crate); exported, it is not a seam but a documented way around the policy"
    - "An integration test needing a cheap keyfile builds one from the public fields, the same way the format-vector test already has to"
    - "Documentation that describes an entropy rule the code enforces as a length rule is a defect, not a wording preference — fix the documents or fix the code, never leave them disagreeing"

key-files:
  created:
    - .planning/phases/01-encrypted-bundle-core/1-11-SUMMARY.md
  modified:
    - src/sync/crypto.rs
    - src/sync/passphrase.rs
    - tests/sync_adversarial.rs
    - tests/sync_vectors.rs
    - docs/sync-format.md
    - .planning/phases/01-encrypted-bundle-core/1-SECURITY.md

key-decisions:
  - "NEW-1: the ceiling moved *into* derive_kek and was deleted from wrap and unwrap_master_key. Neither produced a better message than the shared one, and a second copy of one rule is the shape that let a third caller past it"
  - "F-4: create_with_floor/rewrap_with_floor went pub(crate) rather than #[doc(hidden)] — hiding them from rustdoc would leave the measured bypass fully callable"
  - "F-4: Keyfile's fields stay pub, and the tradeoff is written into the struct doc. They are what lets sync_vectors wrap a keyfile from the documented format by hand and prove Keyfile::open accepts it — the only evidence the document and the implementation agree"
  - "F-4b: the documents were corrected, not the code. Carrying provenance out of generate() would refuse a generated passphrase pasted back from a password manager — more code and worse behaviour"
  - "NEW-2: the check went in root_aad, not seal_root, because both seal_root and open_root route through it; refusing symmetrically costs no reader anything since no build has ever written such a root"
  - "NEW-3: deferred to phase 2 with the analysis written down. A type byte in the AAD moves every sealed byte and ~60 call sites, and would relocate the ciphertext pins — a versioned format change, not a remediation edit"

patterns-established:
  - "Visibility is part of a security control: F-1's pub(crate) on Keys::seal and this plan's on the floor seams are the same rule applied twice"
  - "A known gap is recorded in the code that owns it and in the format document a re-implementer reads, with the trigger that forces the fix ('adding a new object kind under chunk_key')"

requirements-completed: []

coverage:
  - id: D1
    description: "derive_kek refuses a KDF memory parameter above the ceiling itself, so the idiom KdfDoc::params recommends — derive_kek(pw, &salt, keyfile.kdf.params()) with a hostile m_kib — errors instead of aborting in handle_alloc_error"
    requirement: CRYPTO-02
    verification:
      - kind: unit
        ref: "cargo test --lib sync::crypto — derive_kek_refuses_the_ceiling_itself_rather_than_trusting_its_callers"
        status: pass
    human_judgment: false
  - id: D2
    description: "The read-path ceiling still fires through Keyfile::open with the check now one layer down, and the version gate still precedes any derivation"
    requirement: CRYPTO-02
    verification:
      - kind: unit
        ref: "cargo test --lib sync::crypto — a_keyfile_demanding_more_memory_than_the_ceiling_is_refused_before_allocating, a_keyfile_above_the_read_ceiling_is_refused_before_any_cryptographic_work"
        status: pass
    human_judgment: false
  - id: D3
    description: "MIN_KDF_MEMORY_KIB now binds every exported write path: create_with_floor and rewrap_with_floor are pub(crate), so tests/sync_adversarial.rs cannot reach them and does not"
    requirement: CRYPTO-02
    verification:
      - kind: unit
        ref: "cargo test --lib sync::crypto — a_new_keyfile_below_the_memory_floor_is_refused_through_both_entry_points; compile-enforced: tests/sync_adversarial.rs builds its own keyfile"
        status: pass
    human_judgment: false
  - id: D4
    description: "The lowered-KDF band is pinned as what it is — the accepted length rises from 12 characters to 20, and a typed 20-character password clears it exactly as a generated one does"
    requirement: CRYPTO-06
    verification:
      - kind: unit
        ref: "cargo test --lib sync::passphrase — a_lowered_kdf_cost_raises_the_accepted_length_from_twelve_to_twenty"
        status: pass
    human_judgment: false
  - id: D5
    description: "An empty repo_id is refused on seal and on open, so the root's associated data can never degenerate to the bare literal"
    requirement: CRYPTO-05
    verification:
      - kind: unit
        ref: "cargo test --lib sync::crypto — an_empty_bundle_identifier_is_refused_rather_than_scoping_the_root_to_nothing"
        status: pass
    human_judgment: false
  - id: D6
    description: "Every id pin and every ciphertext pin holds unchanged — none of these fixes altered what is sealed"
    requirement: CRYPTO-01
    verification:
      - kind: unit
        ref: "cargo test --test sync_vectors — all 13"
        status: pass
    human_judgment: false
  - id: D7
    description: "The nine adversarial refusals still fire with nine distinct messages against a hand-wrapped keyfile, so replacing the pub(crate) seam changed no outcome"
    requirement: CRYPTO-03
    verification:
      - kind: unit
        ref: "cargo test --test sync_adversarial — all 13"
        status: pass
    human_judgment: false

# Metrics
duration: 55min
completed: 2026-08-19
status: complete
---

# Phase 1 Plan 11: Residual Security Findings Summary

**Four findings closed and one deferred with its analysis written down. Nothing here changes what
is sealed, so every id pin and every ciphertext pin in `sync_vectors.rs` holds unchanged. The
theme across three of the four is the same: a control that was applied at the call sites that
remembered it rather than at the thing everything routes through.**

## Performance

- **Duration:** ~55 min
- **Findings closed:** 4/5 (NEW-1, F-4, F-4b, NEW-2); NEW-3 deferred deliberately
- **Files modified:** 6 (two modules, two test binaries, the format document, `1-SECURITY.md`)
- **Commits:** `8f3ad18` (NEW-1), `7e58e56` (F-4), `bae5244` (F-4b), `f33dab4` (NEW-2),
  `ae1f517` (NEW-3, documentation)

## Test counts

| | before (`9c9ef9a`) | after |
|---|---|---|
| `cargo test` passing | 1,102 | 1,104 |
| ignored (live smoke) | 11 | 11 |
| failing | 0 | 0 |

Two tests added (`derive_kek_refuses_the_ceiling_itself_rather_than_trusting_its_callers`,
`an_empty_bundle_identifier_is_refused_rather_than_scoping_the_root_to_nothing`), one renamed and
strengthened, none removed or weakened. `cargo clippy --all-targets -- -D warnings` and
`cargo fmt --check` are clean; `make test` — `cargo test` plus the GNOME, KDE and Omarchy contract
suites — passes. `cargo doc` warnings are unchanged: the two seams that went `pub(crate)` had their
incoming links from public docs converted to plain code spans first.

---

## NEW-1 — the ceiling guarded two callers instead of the rule

`derive_kek` is `pub` and had no bound of its own; `check_kdf_ceiling` was private, so an outside
caller could not have enforced it even wanting to. Meanwhile `KdfDoc::params` is `pub` and its own
doc tells callers to use the keyfile's stored parameters, which makes

```rust
derive_kek(pw, &salt, keyfile.kdf.params())   // m_kib is whatever the remote wrote
```

the *recommended* idiom — and an unbounded path into `argon2` 0.5.3's infallible `vec![]`, i.e. a
`handle_alloc_error` abort, against the hard invariant that the widget always exits 0. Two
non-`Keyfile` callers already exist in `tests/live.rs` and `tests/sync_vectors.rs`, so the shape is
not hypothetical.

`check_kdf_ceiling(k.m_kib)?` is now the first statement of `derive_kek`. Both old call sites were
**deleted** rather than kept: neither produced a better message than the shared one, and the audit's
own framing is the argument — a second copy of one rule is precisely the shape that let a third
caller past it. What survives at the call sites is what is genuinely different there:
`Keyfile::wrap` keeps the *floor* (write-path only), and `unwrap_master_key` keeps the version gate
ahead of everything, so refusing a too-new bundle still costs nothing. Between that gate and the
ceiling there is now base64 decoding and nothing else — no allocation, no derivation.

`check_kdf_ceiling` stays private, and its doc says why: with `derive_kek` applying it to every
derivation there is nothing left for an outside caller to enforce by hand.

## F-4 — the floor seam was a hole, not a seam

Measured by the auditor: `create` at 8 KiB is refused, `create_with_floor` at 8 KiB writes the
keyfile and it opens. Both `_with_floor` functions were `pub`, no `cfg`, no `#[doc(hidden)]` — so
`MIN_KDF_MEMORY_KIB` bound `create` and `rewrap` rather than the format, and the inconsistency was
inside one remediation: F-1 had narrowed `Keys::seal` to `pub(crate)` for exactly this reason.

Both are now `pub(crate)`. Not `#[doc(hidden)]`: hiding them from rustdoc would leave the measured
bypass fully callable, which closes nothing.

The export was not forced by the tests. `tests/sync_adversarial.rs` was the only outside caller, and
it now wraps its own keyfile from the public fields — the pattern `tests/sync_vectors.rs` already
needs, because `create` draws a random master key and so can never anchor a vector. One helper,
three call sites:

```rust
fn wrap_by_hand(seed: u8, k: KdfParams) -> Keyfile   // seed picks the master key *and* the salt
```

The seed picks both, deliberately: two keyfiles from the helper are two hierarchies **and** two
KEKs, so the fixed wrap nonce is never reused. One nonce under one KEK wrapping two different master
keys is the misuse that file exists to catch, and it would have been embarrassing to introduce it
there.

### `Keyfile`'s fields stay `pub`, and the tradeoff is written down

Narrowing them would break `sync_vectors.rs`'s `fixed_keyfile()`, which assembles a keyfile from
`docs/sync-format.md` §1 by hand and asserts `Keyfile::open` accepts it. That assertion is the only
evidence in the repository that the *documented* wrap format and the implemented one are the same
thing, and it is irreplaceable — a round trip through `create`/`open` proves only self-consistency.

The residual is much smaller than the one that was closed. Filling `wrapped_master_key` means
running Argon2id and XChaCha20-Poly1305 yourself against the canonical AAD; a caller who does that
has brought their own crypto and is not being handed a bypass. What was actually dangerous was an
exported entry point that *takes the floor as an argument*, which needs no crypto knowledge at all.
The struct doc now says this rather than leaving it implied.

## F-4b — four documents describing behaviour that does not exist

`passphrase::check` returns `Strong` for any password of at least `RECOMMENDED_CHARS` **before** it
looks at `k.m_kib`, and `GENERATED_CHARS == RECOMMENDED_CHARS == 20`, so it cannot tell 20 typed
characters from `generate()`'s 100 bits. The real behaviour below default memory is that the floor
rises from 12 characters to 20. Four places called it an entropy rule — "refused unless it is of
generated strength" — which is the same defect class 1-10 closed for `Root.kdf`, reintroduced by the
fix for F-4 and, worst of all, sitting in the format document a re-implementer works from.

**The documents were corrected, not the code.** Making the rule real means `generate()` returning a
provenance-carrying type — more code, and *worse behaviour*: a generated passphrase kept in a
password manager comes back through the same stdin as anything else, so the type would refuse
exactly the users it exists to bless. Provenance is lost at the paste, not at the type boundary.

Corrected in six places, not four — the audit named the docs, and the same claim was also in
user-facing text and in the phase's own security record:

| Location | Now says |
|---|---|
| `src/sync/passphrase.rs` module doc | the accepted length rises 12 → 20, plus why length is a weak proxy and why provenance was rejected |
| `src/sync/passphrase.rs` `check` doc | "a longer password, not a provably stronger one" |
| `src/sync/passphrase.rs` `WEAKENED_KDF` | the message a user actually reads: "the minimum is 20 characters rather than 12" |
| `src/sync/crypto.rs` `MIN_KDF_MEMORY_KIB` | length, not measured entropy |
| `docs/sync-format.md` §1 and §9 | §9 gained a paragraph saying a 20-character typed password **is** accepted and an implementation refusing one is not following the document |
| `1-SECURITY.md` | the remediation line corrected, with a note saying what it used to claim |

Each says plainly that length is a proxy: honest at the bottom (6 characters is weak whoever chose
it) and weak at the top, where 20 may mean 100 uniform bits or a remembered phrase worth 40. The
renamed test carries the assertion that keeps them honest —
`check("twenty-characters-!!", lowered) == Strong` — so the next agent to write "generated strength"
has a red assertion telling them otherwise.

## NEW-2 — an empty `repo_id`

`root_aad("")` is the bare `ROOT_AAD` literal: the pre-F-3 global constant, identical in every
bundle in the world, so two bundles that both left the identifier empty and shared a master key
opened each other's roots again. Nothing required it to be non-empty.

The check went into `root_aad`, which returns `Result<Vec<u8>>` now — it is the one function both
`seal_root` and `open_root` route through, so no path can forget it, and it is one `?` at each. The
refusal is symmetric: refusing on read costs no reader anything, because no build has ever been able
to *write* such a root. `docs/sync-format.md` §5 states the requirement for a re-implementer.

## NEW-3 — object-type confusion in the chunk AAD: **deferred to phase 2, deliberately**

Confirmed as described. Every object sealed through `seal_chunk` — data chunk, manifest chunk, index
chunk, pack header — uses `chunk_key` with `aad = its own chunk_id` and no domain separator saying
which kind it is. Serde ignores unknown fields, so an `IndexObject` structurally deserializes as a
`PackHeader` (`IndexEntry`'s `pack` field is simply dropped).

**Why it dead-ends.** The id is still bound as AAD and `open_chunk` still rechecks
`chunk_id(plaintext) == id`, so the confusion needs the attacker to serve a *genuine* object of the
wrong kind under its own genuine id. From there `pack::read_header`'s `entries_within` bounds check
and the per-blob Poly1305 tags refuse it: an error, no plaintext, no forgery. Same reading as the
auditor's — a confused read that errors, not a compromise — and it pre-dates this remediation.

**Why it is not fixed here.** A type byte in the AAD is not a local edit:

- it changes `Keys::{seal,open}`, `chunk::{seal_chunk,open_chunk,seal_all,reassemble}`,
  `model::{seal_object,open_object}`, `pack::{finish_at_version,read_header,open_blob}` — every
  signature that carries a chunk from one layer to the next;
- ~60 call sites across four modules and two integration binaries;
- it moves **every ciphertext pin** in `tests/sync_vectors.rs`, because the AAD feeds the Poly1305
  tag. The id pins would hold, but the plan's own instruction is that ciphertext pins may move only
  when what is sealed changes — which is exactly what this is, and exactly why it wants a plan;
- and it rewrites §3–§5 of the format document, which was stabilised days ago.

That is a versioned format change. Rushing one onto a just-pinned format to close a
confirmed-dead-end confusion trades a real risk for a theoretical one. It is recorded in the two
places it will be read — `Keys::seal`'s safety contract and `docs/sync-format.md` §3 — each naming
the trigger that must force the fix: **an implementation adding a new kind of object under
`chunk_key` introduces the domain separator first.**

The cheap alternatives were considered and rejected. `#[serde(deny_unknown_fields)]` would make the
structural confusion fail to parse, but it contradicts the format's documented forward-compatibility
story ("a v2 object may carry fields this build has never heard of", `probe_version`). Disjoint
`format` ranges per object would repurpose the version field as a type tag, breaking the version
gates and the pins for a partial fix.

## Carried forward

- **NEW-3 is phase 2's**, with the analysis above and the two in-code notes as its brief.
- **No pin moved.** Nothing in this plan changed what is sealed, which is why all 13 vectors and all
  13 adversarial cases pass untouched. The crate versions the vectors were pinned against are
  unchanged: `argon2` 0.5.3, `chacha20poly1305` 0.11.0, `blake3` 1.8.6, `zstd` 0.13.3.
- **`tests/sync_adversarial.rs` now imports `chacha20poly1305`.** `sync_vectors.rs`'s claim to be the
  only file outside `crypto.rs` reaching for the primitives was corrected rather than left stale: it
  remains the only one reaching for `argon2`, and the only one calling a primitive to *check* it
  rather than to build a fixture. The containment invariant
  (`only_the_crypto_module_imports_the_cryptographic_crates`) walks `src/sync/` and is unaffected.
