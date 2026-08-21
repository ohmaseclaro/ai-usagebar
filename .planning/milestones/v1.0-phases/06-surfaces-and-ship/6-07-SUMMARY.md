---
phase: 6
plan: 7
subsystem: sync
tags: [sync, setup, keyfile, multi-machine, github]
requires: [4-01 upload::ensure_keyfile, 5-01 restore::fetch, 3-04 github::setup]
provides: [second-machine-join]
affects: [src/sync/github/setup.rs, src/sync/restore/fetch.rs]
status: complete
---

# Phase 6 Plan 7: The second machine is no longer read-only

`sync setup` now reads the snapshot pointer at step 3 and, when the repository
already holds a published bundle, adopts that bundle's keyfile instead of
minting a rival master key.

## Signature changes (read this first)

Three, all `pub(crate)` or narrower except the first, which is on a **public
trait** and is therefore breaking for any outside implementor:

1. **`SetupPrompt` gains a required method.** Quoted exactly as it now reads in
   `src/sync/github/setup.rs`:

   ```rust
   fn existing_passphrase(&mut self) -> Result<Zeroizing<String>>;
   ```

   Implemented for `TtyPrompt` (production) and for both test doubles —
   `setup::Double` in the crate and `Joining` in `tests/sync_restore_e2e.rs`.
   No default body: a double that silently inherited one would answer the join
   path with whatever the generate path was scripted with.

2. `setup::write_keyfile(&Path, &Keyfile)` → `write_keyfile(&Path, &[u8])`.
   Private. The two step-3 shapes disagree about what the file should contain:
   `generate` pretty-prints its own new keyfile, `join` writes the published
   asset verbatim.

3. `restore::fetch::{find_release, asset_index, download}` take
   `(client, repo, now)` rather than `&RestoreCtx`. Private, mechanical, and
   what lets `published_keyfile` reuse them instead of copying the release
   lookup, the listing and the size ceiling.

## The gap

`setup::run` never looked at `Pointer.keyfile`. It always ran
`Keyfile::create(chosen_pw, kdf)`. On a second machine that meant:

- **`sync pull` worked** — `restore::fetch::resolve` downloads the keyfile the
  *pointer* names and opens it with the typed passphrase, so the divergent
  local keyfile was never consulted.
- **`sync push` was refused** — `upload::assert_keyfile_is_current` compared
  the local keyfile's canonical content address against `Pointer.keyfile`, saw
  a mismatch, and correctly refused rather than republishing a divergent
  wrapper.

A machine that can only read is not a second machine, and two machines
continuing each other's work is the reason the milestone exists.

## What was built

`run`'s step 3 now branches on `pointer::load(&client, &repo,
&push::repo_id_for(drift.record.repo_id), now)`. `run` was already `async` and
already held the step-2 gate's `Client`, so the read costs one request and no
new plumbing.

- **`None`** → `generate(prompt)`, which is today's code moved verbatim into a
  function. Same narration, same prompts, same `Keyfile::create`, same
  once-only display of the generated passphrase.
- **`Some(pointer)`** → `join(...)`. Downloads the asset the pointer names,
  asks for the *existing* passphrase, and returns the downloaded bytes.

Both return `(Vec<u8>, Keys)`, so step 5 has exactly one write for either
shape.

### Adoption is proven, not assumed

The downloaded keyfile is attacker-controlled bytes from a hostile remote, and
the pointer that named it is the one unauthenticated link in the chain. The
only thing that authenticates it is that the passphrase unwraps the master key
— `Keyfile::open`, an AEAD tag over the keyfile's own `{format, kdf}`
associated data. If it fails, `join` returns an error and nothing is written.

- **A wrong passphrase leaves no keyfile on disk.** Step 5 remains the only
  place anything persists (the F-10 ordering), and `join` refuses well before
  it. That matters because `existing_keyfile_message` refuses to overwrite an
  existing keyfile: one written on a failed attempt would strand the user
  behind a file that opens with a password nobody has.
- **Re-prompt, capped at three** (`JOIN_ATTEMPTS`), the way the generate path
  re-prompts on a passphrase under the strength floor. A cap rather than the
  generate path's unbounded loop because each attempt here costs an Argon2id
  derivation and an AEAD unwrap, driven by whatever is on stdin.
- **One message for two failures.** "Wrong password" and "not this bundle's
  keyfile" read identically; separating them is the oracle Phase 1 refused to
  build. `restore::fetch::resolve` already makes the same choice.
- **No strength floor on the join path.** The password already exists on
  another machine; refusing it for weakness would lock a user out of their own
  bundle.

### `Keyfile::create` is unreachable from the join path

Structurally: it is called only inside `generate`, and `run` calls exactly one
of the two. A generated-then-discarded key is a passphrase shown to a user,
under a "there is no recovery" warning, for a string that was never a key to
anything. The integration double's `passphrase` method **panics**, so a
regression that reintroduces it fails at the panic rather than at an assertion
three steps later.

### Bytes, verbatim

`join` returns the downloaded bytes and step 5 writes them unchanged, so the
local file is byte-identical to the published asset. That is what makes the
first push *accepted*: `assert_keyfile_is_current` addresses the canonical
serialization of the local file, and a re-serialization here would be a second
place for the two to diverge.

### The prompt seam

`SetupPrompt::passphrase(&mut self, generated: &str)` is documented as
"`generated` has already been displayed". Joining has no generated value, and
`passphrase("")` would make `TtyPrompt` print *"Press Enter to take the
generated passphrase"* — an instruction that takes an empty string as the
password. Hence the distinct method (quoted above).

### Bounds before allocation

`restore::fetch::published_keyfile` composes `find_release`, `asset_index` and
`download` — the same three verbs `resolve` uses, none of which takes a
`gate::Pushing`, so the join path is as structurally incapable of writing to
the remote as a restore is. It reuses `download`'s `MAX_ASSET_BYTES` ceiling,
checked against the size the release listing declares, before the request that
would allocate it. No second number was invented. The hostile-`m_kib` ceiling
is already `derive_kek`'s `MAX_KDF_MEMORY_KIB`, which every derivation routes
through.

## Constraints honoured

- Steps 1, 2 and 4 were not moved. The private-repo gate stays before this; the
  pointer read sits inside step 3.
- Step 5 remains the only place anything persists.
- Nothing prints, logs or embeds the passphrase, the keyfile bytes, the token,
  or a signed URL's query string. Asserted: the join test greps the narration
  for the password and the refusal test greps the error for the attempt.
- `Cargo.toml` and `Cargo.lock` are byte-identical (`git diff --stat` on both
  is empty). No crate added.

## Tests

The round trip is `tests/sync_restore_e2e.rs::a_second_machine_joins_the_published_bundle_and_its_push_is_accepted`,
driven through the real orchestrators against the file's existing stateful
mockito fake:

1. Machine A (alice roots) seeds a tree and `push::run`s it.
2. Machine B — separate `TempDir`, bob roots, no keyfile, no pairing record, no
   index — runs `setup::run` against the same remote with A's password. Its
   local keyfile is asserted **byte-identical** to the asset the fake holds.
3. B seeds a file and `push::run`s. Accepted: the pointer grows to two
   snapshots and still names the same keyfile asset.
4. A `restore::run`s and B's file arrives byte for byte.

**It fails without the change**, and was confirmed to: with step 3 forced back
to `generate`, B's keyfile is the pretty-printed 8-MiB-KDF wrapper it minted
rather than A's published one, and the byte-identity assertion fails on exactly
that.

Also added:

- `a_wrong_password_on_the_second_machine_leaves_nothing_to_strand_it`
  (integration) — refuses, writes no keyfile and no pairing record, and leaves
  the remote pointer byte-identical.
- `a_repository_that_already_holds_a_bundle_is_joined_not_re_keyed` (unit) —
  byte-identity, `existing_passphrase` reached and `passphrase` not, and
  "generated passphrase:" never printed.
- `a_wrong_passphrase_on_join_refuses_and_writes_nothing` (unit) — three
  attempts exactly, no keyfile, no pairing record, no stored token, and the
  attempts are not echoed in the error.
- `a_corrupted_published_keyfile_is_refused_before_the_password_ask` (unit) —
  unreadable asset bytes refuse before the user is asked for anything.
- `an_empty_repository_still_generates_and_shows_the_passphrase_once` (unit) —
  the first-run path is unaffected and the generated line is printed exactly
  once.

Hermeticity: every root is a `TempDir`, both `Endpoints` fields point at
mockito, `now` is a constant, and **`store_token` is overridden in every
double** — its production default writes the real macOS login Keychain, and the
AUR `check()` runs `cargo test` during `makepkg`.

Two existing fixtures gained a `sync/pointer.json` 404 mock
(`setup::tests::drive` and `cli::tests::the_success_line_reports_the_token_source_and_never_the_token`),
because step 3 now reads the pointer and an unmocked path answers with
mockito's 501.

## Production call sites of everything added

Enumerated as asked; **none is zero**.

| Added | Production call site |
|---|---|
| `SetupPrompt::existing_passphrase` (trait method) | `setup::join`, reached from `cli::setup` → `TtyPrompt` → `setup::run` step 3 |
| `TtyPrompt::existing_passphrase` (impl) | the same, via `cli::setup`'s `TtyPrompt` |
| `setup::join` | `setup::run` step 3, the `Some(pointer)` arm |
| `setup::generate` | `setup::run` step 3, the `None` arm |
| `setup::JOIN_ATTEMPTS` | `setup::join`'s loop bound and its refusal message |
| `restore::fetch::published_keyfile` | `setup::join` |

## Verify

- `cargo test` — **1730 passing, 0 failing** (baseline 1724; +4 lib, +2
  integration). Run with `< /dev/null`.
- `cargo clippy --all-targets -- -D warnings` — clean.
- `cargo fmt --check` — clean.
- `make test` — clean, including the GNOME, KDE and Omarchy frontend contract
  suites.
- `git diff --stat -- Cargo.toml Cargo.lock` — empty.

## Deviations from plan

None in substance. One test written and then removed: a "pointer names an asset
the release does not carry" case, which the shared `drive_joining` fixture
cannot express (it registers the asset under the name the pointer gives) and
which `restore::fetch::download`'s own suite already covers.

## Known stubs

None.

## Deferred

`cli::rekey` calls `prompt.passphrase("")` twice — the same misnomer this plan
fixes for setup, so its two asks print "Press Enter to take the generated
passphrase" when there is no generated passphrase. Pre-existing, not in scope,
and a sibling plan is editing narration in this area; logged to
`deferred-items.md` rather than changed here.

## Self-Check: PASSED

- `src/sync/github/setup.rs`, `src/sync/restore/fetch.rs`, `src/sync/cli.rs`,
  `tests/sync_restore_e2e.rs` — all present and modified.
- Commit `5dc731e` — found in `git log`.
