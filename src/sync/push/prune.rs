//! Retention, and the one destructive pass in the crate.
//!
//! Plan 4-01 created this file with the safe minimum — nothing is deleted.
//! **Plan 4-05 owns it** and fills [`plan_deletions`]'s retention rules and the
//! delete pass.
//!
//! # The ordering rule is structural, and must stay that way
//!
//! D2 is absolute that a snapshot *record* is deleted before any pack, because
//! the reverse can leave a live snapshot pointing at a deleted pack — an
//! unrestorable backup, the worst outcome this feature can produce. That
//! ordering is **not a step in this file**: the truncated pointer is published by
//! the flip, and this file's deletion list is acted on only after
//! [`pointer::commit`](super::pointer::commit) has returned. The record is
//! therefore always gone from the remote before the first `DELETE` is issued.
//! Nobody may later "optimise" the delete pass to run in parallel with the flip.
//!
//! Say equally plainly what that ordering does **not** buy. It orders this
//! machine's own record against this machine's own deletes. It says nothing
//! about another machine's snapshot that has not been published yet. That gap
//! belongs to [`PRUNE_GRACE`](super::PRUNE_GRACE), and the two mitigations are
//! not interchangeable.

use chrono::{DateTime, TimeDelta, Utc};

use crate::error::Result;
use crate::sync::github::gate;
use crate::sync::github::write::Asset;

use super::{Pointer, PushCtx};

/// Which snapshot records survive, and which assets may be deleted.
///
/// Pure — `now` and `grace` are parameters rather than reads — so every rule is
/// testable against a table rather than against a server.
///
/// Plan 4-05 fills it. The rules it must implement, in full:
/// - truncate `pointer.snapshots` from the **oldest** end down to `keep`;
/// - the live pack set is the union of `packs` across every **surviving**
///   record;
/// - an asset is deletable when its name parses as a pack name whose id is not
///   in that set **and** `now - asset.created_at > grace`;
/// - the asset named by `pointer.keyfile` is never deletable — no snapshot's
///   `packs` names it, and deleting it makes the bundle permanently unreadable;
/// - an asset matching neither the pack shape nor the keyfile shape is never
///   deletable: it might be a future version's object, and a collector that
///   deletes what it does not recognise turns every format addition into a
///   data-loss bug;
/// - a pointer with **no** snapshots yields no deletions. The union of an empty
///   set is empty, which read naively says "everything is garbage".
pub fn plan_deletions(
    pointer: &Pointer,
    assets: &[Asset],
    keep: usize,
    now: DateTime<Utc>,
    grace: TimeDelta,
) -> (Pointer, Vec<u64>) {
    let _ = (assets, keep, now, grace);
    (pointer.clone(), Vec::new())
}

/// The delete pass after a successful flip.
///
/// `landed` is the pointer [`pointer::commit`](super::pointer::commit) returned
/// — the one that is actually on the remote — and **never** the one this run
/// built. That is the whole mitigation for the race D2 warns about: if another
/// machine won the flip, `landed` is *its* pointer, its snapshot records are in
/// the list, and its packs are consequently live. Deleting a competitor's pack
/// stops being unlikely and becomes impossible.
pub async fn run(
    ctx: &PushCtx<'_>,
    release_id: u64,
    landed: &Pointer,
    keep: usize,
    permit: &gate::Pushing,
) -> Result<usize> {
    let _ = (ctx, release_id, landed, keep, permit);
    Ok(0)
}

/// The `ai-usagebar sync prune` entry point.
///
/// It **starts with the gate**, exactly as the push path does: this issues
/// writes to the remote, and every write path in the crate re-earns its
/// clearance rather than assuming one. A prune is a lower-stakes write than an
/// upload, but "lower stakes" is not a reason for a second rule.
///
/// It takes no release id, because obtaining one means calling `ensure_release`,
/// which is itself a write and therefore already needs the permit this function
/// mints.
///
/// Plan 4-05 fills it: gate, `ensure_release`, load the pointer, publish the
/// truncated one through [`pointer::commit`](super::pointer::commit) — because
/// on-demand pruning must drop records in the same ordered way a push does,
/// through the same compare-and-swap — then run the delete pass against what
/// landed. Unlike the push path, a failure here **is** a failure: the user asked
/// for exactly this.
pub async fn run_on_demand(ctx: &PushCtx<'_>, keep: usize) -> Result<usize> {
    let _ = keep;
    let _permit = super::gate_now(
        ctx,
        ctx.cfg.includes(crate::config::SyncCategory::Credentials),
    )
    .await?;
    Ok(0)
}
