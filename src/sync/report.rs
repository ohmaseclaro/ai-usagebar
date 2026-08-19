//! The pure `sync status` / `sync push --dry-run` model and its renderers.
//! Owned by plan 2-01, extended with D4's dry-run by plan 2-07.
//!
//! Same split the rest of the project uses: [`build_status`] touches the
//! filesystem, [`render_status`] and [`render_dry_run`] are pure functions of
//! the model, so the wording is testable without a disk.
//!
//! Both commands print **one** table, because they answer the same question at
//! different moments. The third column — what a push would actually put on the
//! wire — is the one D4 says matters, and it is present only when a sync key
//! was available: file counts and raw bytes need no key, and a user checking
//! what is in scope should not have to authenticate. Where the key is missing
//! the column is absent and the reason is printed, never a zero.

use std::path::PathBuf;

use chrono::{DateTime, Utc};

use crate::config::{SyncCategory, SyncConfig};
use crate::sync::index::Index;
use crate::sync::plan::{CategoryPlan, SyncPlan};
use crate::sync::scope;
use crate::sync::{SyncRoots, scope::CategoryScan};

/// One category's row in `sync status`.
#[derive(Debug, Clone)]
pub struct CategoryLine {
    pub category: SyncCategory,
    pub enabled: bool,
    pub files: usize,
    pub bytes: u64,
    /// True when the walk hit its entry cap — the counts are a floor, not a
    /// total, and saying so beats quietly under-reporting.
    pub capped: bool,
}

/// What `sync status` learned about the remote (plan 3-07).
///
/// **Best-effort with respect to the category lines, not with respect to the
/// exit code.** A user whose token expired should still see what would be sent,
/// so [`failure`](RepoSection::failure) suppresses nothing above it — but a
/// non-empty `failure` is a non-zero exit (D-06, REPO-05, T-3-41).
///
/// The token appears here only as a **source label**. There is no field that
/// could hold its value, which is a cheaper guarantee than remembering not to
/// print one (REPO-02).
#[derive(Debug, Clone, Default)]
pub struct RepoSection {
    /// `owner/name` from `[sync] repo`; `None` means the key is unset, which is
    /// an unconfigured machine rather than a failure.
    pub configured: Option<String>,
    /// What GitHub reported this run.
    pub visibility: Option<String>,
    /// [`TokenSource::label`](crate::sync::github::token::TokenSource::label).
    pub token_source: Option<&'static str>,
    /// The pairing record's `checked_at` — when this machine last verified it.
    pub last_verified: Option<DateTime<Utc>>,
    /// Drift and gate warnings, verbatim from plans 3-04 and 3-03.
    pub warnings: Vec<String>,
    /// Why the section could not be filled, or the incident that filled it.
    /// Verbatim; a status-flavoured paraphrase would make one event read as
    /// two.
    pub failure: Option<String>,
}

impl RepoSection {
    /// Record why this section could not be filled and stop. Returns `self` so
    /// a caller can `return section.failed(e.to_string())` on one line — the
    /// fields gathered before the failure stay visible, which is the point of
    /// the section being best-effort about its *contents*.
    pub fn failed(mut self, why: String) -> RepoSection {
        self.failure = Some(why);
        self
    }
}

/// Everything `sync status` prints.
#[derive(Debug, Clone)]
pub struct StatusReport {
    pub lines: Vec<CategoryLine>,
    pub last_sync: Option<DateTime<Utc>>,
    /// Empty when the index could not be opened — the caller has already said
    /// why, and the status is still worth printing without it.
    pub index_path: PathBuf,
    /// What a push would send right now. `None` before a plan was ever built,
    /// or when no sync key was available to build one.
    pub plan: Option<SyncPlan>,
    /// The repository half. `None` only for `push --dry-run`, which contacts no
    /// network at all.
    pub repo: Option<RepoSection>,
}

impl StatusReport {
    /// Enabled categories only. A disabled one contributes no files by
    /// construction, and summing it anyway would make that an accident.
    pub fn total_files(&self) -> usize {
        self.lines
            .iter()
            .filter(|l| l.enabled)
            .map(|l| l.files)
            .sum()
    }

    pub fn total_bytes(&self) -> u64 {
        self.lines
            .iter()
            .filter(|l| l.enabled)
            .map(|l| l.bytes)
            .sum()
    }

    fn category(&self, cat: SyncCategory) -> Option<&CategoryPlan> {
        self.plan
            .as_ref()?
            .categories
            .iter()
            .find(|c| c.category == cat)
    }
}

/// Everything `sync push --dry-run` prints.
///
/// A thin wrapper over [`StatusReport`] rather than a parallel model: the
/// dry-run and the status show the same figures, and the only thing the dry-run
/// adds is what to say when the would-upload column could not be computed.
#[derive(Debug, Clone)]
pub struct DryRunReport {
    pub status: StatusReport,
    /// Why `status.plan` is `None`, in the user's words. Rendered in place of
    /// the column — a wrong zero would read as "a push is free".
    pub no_key: Option<String>,
}

/// Scan every category in D1 order. `now` goes straight through to
/// [`scope::collect`] so the transcripts bounds have a reference point that no
/// test has to fake by moving the clock.
///
/// With a `plan`, the rows come from the walk the planner already did: a second
/// scan would cost another walk of the same tree and could disagree with the
/// first if a file appeared between them.
pub fn build_status(
    roots: &SyncRoots,
    cfg: &SyncConfig,
    index: Option<&Index>,
    now: DateTime<Utc>,
    plan: Option<SyncPlan>,
    repo: Option<RepoSection>,
) -> StatusReport {
    let lines = match &plan {
        Some(p) => p.categories.iter().map(|c| line_of(c, cfg)).collect(),
        None => SyncCategory::ALL
            .iter()
            .map(|&category| line(scope::collect(category, roots, cfg, now), cfg))
            .collect(),
    };
    StatusReport {
        lines,
        last_sync: index.and_then(Index::last_sync),
        // Taken from the opened index rather than re-resolved, so nothing in
        // this builder touches `$HOME`.
        index_path: index.map(|i| i.path().to_path_buf()).unwrap_or_default(),
        plan,
        repo,
    }
}

fn line(scan: CategoryScan, cfg: &SyncConfig) -> CategoryLine {
    CategoryLine {
        category: scan.category,
        enabled: cfg.includes(scan.category),
        files: scan.files.len(),
        bytes: scan.bytes,
        capped: scan.walk_capped,
    }
}

fn line_of(c: &CategoryPlan, cfg: &SyncConfig) -> CategoryLine {
    CategoryLine {
        category: c.category,
        enabled: cfg.includes(c.category),
        files: c.files,
        bytes: c.raw_bytes,
        capped: c.capped,
    }
}

/// Pure. Given the same struct it always renders the same string.
pub fn render_status(report: &StatusReport) -> String {
    let mut out = table(report);
    out.push_str(&format!(
        "\n  last sync: {}\n",
        report
            .last_sync
            .map_or_else(|| "never".to_string(), |t| t.to_rfc3339()),
    ));
    if report.index_path.as_os_str().is_empty() {
        out.push_str("  index:     unavailable\n");
    } else {
        out.push_str(&format!("  index:     {}\n", report.index_path.display()));
    }
    out.push_str(&rebuilt_note(report));
    if let Some(repo) = &report.repo {
        out.push_str(&render_repo(repo));
    }
    out
}

/// The repository half of `sync status`. Pure, like everything else here.
///
/// Prints the token's **source**, never the token — [`RepoSection`] has no
/// field that could carry one.
fn render_repo(repo: &RepoSection) -> String {
    let Some(name) = &repo.configured else {
        return "\n  repo:      not configured — no sync repository is paired.\n\
                \x20            Name one in config.toml, after creating it yourself:\n\
                \x20              gh repo create <owner>/<name> --private\n\
                \n\
                \x20              [sync]\n\
                \x20              repo = \"<owner>/<name>\"\n"
            .to_string();
    };

    let mut out = format!("\n  repo:      {name}\n");
    out.push_str(&format!(
        "  visible:   {}\n",
        repo.visibility.as_deref().unwrap_or("unknown")
    ));
    out.push_str(&match repo.token_source {
        Some(source) => format!("  token:     present ({source})\n"),
        None => "  token:     none found\n".to_string(),
    });
    out.push_str(&match repo.last_verified {
        Some(at) => format!("  verified:  {}\n", at.to_rfc3339()),
        None => "  verified:  never — this machine is not paired yet, run \
                 `ai-usagebar sync setup`\n"
            .to_string(),
    });
    for warning in &repo.warnings {
        out.push_str(&note("warning", warning));
    }
    if let Some(failure) = &repo.failure {
        out.push_str(&note("repo:      FAILED", failure));
    }
    out
}

/// D4's dry-run: the same table, then the totals, then what a push would
/// actually do. Pure — it takes the model and returns a string, and touches no
/// filesystem.
pub fn render_dry_run(report: &DryRunReport) -> String {
    let status = &report.status;
    let mut out = table(status);

    out.push_str(&format!(
        "\n  snapshot: {} files, {} of local state\n",
        status.total_files(),
        human_bytes(status.total_bytes())
    ));

    match (&status.plan, &report.no_key) {
        (Some(plan), _) => {
            if plan.is_empty() {
                out.push_str(&format!(
                    "  a push would send nothing — every file matched the local index, \
                     {} opened\n",
                    plan.files_opened
                ));
            } else {
                out.push_str(&format!(
                    "  a push would send {} in {} new chunks ({} of plaintext, from {} \
                     file{} read)\n",
                    human_bytes(plan.total_new_stored_bytes),
                    plan.new_chunk_ids.len(),
                    human_bytes(plan.total_new_bytes),
                    plan.files_opened,
                    if plan.files_opened == 1 { "" } else { "s" },
                ));
            }
            if plan.append_check_miss_bytes > 0 {
                out.push_str(&format!(
                    "  {} was re-read by append checks that then failed\n",
                    human_bytes(plan.append_check_miss_bytes)
                ));
            }
        }
        // Never a zero here: "0 bytes" and "not computed" are opposite answers
        // to "what will this cost me".
        (None, Some(why)) => out.push_str(&note("would upload: not computed", why)),
        (None, None) => out.push_str(&note("would upload: not computed", "no plan was built")),
    }

    out.push_str(&rebuilt_note(status));
    out.push_str("  --dry-run uploads nothing and contacts no network.\n");
    out
}

/// The shared table: one row per category, then the totals. The third column
/// appears only when a plan was built.
fn table(report: &StatusReport) -> String {
    let width = SyncCategory::ALL
        .iter()
        .map(|c| c.label().len())
        .max()
        .unwrap_or(0);
    let has_plan = report.plan.is_some();

    let mut out = String::new();
    if has_plan {
        out.push_str(&format!(
            "  {:<width$}  {:>11}  {:>10}  {:>10}\n",
            "", "files", "raw", "would send"
        ));
    }
    for l in &report.lines {
        // "off" and "0" are different facts and the user is choosing between
        // them: an off category has not been looked at, not found to be empty.
        if !l.enabled {
            out.push_str(&format!(
                "  {:<width$}  {:>11}\n",
                l.category.label(),
                "off"
            ));
            continue;
        }
        out.push_str(&format!(
            "  {:<width$}  {:>5} files  {:>10}",
            l.category.label(),
            l.files,
            human_bytes(l.bytes)
        ));
        if let Some(c) = report.category(l.category) {
            out.push_str(&format!("  {:>10}", human_bytes(c.new_stored_bytes)));
        }
        if l.capped {
            out.push_str("  (capped)");
        }
        out.push('\n');
        out.push_str(&excluded_note(report, l, width));
    }

    out.push_str(&format!(
        "\n  total{:<w$}  {:>5} files  {:>10}",
        "",
        report.total_files(),
        human_bytes(report.total_bytes()),
        w = width.saturating_sub(5)
    ));
    if let Some(plan) = &report.plan {
        out.push_str(&format!(
            "  {:>10}",
            human_bytes(plan.total_new_stored_bytes)
        ));
    }
    out.push('\n');
    out
}

/// What D3's bounds left behind — **transcripts only**.
///
/// The other four categories have no bounds, so their `excluded_*` are
/// structurally zero and a column of zeros would invite the reader to look for
/// a meaning it does not have (2-CONTEXT, and plan 2-06's note).
///
/// Deliberately never phrased as "30 days": on this project's own measured
/// archive the byte budget binds first and reaches back ~21 days, so the day
/// window is a ceiling to report against, not a promise to make.
fn excluded_note(report: &StatusReport, l: &CategoryLine, width: usize) -> String {
    if l.category != SyncCategory::Transcripts {
        return String::new();
    }
    match report.category(l.category) {
        Some(c) if c.excluded_files > 0 => format!(
            "  {:<width$}  {:>5} files  {:>10}   left out by the age and size bounds\n",
            "",
            c.excluded_files,
            human_bytes(c.excluded_bytes)
        ),
        _ => String::new(),
    }
}

/// `  {head} — {first line}`, with any further lines of `body` indented under
/// it. The reasons a column is missing run past a terminal width otherwise.
fn note(head: &str, body: &str) -> String {
    let mut lines = body.lines();
    let mut out = format!("  {head} — {}\n", lines.next().unwrap_or_default());
    for line in lines {
        out.push_str(&format!("    {line}\n"));
    }
    out
}

/// A rebuilt index makes everything read as changed, which is the difference
/// between a slow first run and a bug.
fn rebuilt_note(report: &StatusReport) -> String {
    match &report.plan {
        Some(p) if p.index_rebuilt => "  the local index was missing or unreadable and was \
             rebuilt, so everything reads as new — the next run will be cheap.\n"
            .to_string(),
        _ => String::new(),
    }
}

pub(crate) fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::plan;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn seed(dir: &Path, rel: &str, body: &str) {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, body).unwrap();
    }

    fn roots_at(dir: &TempDir) -> SyncRoots {
        SyncRoots::at(
            dir.path().join("config.toml"),
            dir.path().to_path_buf(),
            dir.path().join("desktop"),
            dir.path().join("profiles"),
            dir.path().join("claude-home"),
        )
    }

    #[test]
    fn a_seeded_tree_renders_every_category_in_d1_order_with_counts_and_bytes() {
        let dir = TempDir::new().unwrap();
        seed(dir.path(), "config.toml", "[sync]\n");
        seed(dir.path(), "accounts/work/.credentials.json", "{}");

        let report = build_status(
            &roots_at(&dir),
            &SyncConfig::default(),
            None,
            Utc::now(),
            None,
            None,
        );
        assert_eq!(
            report.lines.iter().map(|l| l.category).collect::<Vec<_>>(),
            SyncCategory::ALL.to_vec()
        );
        let config_line = &report.lines[0];
        assert_eq!(config_line.files, 2);
        assert_eq!(config_line.bytes, 9); // "[sync]\n" + "{}"

        let text = render_status(&report);
        for cat in SyncCategory::ALL {
            assert!(
                text.contains(cat.label()),
                "missing {}: {text}",
                cat.label()
            );
        }
        assert!(text.contains("2 files"), "{text}");
    }

    #[test]
    fn a_category_absent_from_the_configured_set_renders_off_not_zero() {
        let dir = TempDir::new().unwrap();
        seed(dir.path(), "config.toml", "[sync]\n");

        let report = build_status(
            &roots_at(&dir),
            &SyncConfig::default(),
            None,
            Utc::now(),
            None,
            None,
        );
        let transcripts = report.lines.last().unwrap();
        assert_eq!(transcripts.category, SyncCategory::Transcripts);
        assert!(!transcripts.enabled);

        let line = render_status(&report)
            .lines()
            .find(|l| l.contains("transcripts"))
            .unwrap()
            .to_string();
        assert!(line.contains("off"), "{line}");
        assert!(!line.contains("files"), "{line}");
    }

    #[test]
    fn no_last_sync_renders_as_never() {
        let dir = TempDir::new().unwrap();
        let report = build_status(
            &roots_at(&dir),
            &SyncConfig::default(),
            None,
            Utc::now(),
            None,
            None,
        );
        assert!(report.last_sync.is_none());
        assert!(render_status(&report).contains("last sync: never"));
    }

    fn report_of(lines: Vec<CategoryLine>, plan: Option<SyncPlan>) -> StatusReport {
        StatusReport {
            lines,
            last_sync: DateTime::parse_from_rfc3339("2026-08-19T12:00:00Z")
                .ok()
                .map(|t| t.with_timezone(&Utc)),
            index_path: PathBuf::from("/nowhere/index.sqlite3"),
            plan,
            repo: None,
        }
    }

    fn a_line(category: SyncCategory, files: usize, bytes: u64) -> CategoryLine {
        CategoryLine {
            category,
            enabled: true,
            files,
            bytes,
            capped: false,
        }
    }

    fn a_category(category: SyncCategory, files: usize, raw: u64, stored: u64) -> CategoryPlan {
        CategoryPlan {
            category,
            files,
            raw_bytes: raw,
            new_bytes: raw,
            new_stored_bytes: stored,
            excluded_files: 0,
            excluded_bytes: 0,
            capped: false,
        }
    }

    fn a_plan(categories: Vec<CategoryPlan>) -> SyncPlan {
        let total_raw = categories.iter().map(|c| c.raw_bytes).sum();
        let total_new = categories.iter().map(|c| c.new_bytes).sum();
        let total_stored = categories.iter().map(|c| c.new_stored_bytes).sum();
        SyncPlan {
            categories,
            new_chunk_ids: if total_stored == 0 {
                Vec::new()
            } else {
                vec![[7u8; 32]]
            },
            total_raw_bytes: total_raw,
            total_new_bytes: total_new,
            total_new_stored_bytes: total_stored,
            files_opened: 0,
            append_check_miss_bytes: 0,
            index_rebuilt: false,
            file_plans: Vec::new(),
        }
    }

    #[test]
    fn rendering_is_a_pure_function_of_the_report() {
        let report = report_of(vec![a_line(SyncCategory::Config, 3, 2048)], None);
        let once = render_status(&report);
        assert_eq!(once, render_status(&report));
        assert!(once.contains("2.0 KiB"), "{once}");
        assert!(once.contains("2026-08-19T12:00:00"), "{once}");
    }

    #[test]
    fn a_capped_walk_is_reported_rather_than_silently_under_counting() {
        let report = report_of(
            vec![CategoryLine {
                category: SyncCategory::Transcripts,
                enabled: true,
                files: 200_000,
                bytes: 1,
                capped: true,
            }],
            None,
        );
        assert!(render_status(&report).contains("capped"));
    }

    // ---- 2-07: D4's dry-run ---------------------------------------------

    /// The four figures D4 names, per category and in total.
    #[test]
    fn a_dry_run_renders_files_raw_bytes_and_would_send_per_category() {
        let report = DryRunReport {
            status: report_of(
                vec![
                    a_line(SyncCategory::Config, 2, 4096),
                    a_line(SyncCategory::Credentials, 107, 24 * 1024 * 1024),
                ],
                Some(a_plan(vec![
                    a_category(SyncCategory::Config, 2, 4096, 1024),
                    a_category(
                        SyncCategory::Credentials,
                        107,
                        24 * 1024 * 1024,
                        11 * 1024 * 1024,
                    ),
                ])),
            ),
            no_key: None,
        };

        let text = render_dry_run(&report);
        let config = text.lines().find(|l| l.contains("config")).unwrap();
        assert!(config.contains("2 files"), "{config}");
        assert!(config.contains("4.0 KiB"), "raw bytes: {config}");
        assert!(config.contains("1.0 KiB"), "would-send bytes: {config}");

        let total = text.lines().find(|l| l.contains("total")).unwrap();
        assert!(total.contains("109 files"), "{total}");
        assert!(total.contains("24.0 MiB"), "{total}");
        assert!(total.contains("11.0 MiB"), "{total}");
        assert!(text.contains("snapshot: 109 files"), "{text}");
        assert!(text.contains("would send 11.0 MiB"), "{text}");
        assert!(text.contains("uploads nothing"), "{text}");
    }

    #[test]
    fn a_disabled_category_renders_off_in_every_column_not_three_zeros() {
        let report = DryRunReport {
            status: report_of(
                vec![
                    a_line(SyncCategory::Config, 1, 100),
                    CategoryLine {
                        category: SyncCategory::Transcripts,
                        enabled: false,
                        files: 0,
                        bytes: 0,
                        capped: false,
                    },
                ],
                Some(a_plan(vec![a_category(SyncCategory::Config, 1, 100, 64)])),
            ),
            no_key: None,
        };

        let line = render_dry_run(&report)
            .lines()
            .find(|l| l.contains("transcripts"))
            .unwrap()
            .to_string();
        assert!(line.contains("off"), "{line}");
        assert!(!line.contains('0'), "not a row of zeros: {line}");
    }

    /// 2-CONTEXT: the excluded column counts bound-dropped files, which only
    /// transcripts have — and it must never be phrased as "30 days", because on
    /// the measured archive the byte budget binds first.
    #[test]
    fn only_transcripts_report_what_the_bounds_left_behind() {
        let mut transcripts = a_category(SyncCategory::Transcripts, 2077, 2_136_746_229, 1_000);
        transcripts.excluded_files = 2135;
        transcripts.excluded_bytes = 1_782_579_527;
        let config = a_category(SyncCategory::Config, 1, 183, 168);

        let report = DryRunReport {
            status: report_of(
                vec![
                    a_line(SyncCategory::Config, 1, 183),
                    a_line(SyncCategory::Transcripts, 2077, 2_136_746_229),
                ],
                Some(a_plan(vec![config, transcripts])),
            ),
            no_key: None,
        };

        let text = render_dry_run(&report);
        assert!(text.contains("2135 files"), "the excluded count: {text}");
        assert!(
            text.contains("left out by the age and size bounds"),
            "{text}"
        );
        assert!(
            !text.contains("30 days"),
            "the byte budget binds first, so the day window is not a promise: {text}"
        );
        // One excluded row, on the transcripts line only.
        assert_eq!(
            text.lines()
                .filter(|l| l.contains("left out by the"))
                .count(),
            1,
            "{text}"
        );
    }

    #[test]
    fn a_plan_with_nothing_new_says_a_push_would_send_nothing() {
        let report = DryRunReport {
            status: report_of(
                vec![a_line(SyncCategory::Config, 1649, 104_248_000)],
                Some(a_plan(vec![a_category(
                    SyncCategory::Config,
                    1649,
                    104_248_000,
                    0,
                )])),
            ),
            no_key: None,
        };

        let text = render_dry_run(&report);
        assert!(text.contains("would send nothing"), "{text}");
        assert!(text.contains("0 opened"), "SYNC-02's evidence: {text}");
        assert!(!text.contains("new chunks"), "{text}");
    }

    #[test]
    fn a_rebuilt_index_is_reported_because_it_explains_a_slow_run() {
        let mut plan = a_plan(vec![a_category(SyncCategory::Config, 1, 10, 64)]);
        plan.index_rebuilt = true;
        let report = DryRunReport {
            status: report_of(vec![a_line(SyncCategory::Config, 1, 10)], Some(plan)),
            no_key: None,
        };
        assert!(render_dry_run(&report).contains("rebuilt"));
    }

    /// SCOPE-04's answerable half: counts and raw bytes need no key at all.
    #[test]
    fn without_a_key_the_counts_still_render_and_the_missing_column_is_named() {
        let report = DryRunReport {
            status: report_of(
                vec![
                    a_line(SyncCategory::Config, 2, 4096),
                    a_line(SyncCategory::Credentials, 107, 24 * 1024 * 1024),
                ],
                None,
            ),
            no_key: Some("this needs the sync password".to_string()),
        };

        let text = render_dry_run(&report);
        assert!(text.contains("2 files"), "{text}");
        assert!(text.contains("4.0 KiB"), "{text}");
        assert!(text.contains("24.0 MiB"), "{text}");
        assert!(text.contains("sync password"), "{text}");
        assert!(text.contains("not computed"), "{text}");
        assert!(
            !text.contains("would send 0"),
            "a wrong zero reads as 'a push is free': {text}"
        );
    }

    #[test]
    fn rendering_a_dry_run_is_pure_and_touches_no_filesystem() {
        let report = DryRunReport {
            status: report_of(
                vec![a_line(SyncCategory::Config, 1, 10)],
                Some(a_plan(vec![a_category(SyncCategory::Config, 1, 10, 64)])),
            ),
            no_key: None,
        };
        assert_eq!(render_dry_run(&report), render_dry_run(&report));
    }

    /// The one integration-style test: a real plan through the real chunker,
    /// and the rendered totals are that plan's own fields.
    #[test]
    fn the_rendered_totals_are_the_plans_own_figures() {
        use crate::sync::crypto::{KdfParams, Keyfile};
        use crate::sync::index::Index;

        let dir = TempDir::new().unwrap();
        seed(dir.path(), "config.toml", "[sync]\n");
        seed(dir.path(), "accounts/work/.credentials.json", "{}");
        let roots = roots_at(&dir);
        let index = Index::at(&dir.path().join("index.sqlite3")).unwrap();
        // Microseconds, not a gibibyte: the AUR `check()` runs this.
        let cheap = KdfParams {
            m_kib: 8,
            t: 1,
            p: 1,
        };
        let keys = Keyfile::create_with_floor(b"a-test-passphrase", cheap, cheap.m_kib)
            .unwrap()
            .1;

        let cfg = SyncConfig {
            categories: vec![SyncCategory::Config],
            transcript_days: 30,
            transcript_max_bytes: 0,
            repo: None,
        };
        // Fixed, not the wall clock: `now` is only a reference point for the
        // transcript bounds, and a test that reads the clock is a test that can
        // fail on a date nobody chose.
        let now = DateTime::from_timestamp(1_760_000_000, 0).unwrap();
        let plan = plan::build_with_keys(&roots, &cfg, &index, now, &keys).unwrap();
        assert_eq!(plan.files_opened, 2);
        assert!(plan.total_new_stored_bytes > 0);
        let stored = plan.total_new_stored_bytes;

        let report = DryRunReport {
            status: build_status(&roots, &cfg, Some(&index), now, Some(plan), None),
            no_key: None,
        };
        let text = render_dry_run(&report);
        assert!(text.contains(&human_bytes(stored)), "{stored}: {text}");
        assert!(text.contains("2 files"), "{text}");
        assert!(
            text.contains("9 B"),
            "the raw bytes of the two files: {text}"
        );

        // …and a second dry-run over the untouched tree sends nothing.
        let again = plan::build_with_keys(&roots, &cfg, &index, now, &keys).unwrap();
        assert_eq!(again.files_opened, 0);
        let report = DryRunReport {
            status: build_status(&roots, &cfg, Some(&index), now, Some(again), None),
            no_key: None,
        };
        assert!(render_dry_run(&report).contains("would send nothing"));
    }

    #[test]
    fn human_bytes_steps_through_the_binary_units() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1024), "1.0 KiB");
        assert_eq!(human_bytes(4 * 1024 * 1024), "4.0 MiB");
        assert_eq!(human_bytes(2 * 1024 * 1024 * 1024), "2.0 GiB");
    }
}
