//! Every manifest entry, turned into exactly one decision.
//!
//! **Nothing is dropped on the floor.** An entry whose path is refused or whose
//! policy excludes it still becomes an [`ItemPlan`] — with no `dest` and a
//! [`Disposition`] that says why — because a silently discarded entry is a
//! tampered bundle nobody can see (D6, T-5-08).
//!
//! **Restore never plans a deletion.** A local file absent from the manifest is
//! left alone. That is a decision, not an omission: a snapshot is what one
//! machine had, not an assertion about what every machine should have.
//!
//! Plan 5-01 filled `Create`, `Update`, `ExcludedByPolicy` and `RejectedPath`.
//! Plan 5-03 owns the rest — the digest-before-timestamp `SkipIdentical` that
//! makes a second apply a no-op (D7), the locally-newer comparison (SAFE-03,
//! D2), and the credential classification whose second consent `force` alone
//! does not grant.

use std::path::Path;

use crate::config::SyncCategory;
use crate::error::Result;

use super::{Disposition, ItemPlan, Resolved, RestoreCtx, RestorePlan, layout};

/// Decide about every file in the snapshot.
pub fn plan(ctx: &RestoreCtx<'_>, resolved: &Resolved) -> Result<RestorePlan> {
    let mut items = Vec::with_capacity(resolved.manifest.files.len());

    for file in &resolved.manifest.files {
        let (dest, disposition) = decide(ctx, &file.path);
        items.push(ItemPlan {
            manifest_path: file.path.clone(),
            dest,
            category: category_of(&file.path),
            true_len: file.true_len,
            chunks: file.chunks.clone(),
            disposition,
        });
    }

    Ok(RestorePlan {
        items,
        counter: resolved.root.counter,
        created_at: resolved.root.created_at,
        repo_id: resolved.root.repo_id.clone(),
        packs_needed: resolved.packs.packs(),
        bytes_to_fetch: resolved.packs.bytes(),
    })
}

/// Path policy first, then the destination.
///
/// The policy check runs on the **manifest path**, before any resolution, so a
/// bundle naming machine-bound state is refused whatever root it claims (D4).
fn decide(ctx: &RestoreCtx<'_>, manifest_path: &str) -> (Option<std::path::PathBuf>, Disposition) {
    if !layout::accept_for_write(Path::new(manifest_path)) {
        return (None, Disposition::ExcludedByPolicy);
    }
    match layout::from_manifest_path(ctx.roots, manifest_path) {
        Err(why) => (None, Disposition::RejectedPath(why.to_string())),
        Ok(dest) => {
            // Plan 5-03 replaces this with digest-then-timestamp. Until then an
            // existing destination is an `Update`, which is honest about what
            // would happen and never claims a file is new when it is not.
            let disposition = if dest.exists() {
                Disposition::Update
            } else {
                Disposition::Create
            };
            (Some(dest), disposition)
        }
    }
}

/// Which category a bundle path belongs to, from its root prefix and the shape
/// beneath it — the same split `scope`'s collectors made on the way out.
///
/// Plan 5-03 owns refining this; what it must keep is that
/// `config/accounts/*/.credentials.json` and `desktop-profiles/**` are the two
/// credential-bearing shapes, because that is what the second consent hangs on.
fn category_of(manifest_path: &str) -> SyncCategory {
    let (prefix, rest) = manifest_path.split_once('/').unwrap_or((manifest_path, ""));
    match prefix {
        "desktop-profiles" => SyncCategory::Credentials,
        "desktop-data" if rest.starts_with("claude-code-sessions/") => SyncCategory::ChatIndex,
        "desktop-data" => SyncCategory::Routines,
        "claude-home" if rest.starts_with("projects/") => SyncCategory::Transcripts,
        "claude-home" => SyncCategory::Routines,
        _ => SyncCategory::Config,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_root_prefix_lands_in_the_category_its_collector_came_from() {
        for (path, expected) in [
            ("config/config.toml", SyncCategory::Config),
            (
                "config/accounts/work/.credentials.json",
                SyncCategory::Config,
            ),
            ("desktop-profiles/work/meta.json", SyncCategory::Credentials),
            (
                "desktop-data/claude-code-sessions/a/o/local_1.json",
                SyncCategory::ChatIndex,
            ),
            (
                "desktop-data/a/o/scheduled-tasks.json",
                SyncCategory::Routines,
            ),
            (
                "claude-home/scheduled-tasks/daily.json",
                SyncCategory::Routines,
            ),
            (
                "claude-home/projects/repo/s.jsonl",
                SyncCategory::Transcripts,
            ),
        ] {
            assert_eq!(category_of(path), expected, "{path}");
        }
    }
}
