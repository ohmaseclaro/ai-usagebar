//! `ai-usagebar sync …` entry point. Owned by plan 2-01.
//!
//! Output carries paths and byte counts only — never a file's contents. This
//! command's whole job is telling the user what *would* leave the machine, so
//! printing any of it here would defeat the point.

use chrono::Utc;

use crate::config::Config;
use crate::sync::index::{self, Index};
use crate::sync::{SyncRoots, report};
use crate::widget::cli::SyncAction;

/// Same shape as `account::run` / `tui::settings::run_cli`: an exit code, no
/// async, no Waybar exit-0 contract — a script piping this deserves a real code.
pub fn run(action: &SyncAction) -> i32 {
    match action {
        SyncAction::Status => status(),
    }
}

fn status() -> i32 {
    let config = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("sync: could not read the config file: {e}");
            return 1;
        }
    };
    let roots = match SyncRoots::resolve(&config) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("sync: {e}");
            return 1;
        }
    };

    // The index is a hint (D5): if it will not open, the scan is still the
    // truth and only the last-sync line is lost.
    let index = match index::default_path().and_then(|p| Index::at(&p)) {
        Ok(i) => Some(i),
        Err(e) => {
            eprintln!("sync: local index unavailable, last-sync unknown ({e})");
            None
        }
    };

    let report = report::build_status(&roots, &config.sync, index.as_ref(), Utc::now());
    print!("{}", report::render_status(&report));
    0
}
