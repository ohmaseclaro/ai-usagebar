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

use std::collections::HashSet;

use chrono::{DateTime, TimeDelta, Utc};

use crate::error::Result;
use crate::sync::crypto::ChunkId;
use crate::sync::github::gate;
use crate::sync::github::write::Asset;

use super::{Pointer, PushCtx};

/// Which snapshot records survive, and which assets may be deleted.
///
/// Pure — `now` and `grace` are parameters rather than reads — so every rule is
/// testable against a table rather than against a server.
///
/// # What survives
///
/// `pointer.snapshots` is truncated from the **oldest** end down to `keep`, and
/// the live pack set is the union of `packs` across every **surviving** record.
/// An asset is deletable only when all three hold:
///
/// 1. its name is one this build recognises — `pack-<64 hex>.bin` or
///    `keyfile-<64 hex>.json`. Anything else might be a future version's object
///    or something the user attached by hand, and a collector that deletes what
///    it does not understand turns every format addition into a data-loss bug;
/// 2. no surviving record names it. For a pack that is its id's absence from the
///    live set; for a keyfile it is not being the one `pointer.keyfile` names;
/// 3. it is older than `grace`.
///
/// Three exclusions are absolute, and each has its own test:
///
/// - **The asset named by `pointer.keyfile`.** No snapshot's `packs` list names
///   it, so the naive rule collects it — and the wrapped master key inside is
///   the only route to the data, for every machine, permanently. This is the
///   single worst thing this function could do.
/// - **Anything younger than `grace`**, whatever the pointer says about it. Not
///   belt and braces: see [`PRUNE_GRACE`](super::PRUNE_GRACE) for the race it is
///   the only cover for.
/// - **A pointer with no surviving snapshots proposes nothing.** The union of an
///   empty set is empty, which read naively says "everything is garbage", and
///   that arithmetic is how a first-push race or a hand-edited pointer would
///   wipe a release.
///
/// `keep` is clamped to at least one. Config refuses `keep_snapshots = 0`, but
/// the truncated pointer this returns is *published* by
/// [`run_on_demand`], so a zero arriving by any other route must not empty the
/// snapshot list.
///
/// Keyfiles other than the published one are swept because an interrupted first
/// push, or a rekey on a bundle with no pointer, can leave a wrapper asset no
/// pointer names — an old password still opens it. Plan 4-06 found that and
/// could not close it in `rekey.rs`, because closing it there meant calling
/// `ensure_release` — which *creates* a release — purely to run a delete.
pub fn plan_deletions(
    pointer: &Pointer,
    assets: &[Asset],
    keep: usize,
    now: DateTime<Utc>,
    grace: TimeDelta,
) -> (Pointer, Vec<u64>) {
    let mut kept = pointer.clone();
    let keep = keep.max(1);
    if kept.snapshots.len() > keep {
        kept.snapshots.drain(..kept.snapshots.len() - keep);
    }
    if kept.snapshots.is_empty() {
        return (kept, Vec::new());
    }

    let live: HashSet<ChunkId> = kept
        .snapshots
        .iter()
        .flat_map(|s| s.packs.iter().copied())
        .collect();
    let doomed = assets
        .iter()
        .filter(|a| now - a.created_at > grace)
        .filter(|a| match asset_kind(&a.name) {
            Some(Kind::Pack(id)) => !live.contains(&id),
            Some(Kind::Keyfile) => a.name != kept.keyfile,
            None => false,
        })
        .map(|a| a.id)
        .collect();
    (kept, doomed)
}

/// The two asset shapes this build writes. Anything else is somebody else's.
enum Kind {
    Pack(ChunkId),
    Keyfile,
}

/// Strict: the id must parse as 64 hex characters, and the affixes must be
/// exact. `pack-<hex>.bin.bak` is not a pack, and is therefore not ours.
fn asset_kind(name: &str) -> Option<Kind> {
    if let Some(hex) = name
        .strip_prefix("pack-")
        .and_then(|r| r.strip_suffix(".bin"))
    {
        return hex.parse().ok().map(Kind::Pack);
    }
    let hex = name.strip_prefix("keyfile-")?.strip_suffix(".json")?;
    hex.parse::<ChunkId>().ok().map(|_| Kind::Keyfile)
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
    let _ = (release_id, permit);
    // Plan 4-05 replaces the empty slice with one `list_assets` call and then
    // deletes what comes back, sequentially. The dispatch is wired from the
    // tracer on purpose: a retention rule nothing calls is a retention rule
    // whose tests prove only that it can be called directly.
    let (_truncated, deletions) = plan_deletions(landed, &[], keep, ctx.now, super::PRUNE_GRACE);
    Ok(deletions.len())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::crypto::ChunkId;
    use crate::sync::github::write::ASSET_STATE_UPLOADED;
    use crate::sync::push::{POINTER_VERSION, SnapshotRecord, keyfile_asset_name, pack_asset_name};

    /// Fixed. Nothing here reads a clock: `plan_deletions` takes `now`.
    const NOW: DateTime<Utc> = match DateTime::from_timestamp(1_700_000_000, 0) {
        Some(t) => t,
        None => panic!("a fixed timestamp"),
    };

    fn id(byte: u8) -> ChunkId {
        ChunkId::from_bytes([byte; 32])
    }

    /// `age` is how long before [`NOW`] the remote created it — the only input
    /// the grace window reads.
    fn asset(asset_id: u64, name: &str, age: TimeDelta) -> Asset {
        Asset {
            id: asset_id,
            name: name.to_owned(),
            size: 9,
            state: ASSET_STATE_UPLOADED.to_owned(),
            created_at: NOW - age,
            digest: None,
        }
    }

    fn pack(asset_id: u64, byte: u8, age: TimeDelta) -> Asset {
        asset(asset_id, &pack_asset_name(&id(byte)), age)
    }

    fn snapshot(packs: Vec<ChunkId>) -> SnapshotRecord {
        SnapshotRecord {
            root: format!("root-{}", packs.len()),
            index_chunks: Vec::new(),
            packs,
        }
    }

    fn pointer(keyfile: &str, snapshots: Vec<SnapshotRecord>) -> Pointer {
        Pointer {
            format: POINTER_VERSION,
            repo_id: "github:1".into(),
            keyfile: keyfile.to_owned(),
            snapshots,
        }
    }

    const OLD: TimeDelta = TimeDelta::hours(48);
    const GRACE: TimeDelta = TimeDelta::hours(24);

    /// The base case: two surviving snapshots name A and B, C is nobody's.
    #[test]
    fn only_a_pack_no_surviving_snapshot_names_is_deleted() {
        let keyfile = keyfile_asset_name(&id(0xff));
        let p = pointer(
            &keyfile,
            vec![snapshot(vec![id(0xaa)]), snapshot(vec![id(0xaa), id(0xbb)])],
        );
        let assets = [
            pack(1, 0xaa, OLD),
            pack(2, 0xbb, OLD),
            pack(3, 0xcc, OLD),
            asset(4, &keyfile, OLD),
        ];

        let (kept, doomed) = plan_deletions(&p, &assets, 10, NOW, GRACE);

        assert_eq!(doomed, vec![3], "only C");
        assert_eq!(kept.snapshots.len(), 2, "nothing to truncate at keep = 10");
    }

    /// Both sides of the boundary. An unreferenced pack uploaded by a machine
    /// that has not flipped yet is indistinguishable from garbage; the age floor
    /// is the only thing that tells them apart.
    #[test]
    fn an_asset_younger_than_the_grace_window_survives_being_unreferenced() {
        let p = pointer("keyfile-x.json", vec![snapshot(vec![id(0xaa)])]);

        for (age, expected) in [
            (TimeDelta::hours(1), vec![]),
            (GRACE, vec![]),
            (GRACE + TimeDelta::seconds(1), vec![7]),
            (OLD, vec![7]),
        ] {
            let (_, doomed) = plan_deletions(&p, &[pack(7, 0xcc, age)], 10, NOW, GRACE);
            assert_eq!(doomed, expected, "at age {age}");
        }
    }

    /// Sharing is what makes ten snapshots cheap. A pack only the oldest
    /// *surviving* record names is still live.
    #[test]
    fn a_pack_only_the_oldest_surviving_snapshot_names_is_retained() {
        let p = pointer(
            "keyfile-x.json",
            vec![snapshot(vec![id(0xaa)]), snapshot(vec![id(0xbb)])],
        );
        let (_, doomed) = plan_deletions(&p, &[pack(1, 0xaa, OLD)], 10, NOW, GRACE);
        assert!(doomed.is_empty(), "{doomed:?}");
    }

    /// **The worst thing this function could do.** No snapshot's `packs` list
    /// names the keyfile, so the naive rule deletes it — and the wrapped master
    /// key is the only route to the data, for every machine, permanently.
    #[test]
    fn the_keyfile_the_pointer_names_is_never_deleted() {
        let keyfile = keyfile_asset_name(&id(0xff));
        for snapshots in [
            vec![],
            vec![snapshot(vec![])],
            vec![snapshot(vec![id(0xaa)]), snapshot(vec![id(0xbb)])],
        ] {
            let p = pointer(&keyfile, snapshots);
            // Ancient, unreferenced by any `packs` list, and alone in the
            // release: every reason the naive rule would have to collect it.
            let (_, doomed) = plan_deletions(
                &p,
                &[asset(1, &keyfile, TimeDelta::days(365))],
                1,
                NOW,
                GRACE,
            );
            assert!(doomed.is_empty(), "{doomed:?}");
        }
    }

    /// 4-06's gap: an interrupted first push, or a local-only rekey, can leave a
    /// keyfile asset no pointer names. Same grace window, same landed pointer.
    #[test]
    fn an_orphan_keyfile_no_pointer_names_is_swept_like_a_pack() {
        let live = keyfile_asset_name(&id(0xff));
        let orphan = keyfile_asset_name(&id(0xee));
        let p = pointer(&live, vec![snapshot(vec![id(0xaa)])]);
        let assets = [
            asset(1, &live, OLD),
            asset(2, &orphan, OLD),
            asset(3, &keyfile_asset_name(&id(0xdd)), TimeDelta::hours(2)),
        ];

        let (_, doomed) = plan_deletions(&p, &assets, 10, NOW, GRACE);

        assert_eq!(doomed, vec![2], "the live one and the young one both stay");
    }

    /// A collector that deletes what it does not understand turns every format
    /// addition into a data-loss bug.
    #[test]
    fn an_asset_this_build_does_not_recognise_is_never_deleted() {
        let p = pointer("keyfile-x.json", vec![snapshot(vec![id(0xaa)])]);
        let hex = "aa".repeat(32);
        let strangers = [
            "manifest-v2.bin".to_owned(),
            format!("pack-{hex}"),                   // no extension
            format!("pack-{hex}.bin.bak"),           // trailing junk
            format!("pack-{}.bin", "zz".repeat(32)), // not hex
            format!("pack-{}.bin", "aa".repeat(16)), // too short
            "pack-.bin".to_owned(),
            format!("keyfile-{}.json", "zz".repeat(32)),
            "keyfile.json".to_owned(),
            "README.md".to_owned(),
        ];
        let assets: Vec<Asset> = strangers
            .iter()
            .enumerate()
            .map(|(i, name)| asset(i as u64 + 1, name, TimeDelta::days(365)))
            .collect();

        let (_, doomed) = plan_deletions(&p, &assets, 10, NOW, GRACE);

        assert!(doomed.is_empty(), "{doomed:?}");
    }

    /// The packs freed by truncation are exactly what this pass exists to
    /// collect.
    #[test]
    fn truncation_drops_the_oldest_records_and_frees_their_packs() {
        let p = pointer(
            "keyfile-x.json",
            vec![
                snapshot(vec![id(0x01)]),
                snapshot(vec![id(0x02)]),
                snapshot(vec![id(0x03), id(0x02)]),
            ],
        );
        let assets = [pack(1, 0x01, OLD), pack(2, 0x02, OLD), pack(3, 0x03, OLD)];

        let (kept, doomed) = plan_deletions(&p, &assets, 2, NOW, GRACE);

        assert_eq!(kept.snapshots, p.snapshots[1..], "oldest end only");
        assert_eq!(doomed, vec![1], "0x02 is still shared, 0x03 is the newest");
    }

    /// The union of an empty set is empty, which read naively says "everything
    /// is garbage". A first-push race or a hand-edited pointer would then wipe
    /// the release.
    #[test]
    fn a_pointer_with_no_snapshots_proposes_no_deletions() {
        let p = pointer("keyfile-x.json", vec![]);
        let assets = [pack(1, 0xaa, OLD), pack(2, 0xbb, OLD)];
        let (kept, doomed) = plan_deletions(&p, &assets, 10, NOW, GRACE);
        assert!(doomed.is_empty(), "{doomed:?}");
        assert!(kept.snapshots.is_empty());
    }

    /// Config refuses `keep_snapshots = 0`, but this function's truncated
    /// pointer is *published*, so a zero arriving any other way must not empty
    /// the snapshot list.
    #[test]
    fn keep_of_zero_still_leaves_the_newest_record() {
        let p = pointer(
            "keyfile-x.json",
            vec![snapshot(vec![id(0x01)]), snapshot(vec![id(0x02)])],
        );
        let (kept, doomed) = plan_deletions(&p, &[pack(1, 0x01, OLD)], 0, NOW, GRACE);
        assert_eq!(kept.snapshots, p.snapshots[1..]);
        assert_eq!(doomed, vec![1]);
    }
}
