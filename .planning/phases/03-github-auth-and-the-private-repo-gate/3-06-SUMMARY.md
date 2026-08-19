---
phase: 03-github-auth-and-the-private-repo-gate
plan: 06
type: summary
date: 2026-08-19
---

# 3-06 Execution Summary

**CAL-1: declined.** Offered in phase 3, not run. Ranged reads on a private-repo
release asset are still *assumed* unsupported; `PACK_TARGET` stays 32 MiB and
`PACK_MAX` stays 48 MiB. It is no longer a blocker for anything — see below.

**`permissions.admin`: declined, question open.** Not measured. D-03's runtime
warning stays unshipped and `docs/sync-github.md`'s token recipe stays its sole
enforcement, exactly as plan 3-04 left it.

Both probes exist, are `#[ignore]`d, compile, and skip with a printed message.
Neither outcome blocks Phase 4. **Declining was the correct outcome for this
run, not a failure**: this execution had no GitHub token and no repository, and
the plan makes a decline a first-class answer for precisely that case.

## Deliverables

✓ `tests/live.rs` — new `#[ignore]`d `permissions_shape_for_a_fine_grained_contents_token`
✓ `tests/live.rs` — CAL-1's `cal1_range_on_private_release_asset` verified against current code, unchanged
✓ `docs/sync-format.md` §7 — CAL-1 rewritten as an explicit decline; new open-question entry for `permissions.admin`
✓ `docs/sync-format.md` §4 — the sizes paragraph agrees with §7
✓ `src/sync/pack.rs` — `PACK_TARGET`'s doc comment reconciled with what §7 now says
✓ `docs/sync-calibration.md` — cross-reference no longer implies CAL-1 is Phase 1's last word
✓ `docs/sync-github.md` — removed a promise of a warning that does not ship

Commits: `070c9ca` (probe), `7b9e722` (docs + constant).

---

## Deferred verification — the exact invocations

**Neither probe was run.** Nothing in this plan's output is a measurement. Both
need a real token this execution did not have. To answer either question later,
run exactly this.

### CAL-1 — does a private-repo release asset honour `Range:`?

Setup: a **throwaway** private repository with one release carrying an asset a
little over 1 MiB, and a fine-grained PAT scoped to it with `Contents: Read`.
Not the repository you paired for sync — there is no reason to point a
hand-rolled probe at a real backup target. Delete the repository and revoke the
token afterwards.

```bash
GSD_CAL1_TOKEN=<throwaway fine-grained PAT, Contents: Read> \
GSD_CAL1_REPO=<owner>/<throwaway-repo> \
GSD_CAL1_ASSET=<asset file name> \
  cargo test --test live -- --ignored --nocapture \
    cal1_range_on_private_release_asset
```

Read the last line. It says either `CAL-1 = Range IS honoured` (a `206` with a
`Content-Range`) or `CAL-1 = Range is NOT honoured`, with the byte count showing
the whole asset came back. A `401` or `404` is a broken setup, not an answer —
the probe's `expect` messages say so rather than recording it as a result.

**Then:** update `docs/sync-format.md` §7's CAL-1 entry with the date, status,
`Content-Range` and byte count. If `Range:` is honoured and you decide to raise
`PACK_TARGET`, raise `PACK_MAX` with it and rewrite the doc comment that
justifies both — a stale reason beside a new number misleads every later reader.

### `permissions.admin` — is it the token's grant or the user's role?

One read-only `GET`. Creates nothing, needs no throwaway anything. Run it with
the PAT and repository already paired in `sync setup`.

```bash
GSD_PERM_TOKEN=<your sync PAT> \
GSD_PERM_REPO=<owner>/<name> \
  cargo test --test live -- --ignored --nocapture \
    permissions_shape_for_a_fine_grained_contents_token
```

**The token shape is part of the question.** It must be the shape
`docs/sync-github.md` prescribes: fine-grained, `Contents: Read and write`,
`Metadata: Read`, **no Administration**, on a repository you own. A classic PAT,
an org-owned repository, or a token with Administration granted each move the
field for their own reasons, and a reading from one of those answers a different
question.

Read `admin` in the printed `permissions` object:

| Result | What it means | What to do |
|---|---|---|
| `admin: true` | The field reflects *your role on the repository*, not the token's grant. It cannot detect an over-permissioned token. | Close D-03's runtime warning with that reason in `docs/sync-format.md` §7. Plan 3-04 was right to ship nothing. |
| `admin: false` | The field narrows to the token's grant. | The warning is one line in `sync::github::gate::assert_pushable`'s warning list — a follow-up, in a plan that owns that file. |
| object absent (`null`) | GitHub omits `permissions` for this token class. | Same conclusion as `true`: nothing to warn on. Record which it was. |

---

## Both probes' skip discipline

Verified by running them with every variable unset:

```
cal1_range_on_private_release_asset: GSD_CAL1_TOKEN is unset — skipping; the 32 MiB
  pack fallback recorded in docs/sync-format.md stands
permissions_shape_for_a_fine_grained_contents_token: GSD_PERM_TOKEN and GSD_PERM_REPO
  (owner/name) must both be set — skipping; D-03's runtime warning stays unshipped and
  docs/sync-github.md's token recipe stays its sole enforcement
test result: ok. 2 passed; 0 failed
```

Plain `cargo test --test live` reports both as `ignored`. The AUR `check()` runs
neither, opens no socket, and reads no `$HOME`.

Both follow the same secret discipline Phase 1's CAL-1 probe set:

- **Redirects are never followed** (`redirect::Policy::none()`). CAL-1 re-issues
  the storage request by hand **without** the `Authorization` header. The
  permissions probe treats a `301` — a renamed repository — as a broken probe
  and asserts rather than following it, so the bearer token is never replayed to
  whatever a `Location` header names. (T-3-30)
- **Only the storage *host* is printed**, never the signed URL, whose query
  string is itself a credential. (T-3-31)
- The permissions probe prints the `permissions` object and four named fields
  and **nothing else** from the response, which otherwise carries owner and
  repository metadata with no business in a terminal transcript. The token is
  never printed. (T-3-33b)

---

## What changed about CAL-1's *meaning*, and why the docs no longer call it a blocker

CAL-1 was written when pack sizing looked like it might depend on the answer. It
does not, and it is worth being precise about why, so nobody re-raises it as a
gate:

- **Phase 5 already committed to whole-pack fetch**, on the pessimistic
  assumption. That is the correct design whichever way `Range:` goes.
- **`PACK_MAX`'s 48 MiB sits under `download_asset`'s 64 MiB body cap**
  (`5-02-PLAN.md`), so a whole pack fits in one bounded, buffered download — no
  streaming verb, no `reqwest` `stream` feature, no new crate.

So CAL-1 is now an **optimisation** question: *can a restore fetch only the
chunks it needs out of a pack?* If yes, packs could also grow past 32 MiB
without making the waste worse. That is a performance question for whoever wants
partial restore, and it can be asked at any time. `docs/sync-format.md` §7 says
this in those terms; `PACK_TARGET`'s doc comment now says the same thing rather
than the old "phase 3 may raise this if `Range:` works", which read as a pending
gate.

## Deviation: `docs/sync-github.md` said the tool warns, and it does not

Step 7 of the token recipe claimed *"The tool will warn you if it detects a token
with Administration permissions."* Plan 3-04 decided that warning does not ship,
which left the shipped documentation promising a safety net that does not exist —
the worst version of this problem, since a user could reasonably rely on it.

Replaced with what is true: the tool cannot check this, because the endpoint
reports the user's role rather than the token's grant, and leaving the box
unchecked is the whole enforcement. Two sentences, no behaviour change. Not
strictly in this plan's file list, but it is the same claim this plan exists to
adjudicate, and leaving it standing would have contradicted the entry added to
`docs/sync-format.md` in the same commit.

If the permissions probe later returns `admin: false` and the warning ships,
that sentence goes back.

## Deviation: the CAL-1 user-agent constant was renamed

`CAL1_UA` → `GITHUB_PROBE_UA` (value `"ai-usagebar-probe"`). The plan says to
reuse CAL-1's user-agent constant rather than add one, which is what happened; a
second probe using a constant named `CAL1_UA` would have made a later reader
stop and check. Three call sites, no behaviour change.

## Verification run

Scoped to what this plan touches, as instructed — no full build, no `make test`.

| Check | Result |
|---|---|
| `cargo test --lib sync::pack` | 16 passed |
| `cargo test --test live -- --ignored --list \| grep -c cal1_range_on_private_release_asset` | 1 |
| `cargo test --test live -- --ignored --list \| grep -c permissions_shape_for_a_fine_grained_contents_token` | 1 |
| `cargo test --test live` (default) | both reported `ignored`; 15 ignored, 0 run |
| both probes with all variables unset | skip with printed message, exit 0 |
| `cargo clippy --all-targets -- -D warnings` | clean |
| `cargo fmt -- --check` | clean |
| new crates | zero |

## Files touched

- `tests/live.rs` — new probe, module header, UA constant rename
- `docs/sync-format.md` — §4 sizes paragraph, §7 CAL-1 entry, §7 permissions entry
- `docs/sync-calibration.md` — the "where the other two live" cross-reference
- `docs/sync-github.md` — one sentence in step 7 of the token recipe
- `src/sync/pack.rs` — `PACK_TARGET`'s doc comment

Not touched: `src/sync/github/setup.rs`, `report.rs`, `cli.rs` (plan 3-07's),
`src/sync/github/gate.rs` (plan 3-04's — the warning stays a follow-up),
`.planning/STATE.md`, `ROADMAP.md`, `REQUIREMENTS.md`.

## For Phase 4's planner

Size packs on 32 MiB. It is a fallback, not a measurement, and it is not
blocking anything. Do not re-derive the question — §7 has it.

## For whoever picks up D-03

Run the invocation above before writing any warning. If `admin` is `true`, the
answer is "close it", and the reason is already written down.
