//! The read chain: pointer → keyfile → snapshot root → index object →
//! manifest → packs.
//!
//! Everything this module touches comes off a remote the format treats as
//! hostile, and only the pointer is unauthenticated. So the order matters and
//! is worth stating: the pointer says *where* to look, the keyfile unwraps the
//! master key, and from the snapshot root onward every byte is under an AEAD
//! tag before it is parsed. Nothing the pointer claims about lengths, offsets
//! or ordering is believed — the pointer is trusted only to name a pack asset
//! and a chunk id, and the pack's own sealed header says where that chunk
//! really sits (see [`PackSource`]).
//!
//! # Restore cannot write to the remote
//!
//! There is no request-body call site in this file and no path that creates a
//! release: `Client::get_json`, `Client::list_assets`, `Client::get_contents`
//! (through `push::pointer::load`) and `Client::download_asset` are the four
//! verbs used, and not one of them takes a `gate::Pushing`. A missing release
//! is "nothing has been pushed yet", never a reason to make one.
//!
//! # Whole packs, not ranges
//!
//! CAL-1 — whether a `Range:` request against a private release asset is
//! honoured — was never run, so the recorded fallback stands and a restore
//! fetches whole packs. `download_asset` caps at 64 MiB, comfortably above
//! [`PACK_MAX`], so no streaming verb is needed either.
//!
//! Plan 5-01 filled the chain; plan 5-02 owns hardening it.

use std::collections::HashMap;

use base64::Engine;
use serde::Deserialize;

use crate::error::{AppError, Result};
use crate::sync::anchor::{self, Anchor};
use crate::sync::crypto::{ChunkId, Keyfile};
use crate::sync::github::write::{ASSET_STATE_UPLOADED, MAX_ASSET_BYTES};
use crate::sync::model::{IndexObject, Manifest, Root};
use crate::sync::pack::PACK_MAX;
use crate::sync::push::{self, RELEASE_TAG, SnapshotRecord};

use super::{PackSource, Resolved, RestoreCtx};

const B64: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::STANDARD;

/// Every list inside the plaintext pointer is a remote-chosen length, so each
/// one is bounded before it is walked. The numbers are the format's own
/// defaults plus generous headroom, not guesses at what is reasonable.
///
/// `keep_snapshots` defaults to a handful and a monthly retention tail is a
/// dozen more; 256 is slack rather than a limit.
const MAX_SNAPSHOTS_IN_POINTER: usize = 256;
/// One `RemoteIndexEntry` per chunk of the index object. An index object is a
/// sealed JSON document chunked at `CHUNK_SIZE`, so this bounds it at gigabytes
/// of description — far past anything a real bundle produces.
const MAX_INDEX_CHUNKS: usize = 8192;
/// The same reasoning for the ordered manifest chunk list inside the root.
const MAX_MANIFEST_CHUNKS: usize = 8192;
/// Packs one snapshot may reference. At `PACK_MAX` each, this is the ceiling on
/// what a single restore can be made to download.
const MAX_PACKS_PER_SNAPSHOT: usize = 4096;
/// A sealed root is one chunk plus framing; base64 inflates by 4/3. Bounded
/// before the decode allocates.
const MAX_ROOT_B64: usize = 2 * crate::sync::CHUNK_SIZE;

/// The only field of a release this side reads.
#[derive(Deserialize)]
struct ReleaseRef {
    id: u64,
}

/// One release asset, reduced to what a download needs.
#[derive(Clone, Copy)]
struct AssetRef {
    id: u64,
    size: u64,
}

/// Walk the chain and return everything authenticated.
///
/// `local_anchor` arrives as an argument rather than being read here, because
/// the path it came from must not be derived from the remote's claimed
/// `repo_id` — that is [`anchor`]'s stated constraint, and `restore::run` step 1
/// is what honours it.
pub async fn resolve(ctx: &RestoreCtx<'_>, local_anchor: Option<&Anchor>) -> Result<Resolved> {
    // 1. The pointer. `push::pointer::load` already probes `format` before
    //    deserializing and refuses a `repo_id` that is not this machine's own;
    //    a second copy of either check here would be one more thing to diverge.
    let (pointer, _sha) = push::pointer::load(ctx.client, ctx.repo, ctx.repo_id, ctx.now).await?;
    let Some(pointer) = pointer else {
        return Err(AppError::Other(
            "this repository has no snapshot pointer, so nothing has been pushed to it yet — \
             run `ai-usagebar sync push` on the machine that has the data"
                .into(),
        ));
    };
    if pointer.snapshots.len() > MAX_SNAPSHOTS_IN_POINTER {
        return Err(AppError::Other(format!(
            "the snapshot pointer lists {} snapshots, past the {MAX_SNAPSHOTS_IN_POINTER} this \
             build will walk — refusing rather than following an unbounded list",
            pointer.snapshots.len()
        )));
    }
    if pointer.snapshots.is_empty() {
        return Err(AppError::Other(
            "the snapshot pointer names no snapshots, so there is nothing to restore".into(),
        ));
    }

    // 2. The release, read-only. A missing one is "nothing pushed yet".
    let release_id = find_release(ctx).await?.ok_or_else(|| {
        AppError::Other(format!(
            "this repository has no `{RELEASE_TAG}` release, so the bundle's data is not there \
             even though a pointer is — nothing was restored"
        ))
    })?;

    // 3. One listing for the whole restore: the keyfile and every pack are
    //    looked up in it by name.
    let assets = asset_index(ctx, release_id).await?;

    // 4. The keyfile named by the pointer. Its failure to open is deliberately
    //    the same error a wrong password gives — elaborating it would build the
    //    oracle Phase 1 refused to build.
    let keyfile_bytes = download(ctx, &assets, &pointer.keyfile, "keyfile").await?;
    let keyfile: Keyfile = serde_json::from_slice(&keyfile_bytes).map_err(|_| {
        AppError::Other(format!(
            "the keyfile asset {:?} is not a readable sync keyfile — this bundle cannot be \
             opened",
            pointer.keyfile
        ))
    })?;
    let keys = keyfile.open(ctx.passphrase.as_bytes())?;

    // 5. Every snapshot root, opened under *this machine's* `repo_id`. It is
    //    bound into the root's associated data, so a root belonging to another
    //    bundle fails the Poly1305 tag before a field is parsed — do not
    //    "harden" this with a string comparison and think anything was added.
    let mut newest: Option<(Root, &SnapshotRecord)> = None;
    for record in &pointer.snapshots {
        if record.root.len() > MAX_ROOT_B64 {
            return Err(AppError::Other(
                "a snapshot record carries more base64 than a sealed root can be".into(),
            ));
        }
        let Ok(framed) = B64.decode(&record.root) else {
            continue;
        };
        // A root this build cannot open is skipped, not fatal: a pointer may
        // carry a record written by a format this build predates, and one such
        // record must not make every older snapshot unrestorable.
        let Ok(root) = Root::open(&keys, &framed, ctx.repo_id) else {
            continue;
        };
        if newest
            .as_ref()
            .is_none_or(|(best, _)| root.counter > best.counter)
        {
            newest = Some((root, record));
        }
    }
    let Some((root, record)) = newest else {
        return Err(AppError::Other(
            "none of the snapshots in this pointer could be opened — either the passphrase is \
             wrong, or the bundle was written by a newer ai-usagebar"
                .into(),
        ));
    };

    // 6. The rollback decision, against the root's **sealed** counter and
    //    `repo_id` — never the plaintext pointer's copies. `accept` only
    //    decides; `restore::run` step 7 persists, and only after this whole
    //    function has returned `Ok`. Advancing earlier would let a forged high
    //    counter lock the user out of their own bundle permanently.
    anchor::accept(
        local_anchor,
        &root.repo_id,
        root.counter,
        ctx.opts.allow_rollback,
    )?;

    if record.index_chunks.len() > MAX_INDEX_CHUNKS {
        return Err(AppError::Other(format!(
            "this snapshot describes its index in {} chunks, past the {MAX_INDEX_CHUNKS} this \
             build will read",
            record.index_chunks.len()
        )));
    }
    if root.manifest_chunks.len() > MAX_MANIFEST_CHUNKS {
        return Err(AppError::Other(format!(
            "this snapshot's manifest spans {} chunks, past the {MAX_MANIFEST_CHUNKS} this \
             build will read",
            root.manifest_chunks.len()
        )));
    }
    if record.packs.len() > MAX_PACKS_PER_SNAPSHOT {
        return Err(AppError::Other(format!(
            "this snapshot references {} packs, past the {MAX_PACKS_PER_SNAPSHOT} this build \
             will fetch",
            record.packs.len()
        )));
    }

    // 7. Round one: the packs holding the index object, which is the plaintext
    //    bootstrap — nothing describes itself.
    let mut packs = PackSource::empty(keys);
    let wanted: Vec<ChunkId> = record.index_chunks.iter().map(|e| e.pack).collect();
    fetch_packs(ctx, &assets, &mut packs, &wanted).await?;

    let index_sealed = sealed_in_order(
        &packs,
        record.index_chunks.iter().map(|e| e.id).collect::<Vec<_>>(),
    )?;
    let index = IndexObject::open(packs.keys(), &index_sealed)?;

    // 8. Round two: the packs holding the manifest, located through the index.
    let manifest_packs = packs_for(&index, &root.manifest_chunks)?;
    fetch_packs(ctx, &assets, &mut packs, &manifest_packs).await?;
    let manifest_sealed = sealed_in_order(&packs, root.manifest_chunks.clone())?;
    let manifest = Manifest::open(packs.keys(), &manifest_sealed)?;

    // 9. Round three: file data — **only** when this run will actually write.
    //    A dry run never pulls a byte of a user's file content, which is what
    //    keeps `PackSource`'s accessors synchronous.
    if ctx.opts.apply {
        let mut needed: Vec<ChunkId> = Vec::new();
        for file in &manifest.files {
            for id in &file.chunks {
                let entry = index.resolve(id).ok_or_else(|| missing_chunk(id))?;
                if !packs.holds_pack(&entry.pack) && !needed.contains(&entry.pack) {
                    needed.push(entry.pack);
                }
            }
        }
        if needed.len() > MAX_PACKS_PER_SNAPSHOT {
            return Err(AppError::Other(
                "this snapshot's files span more packs than this build will fetch".into(),
            ));
        }
        fetch_packs(ctx, &assets, &mut packs, &needed).await?;
    }

    Ok(Resolved {
        root,
        manifest,
        index,
        packs,
    })
}

/// The release id for [`RELEASE_TAG`], or `None` when there is no such release.
///
/// Deliberately **not** `write::ensure_release`: that verb's 404 arm creates a
/// release, and restore must be structurally incapable of changing the remote.
/// `get_json` is the crate's one read verb and takes no push capability.
async fn find_release(ctx: &RestoreCtx<'_>) -> Result<Option<u64>> {
    let path = format!(
        "/repos/{}/{}/releases/tags/{RELEASE_TAG}",
        ctx.repo.owner, ctx.repo.name
    );
    let (status, _headers, body) = ctx.client.get_json(&path).await?;
    if status.as_u16() == 404 {
        return Ok(None);
    }
    if !status.is_success() {
        return Err(AppError::Http {
            status: status.as_u16(),
            body: "could not read the sync release from this repository".into(),
        });
    }
    let release: ReleaseRef = serde_json::from_slice(&body)
        .map_err(|_| AppError::Schema("the sync release listing was not readable".into()))?;
    Ok(Some(release.id))
}

/// One listing, by asset name. A torn upload (any state but `uploaded`) is not
/// offered: its bytes are incomplete by definition.
async fn asset_index(ctx: &RestoreCtx<'_>, release_id: u64) -> Result<HashMap<String, AssetRef>> {
    let listed = ctx
        .client
        .list_assets(ctx.repo, release_id, ctx.now)
        .await?;
    Ok(listed
        .into_iter()
        .filter(|a| a.state == ASSET_STATE_UPLOADED)
        .map(|a| {
            (
                a.name,
                AssetRef {
                    id: a.id,
                    size: a.size,
                },
            )
        })
        .collect())
}

/// Download one named asset, refusing a declared size this build will not hold.
async fn download(
    ctx: &RestoreCtx<'_>,
    assets: &HashMap<String, AssetRef>,
    name: &str,
    what: &str,
) -> Result<Vec<u8>> {
    let asset = assets.get(name).ok_or_else(|| {
        AppError::Other(format!(
            "this bundle's pointer names the {what} asset {name:?}, which is not on the sync \
             release — the bundle cannot be opened. It may have been pruned, or the push that \
             would have uploaded it never finished"
        ))
    })?;
    if asset.size > MAX_ASSET_BYTES {
        return Err(AppError::Other(format!(
            "the {what} asset {name:?} is declared as {} bytes, past what this build will \
             download",
            asset.size
        )));
    }
    ctx.client.download_asset(ctx.repo, asset.id, ctx.now).await
}

/// Fetch every pack in `wanted` that is not already held.
async fn fetch_packs(
    ctx: &RestoreCtx<'_>,
    assets: &HashMap<String, AssetRef>,
    packs: &mut PackSource,
    wanted: &[ChunkId],
) -> Result<()> {
    for id in wanted {
        if packs.holds_pack(id) {
            continue;
        }
        let name = push::pack_asset_name(id);
        if assets.get(&name).is_some_and(|a| a.size > PACK_MAX as u64) {
            return Err(AppError::Other(format!(
                "the pack asset {name:?} is larger than a pack can be — refusing it"
            )));
        }
        let bytes = download(ctx, assets, &name, "pack").await?;
        packs.add(*id, bytes)?;
    }
    Ok(())
}

/// Which packs hold `ids`, according to the (authenticated) index object.
fn packs_for(index: &IndexObject, ids: &[ChunkId]) -> Result<Vec<ChunkId>> {
    let mut out: Vec<ChunkId> = Vec::new();
    for id in ids {
        let entry = index.resolve(id).ok_or_else(|| missing_chunk(id))?;
        if !out.contains(&entry.pack) {
            out.push(entry.pack);
        }
    }
    Ok(out)
}

/// The sealed bytes of `ids`, in the order given — the shape
/// [`Manifest::open`] and [`IndexObject::open`] take. Order is the caller's to
/// get right: a chunk carries no position.
fn sealed_in_order(packs: &PackSource, ids: Vec<ChunkId>) -> Result<Vec<(ChunkId, Vec<u8>)>> {
    ids.into_iter()
        .map(|id| packs.sealed(&id).map(|bytes| (id, bytes)))
        .collect()
}

fn missing_chunk(id: &ChunkId) -> AppError {
    AppError::Other(format!(
        "this snapshot names chunk {id}, which its index does not describe — refusing rather \
         than restoring a partial tree"
    ))
}
