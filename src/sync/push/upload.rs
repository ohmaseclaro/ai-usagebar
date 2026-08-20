//! Pack assets on to the release.
//!
//! Plan 4-01 created this file with the straight-line case: upload each pack in
//! turn and verify each one. **Plan 4-03 owns it** and adds the resume scan (D4:
//! skip a pack already present at a matching size and state, delete a torn one
//! first), the four-at-a-time `JoinSet`, and the per-asset progress calls.
//!
//! Nothing in this file prints. Rendering belongs to the [`Progress`]
//! implementations, which keeps the uploader testable with
//! [`Silent`](super::progress::Silent).

use crate::error::{AppError, Result};
use crate::sync::crypto::content_address;
use crate::sync::github::gate;

use super::progress::Progress;
use super::{BuiltPack, PushCtx, pack_asset_name};

/// Upload `packs`, returning `(uploaded, skipped, bytes_uploaded)`.
///
/// **Verification is D3's precondition for the flip**, not a nicety: every asset
/// this run uploaded is fetched back and its content address compared against
/// the pack's id. A mismatch fails this function, so the orchestrator never
/// reaches the pointer `PUT` and no pointer can reference a pack that did not
/// verify.
///
/// Say plainly what that check is and is not: a corrupt pack would in any case
/// fail its per-blob Poly1305 tags on read, so this catches transport and
/// packaging bugs, not attacks. It costs one extra download of newly-uploaded
/// data — on a 115 MB first push, 115 MB — and that is the price D3 sets.
///
/// `permit` is the gate's, minted inside the push. It authorises every write
/// here; the flip gets a second one, minted by the re-gate afterwards.
pub async fn run(
    ctx: &PushCtx<'_>,
    release_id: u64,
    packs: &[BuiltPack],
    permit: &gate::Pushing,
    progress: &mut dyn Progress,
) -> Result<(usize, usize, u64)> {
    let total_bytes: u64 = packs.iter().map(|p| p.bytes.len() as u64).sum();
    progress.start(packs.len(), total_bytes);

    let mut uploaded = 0usize;
    let mut bytes = 0u64;
    for (index, pack) in packs.iter().enumerate() {
        let name = pack_asset_name(&pack.id);
        let asset = ctx
            .client
            .upload_asset(
                ctx.repo,
                release_id,
                &name,
                pack.bytes.clone(),
                permit,
                ctx.now,
            )
            .await?;

        let fetched = ctx
            .client
            .download_asset(ctx.repo, asset.id, ctx.now)
            .await?;
        if content_address(&fetched) != pack.id {
            return Err(AppError::Other(format!(
                "the pack uploaded as {name} does not read back as the bytes that were sent. \
                 Nothing was published — the snapshot pointer is untouched — and re-running \
                 the command re-uploads it. If this repeats, report it at \
                 https://github.com/akitaonrails/ai-usagebar/issues."
            )));
        }

        uploaded += 1;
        bytes += pack.bytes.len() as u64;
        progress.asset_done(index, &name, pack.bytes.len() as u64);
    }

    progress.finish();
    Ok((uploaded, 0, bytes))
}
