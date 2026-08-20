//! What a restore tells the user, before and after.
//!
//! The only module under `restore/` that prints or reads from a terminal; the
//! other five return values. Rendering is pure — a `&RestorePlan` in, a
//! `String` out — so no test needs a TTY.
//!
//! Plan 5-01 filled the two renderers to the level the tracer prints. Plan 5-06
//! owns the grouped table, the line budget that keeps a 5,000-file bundle from
//! printing 5,000 lines, the locally-newer block that must appear *before* the
//! gate, and the two gates themselves — the one interactive confirmation (D6)
//! and the separate credential consent that `--yes` alone never answers.

use super::{Disposition, RestoreOutcome, RestorePlan};

/// The dry-run report: what a restore would do, having written nothing.
pub fn render_plan(plan: &RestorePlan) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "snapshot {} of bundle {} — {} item(s), {} pack(s), {} byte(s)\n",
        plan.counter,
        plan.repo_id,
        plan.items.len(),
        plan.packs_needed,
        plan.bytes_to_fetch
    ));
    for item in &plan.items {
        out.push_str(&format!(
            "  {} {}\n",
            verb(&item.disposition),
            item.manifest_path
        ));
    }
    out.push_str("nothing was written — re-run with --apply to write\n");
    out
}

/// The post-restore summary.
pub fn render_outcome(outcome: &RestoreOutcome) -> String {
    if !outcome.applied {
        return render_plan(&outcome.plan);
    }
    let mut out = format!(
        "restored snapshot {}: {} written, {} skipped\n",
        outcome.plan.counter, outcome.written, outcome.skipped
    );
    for path in &outcome.overwritten {
        out.push_str(&format!("  overwrote {path}\n"));
    }
    if let Some(failed) = &outcome.failed_at {
        out.push_str(&format!("  stopped at {failed}\n"));
    }
    // Printed on the success and the partial-failure path alike: the partial
    // path is when it is needed.
    if let Some(backup) = &outcome.backup {
        out.push_str(&format!("undo: {}\n", backup.rollback_command()));
    }
    out
}

/// Exhaustive on purpose: a new [`Disposition`] must be a compile error here
/// rather than a silently unrendered case.
fn verb(disposition: &Disposition) -> &'static str {
    match disposition {
        Disposition::Create => "create",
        Disposition::Update => "update",
        Disposition::SkipIdentical => "unchanged",
        Disposition::SkipLocalNewer { .. } => "skip (local is newer)",
        Disposition::Overwrite { .. } => "overwrite (local is newer)",
        Disposition::NeedsCredentialConfirm { .. } => "needs confirmation (credential)",
        Disposition::ExcludedByPolicy => "excluded",
        Disposition::RejectedPath(_) => "rejected",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SyncCategory;
    use crate::sync::restore::ItemPlan;
    use chrono::{DateTime, Utc};

    const NOW: DateTime<Utc> = match DateTime::from_timestamp(1_700_000_000, 0) {
        Some(t) => t,
        None => panic!("a fixed timestamp"),
    };

    /// Every variant renders, so plan 5-06 inherits a fixture covering its
    /// whole surface rather than discovering a missing arm at runtime.
    fn every_disposition() -> Vec<Disposition> {
        vec![
            Disposition::Create,
            Disposition::Update,
            Disposition::SkipIdentical,
            Disposition::SkipLocalNewer {
                local_mtime: NOW,
                remote_mtime: NOW,
            },
            Disposition::Overwrite {
                local_mtime: NOW,
                remote_mtime: NOW,
            },
            Disposition::NeedsCredentialConfirm {
                local_mtime: NOW,
                remote_mtime: NOW,
            },
            Disposition::ExcludedByPolicy,
            Disposition::RejectedPath("it contains a `..` component".into()),
        ]
    }

    #[test]
    fn every_disposition_renders_and_names_its_own_item() {
        let items: Vec<ItemPlan> = every_disposition()
            .into_iter()
            .enumerate()
            .map(|(i, disposition)| ItemPlan {
                manifest_path: format!("config/item-{i}.json"),
                dest: None,
                category: SyncCategory::Config,
                true_len: 0,
                chunks: Vec::new(),
                disposition,
            })
            .collect();
        let count = items.len();
        let rendered = render_plan(&RestorePlan {
            items,
            counter: 3,
            created_at: NOW,
            repo_id: "github:1".into(),
            packs_needed: 2,
            bytes_to_fetch: 4096,
        });

        for i in 0..count {
            assert!(
                rendered.contains(&format!("config/item-{i}.json")),
                "item {i} is missing from the report"
            );
        }
        assert!(rendered.contains("nothing was written"));
    }

    #[test]
    fn an_outcome_names_what_it_overwrote_rather_than_counting_it() {
        let outcome = RestoreOutcome {
            plan: RestorePlan {
                items: Vec::new(),
                counter: 3,
                created_at: NOW,
                repo_id: "github:1".into(),
                packs_needed: 0,
                bytes_to_fetch: 0,
            },
            applied: true,
            backup: None,
            written: 2,
            overwritten: vec!["config/config.toml".into()],
            skipped: 1,
            failed_at: None,
        };
        let rendered = render_outcome(&outcome);
        assert!(rendered.contains("config/config.toml"));
        assert!(rendered.contains("2 written"));
    }
}
