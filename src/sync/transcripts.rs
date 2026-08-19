//! Bounded selection of local Claude Code transcripts (D3). Owned by plan 2-04.
//!
//! Only the signature is fixed here, by plan 2-01, so [`super::scope::collect`]
//! can wire its `Transcripts` arm once and never be edited again.

use chrono::{DateTime, Utc};

use crate::config::{SyncCategory, SyncConfig};
use crate::sync::SyncRoots;
use crate::sync::scope::CategoryScan;

/// Select `~/.claude/projects/**/*.jsonl` newest-first under both D3 bounds —
/// `transcript_days` and `transcript_max_bytes` — whichever binds first, whole
/// files only. Everything the bounds leave behind is counted into the scan's
/// `excluded_files` / `excluded_bytes` so the user is told what was dropped.
///
/// `now` is the age bound's reference point, passed in rather than read, so no
/// test here touches the wall clock.
pub fn collect_bounded(_roots: &SyncRoots, _cfg: &SyncConfig, _now: DateTime<Utc>) -> CategoryScan {
    // ponytail: plan 2-04 owns the body; the empty scan keeps `sync status`
    // honest in the meantime (transcripts is off by default anyway).
    CategoryScan::empty(SyncCategory::Transcripts)
}
