//! The pure `sync status` model and its renderer. Owned by plan 2-01.
//!
//! Same split the rest of the project uses: [`build_status`] touches the
//! filesystem, [`render_status`] is a pure function of the model, so the
//! wording is testable without a disk.

use std::path::PathBuf;

use chrono::{DateTime, Utc};

use crate::config::{SyncCategory, SyncConfig};
use crate::sync::index::Index;
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

/// Everything `sync status` prints.
#[derive(Debug, Clone)]
pub struct StatusReport {
    pub lines: Vec<CategoryLine>,
    pub last_sync: Option<DateTime<Utc>>,
    /// Empty when the index could not be opened — the caller has already said
    /// why, and the status is still worth printing without it.
    pub index_path: PathBuf,
}

impl StatusReport {
    pub fn total_files(&self) -> usize {
        self.lines.iter().map(|l| l.files).sum()
    }

    pub fn total_bytes(&self) -> u64 {
        self.lines.iter().map(|l| l.bytes).sum()
    }
}

/// Scan every category in D1 order. `now` goes straight through to
/// [`scope::collect`] so the transcripts bounds have a reference point that no
/// test has to fake by moving the clock.
pub fn build_status(
    roots: &SyncRoots,
    cfg: &SyncConfig,
    index: Option<&Index>,
    now: DateTime<Utc>,
) -> StatusReport {
    let lines = SyncCategory::ALL
        .iter()
        .map(|&category| line(scope::collect(category, roots, cfg, now), cfg))
        .collect();
    StatusReport {
        lines,
        last_sync: index.and_then(Index::last_sync),
        // Taken from the opened index rather than re-resolved, so nothing in
        // this builder touches `$HOME`.
        index_path: index.map(|i| i.path().to_path_buf()).unwrap_or_default(),
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

/// Pure. Given the same struct it always renders the same string.
pub fn render_status(report: &StatusReport) -> String {
    let width = SyncCategory::ALL
        .iter()
        .map(|c| c.label().len())
        .max()
        .unwrap_or(0);
    let mut out = String::new();
    for l in &report.lines {
        // "off" and "0" are different facts and the user is choosing between
        // them: an off category has not been looked at, not found to be empty.
        let detail = if l.enabled {
            let capped = if l.capped { " (capped)" } else { "" };
            format!("{:>5} files  {:>10}{capped}", l.files, human_bytes(l.bytes))
        } else {
            "  off".to_string()
        };
        out.push_str(&format!("  {:<width$}  {detail}\n", l.category.label()));
    }
    out.push_str(&format!(
        "\n  total{:<w$}  {:>5} files  {:>10}\n",
        "",
        report.total_files(),
        human_bytes(report.total_bytes()),
        w = width.saturating_sub(5)
    ));
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
    out
}

fn human_bytes(n: u64) -> String {
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

        let report = build_status(&roots_at(&dir), &SyncConfig::default(), None, Utc::now());
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

        let report = build_status(&roots_at(&dir), &SyncConfig::default(), None, Utc::now());
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
        let report = build_status(&roots_at(&dir), &SyncConfig::default(), None, Utc::now());
        assert!(report.last_sync.is_none());
        assert!(render_status(&report).contains("last sync: never"));
    }

    #[test]
    fn rendering_is_a_pure_function_of_the_report() {
        let report = StatusReport {
            lines: vec![CategoryLine {
                category: SyncCategory::Config,
                enabled: true,
                files: 3,
                bytes: 2048,
                capped: false,
            }],
            last_sync: DateTime::parse_from_rfc3339("2026-08-19T12:00:00Z")
                .ok()
                .map(|t| t.with_timezone(&Utc)),
            index_path: PathBuf::from("/nowhere/index.sqlite3"),
        };
        let once = render_status(&report);
        assert_eq!(once, render_status(&report));
        assert!(once.contains("2.0 KiB"), "{once}");
        assert!(once.contains("2026-08-19T12:00:00"), "{once}");
    }

    #[test]
    fn a_capped_walk_is_reported_rather_than_silently_under_counting() {
        let report = StatusReport {
            lines: vec![CategoryLine {
                category: SyncCategory::Transcripts,
                enabled: true,
                files: 200_000,
                bytes: 1,
                capped: true,
            }],
            last_sync: None,
            index_path: PathBuf::from("/nowhere"),
        };
        assert!(render_status(&report).contains("capped"));
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
