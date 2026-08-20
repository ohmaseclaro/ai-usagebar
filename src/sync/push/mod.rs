//! The outbound half of the sync bundle: packing, uploading, and the one
//! compare-and-swap that publishes a snapshot.
//!
//! **Every type that crosses a module boundary is declared here**, never in a
//! sibling. Five plans fill one file each in parallel worktrees, and a type
//! declared in `packer.rs` and consumed by `upload.rs` would leave two of them
//! unable to compile.
//!
//! Layout — one file per plan:
//! - [`packer`] — `SyncPlan` to packs, manifest, index object and root (4-02).
//! - [`upload`] — the resume scan, the concurrent uploads, the verification (4-03).
//! - [`pointer`] — the pointer read and the compare-and-swap write (4-01 / 4-04).
//! - [`prune`] — retention and the delete pass (4-05).
//! - [`rekey`] — the password change (4-06).
//! - [`progress`] — the reporting seam (4-01 / 4-03).
//!
//! # D3 — the flip is the only commit point
//!
//! Packs are content-addressed and immutable, so a half-finished upload set is
//! inert: nothing references it. The pointer `PUT` with its `sha` precondition is
//! the single instant at which a snapshot becomes visible, and it is the last
//! thing [`run`] does before pruning. An interruption anywhere above leaves the
//! remote byte-identical to what it was (SYNC-04).
//!
//! # The gate is re-earned here, twice
//!
//! [`run`] calls `fetch_facts` + `assert_pushable` + `PushClearance::spend`
//! itself, once before the first byte and again before the flip. A clearance
//! obtained during `sync setup` is never accepted and cannot be: `sync setup`
//! hands one out no longer, and `spend` consumes the clearance while applying
//! `MAX_CLEARANCE_AGE`. A repository can be flipped public from the web UI
//! between the two checks, which is exactly what the second one is for.

pub mod packer;
pub mod pointer;
pub mod progress;
pub mod prune;
pub mod rekey;
pub mod upload;

use base64::Engine;
use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};

use crate::config::{SyncCategory, SyncConfig};
use crate::error::{AppError, Result};
use crate::sync::SyncRoots;
use crate::sync::crypto::{ChunkId, KdfParams, Keys};
use crate::sync::github::{Client, RepoRef, gate, pairing};
use crate::sync::index::Index;

use progress::Progress;

const B64: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::STANDARD;

/// Where the snapshot pointer lives, through the Contents API.
pub const POINTER_PATH: &str = "sync/pointer.json";

/// The one release every asset hangs off. One release, one fixed tag, created
/// once — never a draft, because a draft has no git tag and
/// `GET /releases/tags/{tag}` cannot find it.
pub const RELEASE_TAG: &str = "ai-usagebar-sync-v1";

/// Pointer version written by this build.
pub const POINTER_VERSION: u32 = 1;
/// Highest pointer version this build can read. At-or-below, like every other
/// versioned object in this format — see [`crate::sync::check_version`].
pub const MAX_SUPPORTED_POINTER: u32 = 1;

/// **No asset younger than this is ever deleted.**
///
/// It lives here rather than in [`prune`] because it is the load-bearing half of
/// a safety property two plans reason about, and neither half is sufficient
/// alone.
///
/// Prune runs only after a successful flip and only against the pointer that
/// *landed*, which covers a competing machine that has already committed: its
/// snapshot records are in that pointer, so its packs are live. It says nothing
/// about a competitor **mid-push**: machine 2 uploads pack `P` and has not
/// flipped; machine 1 commits, prunes, sees `P` referenced by no snapshot,
/// deletes it; machine 2 then flips a pointer naming `P`. That is a live
/// snapshot pointing at deleted data — D2's single worst outcome — reached with
/// neither machine doing anything wrong.
///
/// The age floor is what closes it, and it is why `write::Asset` carries
/// `created_at`. The cost is that genuine garbage lingers a day; the alternative
/// is an unrestorable backup.
pub const PRUNE_GRACE: TimeDelta = TimeDelta::hours(24);

// ---------------------------------------------------------------------------
// Naming — pure functions of a content address
// ---------------------------------------------------------------------------

/// `pack-<64 hex>.bin`.
///
/// Content-addressed, which is what makes D4's "already uploaded" a question
/// with an exact answer rather than a heuristic: a changed pack gets a different
/// name, so no local record of a previous run is needed.
///
/// [`pack::shard_path`](crate::sync::pack::shard_path)'s two-level directory
/// layout is for a filesystem store; a release asset name cannot contain a path
/// separator, so the flat form is used here. Both address the same bytes.
pub fn pack_asset_name(id: &ChunkId) -> String {
    format!("pack-{id}.bin")
}

/// `keyfile-<64 hex>.json`.
///
/// Content-addressed for the same reason, plus one specific to rekey: a
/// rewrapped keyfile has different bytes and therefore a different name, so the
/// new and old assets coexist for the instant between the upload and the delete.
/// A fixed name would mean overwriting the only copy of the wrapped master key.
pub fn keyfile_asset_name(id: &ChunkId) -> String {
    format!("keyfile-{id}.json")
}

/// The bundle identifier bound into every snapshot root as associated data.
///
/// Reads the **pairing record's** numeric repository id, never an id in a
/// response being processed: §5 of the format is explicit that a reader binds
/// its own identifier, so a repository swap fails the Poly1305 tag rather than
/// being caught by comparing the remote's claim against itself.
///
/// It can never be empty, which matters because
/// [`Root::seal`](crate::sync::model::Root::seal) refuses an empty `repo_id` and
/// the refusal would otherwise surface as a confusing failure at the end of a
/// long push.
pub fn repo_id_for(pairing_repo_id: u64) -> String {
    format!("github:{pairing_repo_id}")
}

// ---------------------------------------------------------------------------
// The remote layout
// ---------------------------------------------------------------------------

/// One finished, not-yet-uploaded pack. `id` is
/// [`content_address`](crate::sync::crypto::content_address) of `bytes`.
#[derive(Debug, Clone)]
pub struct BuiltPack {
    pub id: ChunkId,
    pub bytes: Vec<u8>,
}

/// Where one chunk of the **index object itself** lives.
///
/// This is the bootstrap. A reader holding only the pointer needs to know where
/// the index object's own chunks are, or it can never resolve anything else —
/// nothing describes itself. It carries the same five fields as
/// [`IndexEntry`](crate::sync::model::IndexEntry) and is a separate type because
/// it is written in the clear, which `IndexEntry` is not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteIndexEntry {
    pub id: ChunkId,
    pub pack: ChunkId,
    pub offset: u64,
    pub clen: u32,
    pub true_len: u32,
}

/// One published snapshot, as the pointer describes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotRecord {
    /// base64 of the sealed snapshot root.
    pub root: String,
    /// Where the index object's own chunks live.
    pub index_chunks: Vec<RemoteIndexEntry>,
    /// **Every** pack this snapshot needs, reused ones included. That is what
    /// makes prune computable from the pointer alone — no download, no key.
    pub packs: Vec<ChunkId>,
}

/// The one mutable remote object, and the format's single linearization point.
///
/// # Why this container is plaintext, and why that is not a regression
///
/// It **introduces no new kind of object sealed under `chunk_key`**, so Phase 1's
/// deferred AAD object-type separator stays untriggered. Every element of value
/// inside it is already sealed — the root under `root_key`, with the reader's own
/// `repo_id` bound as associated data.
///
/// Tampering therefore dead-ends rather than opening anything:
/// - dropping entries is a rollback, which the local anchor's counter catches;
/// - reordering is inert, because a reader selects by the `counter` inside each
///   sealed root, not by position;
/// - adding a fabricated entry fails the Poly1305 tag.
///
/// Deliberately **not** carried in the clear: `counter` and `created_at`. They
/// would be redundant leakage — a reader opens at most `keep_snapshots` small
/// roots to find the newest, and prune needs neither.
///
/// If a later phase finds itself wanting to *seal* this container, that is the
/// trigger for the object-type separator, and it must be raised loudly rather
/// than done quietly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pointer {
    pub format: u32,
    /// `github:<numeric repo id>` — see [`repo_id_for`].
    pub repo_id: String,
    /// The asset name of the keyfile this bundle's readers must fetch.
    pub keyfile: String,
    /// Oldest first, newest last, at most `keep_snapshots` long.
    pub snapshots: Vec<SnapshotRecord>,
}

/// Everything one push has to put on the wire, produced by [`packer::build`].
#[derive(Debug)]
pub struct PushBundle {
    pub packs: Vec<BuiltPack>,
    /// The sealed snapshot root, framed.
    pub root: Vec<u8>,
    pub index_chunks: Vec<RemoteIndexEntry>,
    /// Every pack the snapshot references, this run's and earlier runs' alike.
    pub referenced_packs: Vec<ChunkId>,
    pub counter: u64,
}

/// What the orchestrator threads through five modules.
///
/// A struct rather than nine positional arguments: a long argument list threaded
/// through five files owned by five plans is how signatures drift.
///
/// No `Debug`: it holds [`Keys`], and a derived one would put key material one
/// `{:?}` away from a log line. `Keys`'s own `Debug` redacts, but `Index` has
/// none at all and adding one to reach this derive would be the tail wagging.
pub struct PushCtx<'a> {
    pub client: &'a Client,
    pub repo: &'a RepoRef,
    pub cfg: &'a SyncConfig,
    pub roots: &'a SyncRoots,
    pub keys: &'a Keys,
    /// The KDF parameters recorded in *this bundle's* keyfile, which every new
    /// snapshot root repeats. Carried here so nothing under `push/` has to
    /// re-read the keyfile off disk to build a root.
    pub kdf: KdfParams,
    pub index: &'a Index,
    pub repo_id: String,
    /// The keyfile asset name this machine's local keyfile would publish under.
    /// Only used on a first push; afterwards the arriving pointer's value wins.
    pub keyfile_asset: String,
    /// The pointer currently on the remote, or `None` on a first push.
    ///
    /// Filled by [`run`] after `pointer::load`, **not** by the caller: a CLI that
    /// populated it would have had to make a request before the gate. Callers
    /// construct the context with `None`.
    pub previous: Option<Pointer>,
    pub now: DateTime<Utc>,
}

/// What a push did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct PushOutcome {
    pub packs_uploaded: usize,
    pub packs_skipped: usize,
    pub bytes_uploaded: u64,
    pub snapshots_kept: usize,
    pub packs_deleted: usize,
    /// **An `Option`, never an `Err`.** D2 is explicit that a prune failure is a
    /// warning and never a push failure: the push already succeeded and the
    /// user's data is safe, and leaving a few stale packs costs storage, not
    /// correctness. Encoding that in the type means no later plan can
    /// accidentally make it fatal.
    pub prune_warning: Option<String>,
}

// ---------------------------------------------------------------------------
// The orchestrator
// ---------------------------------------------------------------------------

/// One push, end to end.
///
/// The order below is D3 and is not negotiable. `ctx` is taken by value because
/// step 2 fills [`PushCtx::previous`] from the remote before the packer runs.
pub async fn run(mut ctx: PushCtx<'_>, progress: &mut dyn Progress) -> Result<PushOutcome> {
    let credentials_in_bundle = ctx.cfg.includes(SyncCategory::Credentials);

    // 1. The gate, run *here*, inside the push. Never a clearance carried from
    //    `sync setup`: a repository can be flipped public from the web UI in
    //    between, and `spend` is what turns "immediately" into arithmetic.
    let permit = gate_now(&ctx, credentials_in_bundle).await?;

    // 2. The pointer, before the packer, because the snapshot counter is one
    //    above the newest published snapshot's.
    let (previous, sha) = pointer::load(ctx.client, ctx.repo, &ctx.repo_id, ctx.now).await?;
    ctx.previous = previous.clone();

    // 3. Plan, then pack. Both are local and neither touches the network.
    let plan =
        crate::sync::plan::build_with_keys(ctx.roots, ctx.cfg, ctx.index, ctx.now, ctx.keys)?;
    let bundle = packer::build(&ctx, &plan)?;

    // 4. The release the assets hang off.
    let release_id = ctx
        .client
        .ensure_release(ctx.repo, RELEASE_TAG, &permit, ctx.now)
        .await?;

    // 5. Upload, and verify each asset is retrievable — D3's precondition for
    //    the flip. Nothing here is referenced by anything yet.
    let (packs_uploaded, packs_skipped, bytes_uploaded) =
        upload::run(&ctx, release_id, &bundle.packs, &permit, progress).await?;

    // 6. Re-gate. A public read *here* is an incident, not a refusal.
    let permit = match gate_now(&ctx, credentials_in_bundle).await {
        Ok(permit) => permit,
        Err(why) => {
            return Err(went_public_mid_push(&ctx, release_id, &bundle, &permit, why).await);
        }
    };

    // 7. **The only commit point.** Everything above is inert without it.
    let record = SnapshotRecord {
        root: B64.encode(&bundle.root),
        index_chunks: bundle.index_chunks.clone(),
        packs: bundle.referenced_packs.clone(),
    };
    let keep = ctx.cfg.keep_snapshots as usize;
    let repo_id = ctx.repo_id.clone();
    let local_keyfile = ctx.keyfile_asset.clone();

    // The rebuild closure. Three rules, each with a failure mode that destroys
    // data rather than erroring — plan 4-04 calls this a second time with a
    // competitor's pointer after a 409, and drives its own tests with a closure
    // reproducing all three, so breaking one here fails there.
    let rebuild = move |arriving: Option<&Pointer>| -> Result<Pointer> {
        // Rule 1: carry forward every record this run did not produce. Dropping
        // a competitor's record makes its packs unreferenced, which makes the
        // next prune delete them, which strands its backup.
        let mut snapshots = arriving.map(|p| p.snapshots.clone()).unwrap_or_default();
        // …and append rather than duplicate: `rebuild` runs again on a conflict,
        // and the arriving pointer may already carry this run's own record.
        snapshots.retain(|existing| existing.root != record.root);
        snapshots.push(record.clone());

        // Rule 2: truncate from the **oldest** end only. Doing it inside the
        // pointer being written is what makes D2's mandatory order structural —
        // the snapshot record is dropped by the flip itself, strictly before
        // step 8 deletes anything.
        if snapshots.len() > keep {
            snapshots.drain(..snapshots.len() - keep);
        }

        // Rule 3: the keyfile name comes from the pointer that arrived, not from
        // local state, unless this run is the one changing it — and only `rekey`
        // is. Republishing a stale name points every future reader at an asset a
        // rekey has already deleted.
        let keyfile = arriving
            .map(|p| p.keyfile.clone())
            .unwrap_or_else(|| local_keyfile.clone());

        Ok(Pointer {
            format: POINTER_VERSION,
            repo_id: repo_id.clone(),
            keyfile,
            snapshots,
        })
    };

    let (landed, _new_sha) = pointer::commit(
        ctx.client,
        ctx.repo,
        previous.as_ref(),
        sha.as_deref(),
        rebuild,
        &permit,
        ctx.now,
    )
    .await?;

    // 8. Prune, against the pointer that actually **landed** — never the one
    //    this run built. If another machine won the flip, `landed` is *its*
    //    pointer and its packs are consequently live.
    let mut outcome = PushOutcome {
        packs_uploaded,
        packs_skipped,
        bytes_uploaded,
        snapshots_kept: landed.snapshots.len(),
        ..PushOutcome::default()
    };
    match prune::run(&ctx, release_id, &landed, keep, &permit).await {
        Ok(deleted) => outcome.packs_deleted = deleted,
        // D2 in the type: a prune failure is a warning, never a returned `Err`.
        Err(e) => outcome.prune_warning = Some(e.to_string()),
    }
    Ok(outcome)
}

/// `fetch_facts` → `check_drift` → `assert_pushable` → `spend`, in that order.
///
/// The whole of the gate, in one place, so the pre-upload check and the
/// pre-flip check cannot drift apart. The returned [`gate::Pushing`] is the only
/// thing that can reach a write verb.
pub(crate) async fn gate_now(
    ctx: &PushCtx<'_>,
    credentials_in_bundle: bool,
) -> Result<gate::Pushing> {
    let facts = gate::fetch_facts(ctx.client, ctx.repo, ctx.now).await?;
    let record = pairing::read_from(&pairing::default_path(ctx.roots))?;
    pairing::check_drift(record.as_ref(), &facts, credentials_in_bundle, ctx.now)?;
    let (clearance, _warnings) =
        gate::assert_pushable(&facts, ctx.repo, credentials_in_bundle, ctx.now)?;
    clearance.spend(ctx.now)
}

/// The re-gate said the repository is readable. Delete what this run uploaded,
/// refuse the flip, and say what cannot be undone.
///
/// The assets removed are exactly those that (a) carry a name this run's bundle
/// produced and (b) were created at or after this run's clock — the intersection,
/// because a pack this run *skipped* was uploaded by an earlier run and may be
/// referenced by a live snapshot. Deleting one of those would be the very
/// outcome `PRUNE_GRACE` exists to prevent, arriving through the incident path.
///
/// Nothing is flipped and nothing is pruned. The old pointer, and every snapshot
/// it names, is exactly as it was.
async fn went_public_mid_push(
    ctx: &PushCtx<'_>,
    release_id: u64,
    bundle: &PushBundle,
    permit: &gate::Pushing,
    why: AppError,
) -> AppError {
    let ours: Vec<String> = bundle
        .packs
        .iter()
        .map(|p| pack_asset_name(&p.id))
        .collect();
    let mut removed = 0usize;
    let mut failed: Vec<String> = Vec::new();

    match ctx.client.list_assets(ctx.repo, release_id, ctx.now).await {
        Ok(assets) => {
            for asset in assets
                .iter()
                .filter(|a| ours.contains(&a.name) && a.created_at >= ctx.now)
            {
                match ctx
                    .client
                    .delete_asset(ctx.repo, asset.id, permit, ctx.now)
                    .await
                {
                    Ok(()) => removed += 1,
                    Err(e) => failed.push(format!("{}: {e}", asset.name)),
                }
            }
        }
        Err(e) => failed.push(format!("the asset listing could not be read: {e}")),
    }

    let cleanup = if failed.is_empty() {
        format!("All {removed} asset(s) this run uploaded were deleted.")
    } else {
        format!(
            "{removed} asset(s) were deleted, but {} could not be: {}. Delete them by hand \
             from the release's assets on GitHub.",
            failed.len(),
            failed.join("; ")
        )
    };

    AppError::Other(format!(
        "STOP — the backup repository read as private before this push started and reads as \
         readable now. The snapshot pointer was NOT updated: nothing this run uploaded is \
         referenced by anything, and the previous snapshot is untouched.\n\
         {cleanup}\n\
         Encrypted pack bytes may have been visible for the duration of the upload. They are \
         useless without the sync password, but bytes that were published cannot be \
         un-published — making the repository private again does not undo a read that already \
         happened.\n\
         1. Make the repository private again, in its GitHub settings.\n\
         2. Rotate the sync token at https://github.com/settings/personal-access-tokens.\n\
         3. If you are not certain the packs stayed unread, change the sync password with \
         `ai-usagebar sync rekey` — and note that this is not revocation: anyone holding a \
         copy of the old keyfile can still open it with the old password.\n\
         The gate said: {why}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(byte: u8) -> ChunkId {
        ChunkId::from_bytes([byte; 32])
    }

    #[test]
    fn the_two_asset_names_are_pure_functions_of_a_content_address() {
        let pack = pack_asset_name(&id(0xab));
        let keyfile = keyfile_asset_name(&id(0xab));
        assert_eq!(pack, format!("pack-{}.bin", "ab".repeat(32)));
        assert_eq!(keyfile, format!("keyfile-{}.json", "ab".repeat(32)));
        assert_eq!(pack, pack_asset_name(&id(0xab)), "pure");
        assert_ne!(pack, pack_asset_name(&id(0xac)));
        // Percent-safe by construction, which `upload_asset` relies on.
        for name in [&pack, &keyfile] {
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')),
                "{name}"
            );
        }
    }

    /// `Root::seal` refuses an empty `repo_id`, and the refusal would otherwise
    /// surface at the end of a long push.
    #[test]
    fn a_bundle_identifier_is_never_empty_and_names_its_host() {
        for pairing_repo_id in [0, 1, u64::MAX] {
            let id = repo_id_for(pairing_repo_id);
            assert!(!id.is_empty());
            assert!(id.starts_with("github:"), "{id}");
        }
    }

    #[test]
    fn a_pointer_round_trips_with_its_snapshot_list_intact() {
        let pointer = Pointer {
            format: POINTER_VERSION,
            repo_id: repo_id_for(42),
            keyfile: keyfile_asset_name(&id(1)),
            snapshots: vec![SnapshotRecord {
                root: B64.encode(b"sealed root bytes"),
                index_chunks: vec![RemoteIndexEntry {
                    id: id(2),
                    pack: id(3),
                    offset: 17,
                    clen: 4112,
                    true_len: 4096,
                }],
                packs: vec![id(3), id(4)],
            }],
        };
        let json = serde_json::to_vec(&pointer).unwrap();
        assert_eq!(serde_json::from_slice::<Pointer>(&json).unwrap(), pointer);
    }

    /// The grace window is the only thing standing between prune and another
    /// machine's in-flight upload, so its value is pinned rather than assumed.
    #[test]
    fn the_prune_grace_is_a_full_day() {
        assert_eq!(PRUNE_GRACE.num_hours(), 24);
    }
}
