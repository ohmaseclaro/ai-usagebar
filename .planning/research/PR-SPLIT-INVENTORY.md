# PR split inventory — what actually has to move, and how big each piece is

Measured from `git diff upstream/main...HEAD` at upstream `526ea17` (v1.4.0).
Whole change, `.planning/` excluded: **75 files, +53,409 / -71**.

The 51k figure the maintainer objected to is mostly test code. `src/sync` alone is
+40,328 lines, and **more than half of that is inline `#[cfg(test)]`**. That does not
make the change small enough to review in one pass — his point stands — but it does
change what each staged PR costs a reviewer.

## Proposed grouping

### PR 1 — format + bounded KDF/keyfile core

**6 files — 2,390 code + 2,058 test = 4,448 lines**

| File | code | test |
|---|---:|---:|
| `src/sync/crypto.rs` | 832 | 498 |
| `src/sync/mod.rs` | 386 | 165 |
| `src/sync/model.rs` | 376 | 542 |
| `src/sync/pack.rs` | 301 | 320 |
| `src/sync/passphrase.rs` | 265 | 288 |
| `src/sync/chunk.rs` | 230 | 245 |

### PR 2 — local archive + restore

**11 files — 5,134 code + 7,484 test = 12,618 lines**

| File | code | test |
|---|---:|---:|
| `src/sync/restore/report.rs` | 776 | 571 |
| `src/sync/index.rs` | 695 | 536 |
| `src/sync/push/packer.rs` | 578 | 837 |
| `src/sync/restore/fetch.rs` | 567 | 1,011 |
| `src/sync/plan.rs` | 522 | 922 |
| `src/sync/restore/mod.rs` | 512 | 980 |
| `src/sync/scope.rs` | 463 | 745 |
| `src/sync/restore/write.rs` | 415 | 840 |
| `src/sync/restore/layout.rs` | 274 | 356 |
| `src/sync/restore/backup.rs` | 251 | 445 |
| `src/sync/transcripts.rs` | 81 | 241 |

### PR 3 — remote transport + lifecycle

**17 files — 8,503 code + 10,597 test = 19,100 lines**

| File | code | test |
|---|---:|---:|
| `src/sync/cli.rs` | 1,356 | 2,442 |
| `src/sync/report.rs` | 1,063 | 1,067 |
| `src/sync/github/setup.rs` | 916 | 1,367 |
| `src/sync/github/write.rs` | 847 | 853 |
| `src/sync/push/progress.rs` | 642 | 398 |
| `src/sync/push/mod.rs` | 610 | 108 |
| `src/sync/github/token.rs` | 425 | 226 |
| `src/sync/github/gate.rs` | 421 | 488 |
| `src/sync/push/upload.rs` | 369 | 583 |
| `src/sync/github/http.rs` | 356 | 488 |
| `src/sync/push/prune.rs` | 288 | 765 |
| `src/sync/github/mod.rs` | 264 | 238 |
| `src/sync/push/rekey.rs` | 251 | 668 |
| `src/sync/github/pairing.rs` | 249 | 223 |
| `src/sync/push/pointer.rs` | 206 | 580 |
| `src/sync/anchor.rs` | 171 | 103 |
| `src/sync/github/keychain.rs` | 69 | 0 |

### Companion repo — credentials

**2 files — 1,776 code + 2,386 test = 4,162 lines**

| File | code | test |
|---|---:|---:|
| `src/sync/keystore.rs` | 1,146 | 785 |
| `src/sync/restore/merge.rs` | 630 | 1,601 |

## Files whose group is genuinely arguable

- `src/sync/restore/merge.rs` — the three-way merge is generic, but its credential
  disposition (`ReplacesLiveCredential`, `force_credentials`) is the exact thing the
  maintainer wants behind an independent review. Splitting the file may be better than
  choosing a side for it.
- `src/sync/report.rs` and `src/sync/cli.rs` — user-facing surface for all three PRs.
  Landing them whole in PR 3 means PRs 1 and 2 ship with no way to invoke them.
  The alternative is a thin slice of each in every PR.
- `src/sync/keystore.rs` — credentials by definition; companion repo under the ownership
  decision. But `Store::` is referenced from scope and restore, so the seam has to exist
  upstream even when the credential stores do not.

## Outside `src/sync`

| Area | files | +lines |
|---|---:|---:|
| `tests/` (5 e2e/vector/adversarial suites) | 5 | 7,419 |
| `docs/` | 4 | 1,860 |
| `src/claude_desktop/` | 4 | 608 |
| `macos/` | 2 | 590 |
| `src/tui/` | 2 | 747 |
| `src/widget/` | 2 | 485 |
| `src/cursor/` | 1 | 384 |
| `src/config.rs` | 1 | 222 |
| `README`, `CHANGELOG`, `Cargo.*`, packaging, misc | ~10 | ~600 |

`Cargo.toml` gains 24 lines of dependencies. Every one of them was already in the tree
for another purpose — the milestone held a zero-new-crate rule — which is worth stating
in PR 1 rather than leaving the reviewer to check 24 lines by hand.
