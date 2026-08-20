//! Progress reporting for a long push, at **asset** granularity (D6).
//!
//! A first push moves ~115 MB in a handful of large assets, so "asset i of n,
//! bytes done of total" is the whole of what a user needs. There is deliberately
//! no per-chunk hook and one must not be added: 5,000 chunk callbacks per push
//! is chatter, and the thing a user is waiting on is the upload, which happens
//! one asset at a time.
//!
//! Plan 4-01 defined the trait and [`Silent`]; plan 4-03 added [`Terminal`] and
//! [`Plain`] behind it, chosen by [`reporter`] from an **injected** flag.
//!
//! Both are split into a pure renderer — [`render`] — and a thin writer, and
//! the tests assert against the renderer's string. A progress reporter whose
//! correctness can only be checked by looking at a terminal is a progress
//! reporter with no tests.
//!
//! Everything goes to **standard error**. `sync push`'s standard output is the
//! machine-readable outcome, and a progress line in it would corrupt anything
//! piping the command (T-4-27).

/// What `upload::run` reports as it works.
///
/// A trait rather than a closure so the non-terminal implementation can hold the
/// rate-limiting state D6 asks for, and so `upload::run` needs no branch.
pub trait Progress {
    /// Called once, with the assets actually being uploaded and the sum of their
    /// lengths — measured, never projected.
    fn start(&mut self, assets: usize, total_bytes: u64);

    /// One completed asset. `index` is zero-based.
    fn asset_done(&mut self, index: usize, name: &str, bytes: u64);

    /// Called once, whether the run succeeded or not.
    fn finish(&mut self);
}

/// No-ops. What every test passes, and what a caller that wants no output uses.
#[derive(Debug, Default)]
pub struct Silent;

impl Progress for Silent {
    fn start(&mut self, _assets: usize, _total_bytes: u64) {}
    fn asset_done(&mut self, _index: usize, _name: &str, _bytes: u64) {}
    fn finish(&mut self) {}
}

use std::io::{Stderr, Write, stderr};

use crate::sync::report::human_bytes;

/// The one line both implementations render, as a pure function of the
/// counters.
///
/// **Counts and byte totals only.** The asset name reaches
/// [`Progress::asset_done`] and deliberately stops there: it is a content
/// address today, and a reporter that prints its argument is one refactor away
/// from printing something that is not (T-4-25).
pub fn render(done: usize, total: usize, bytes_done: u64, bytes_total: u64) -> String {
    format!(
        "uploading {done}/{total} asset{} — {} of {}",
        if total == 1 { "" } else { "s" },
        human_bytes(bytes_done),
        human_bytes(bytes_total),
    )
}

/// What both implementations carry, so the arithmetic exists once.
#[derive(Debug, Default, Clone, Copy)]
struct Counters {
    done: usize,
    total: usize,
    bytes_done: u64,
    bytes_total: u64,
}

impl Counters {
    fn start(&mut self, assets: usize, total_bytes: u64) -> String {
        self.total = assets;
        self.bytes_total = total_bytes;
        self.line()
    }

    fn asset_done(&mut self, bytes: u64) -> String {
        self.done += 1;
        self.bytes_done += bytes;
        self.line()
    }

    fn line(&self) -> String {
        render(self.done, self.total, self.bytes_done, self.bytes_total)
    }
}

/// One line, rewritten in place — for a terminal.
///
/// Generic over its sink so a test can assert against the bytes actually
/// emitted rather than against a tty. Write failures are **swallowed**: a
/// closed standard error must not fail a push that is otherwise succeeding.
#[derive(Debug)]
pub struct Terminal<W = Stderr> {
    out: W,
    at: Counters,
}

impl Default for Terminal<Stderr> {
    fn default() -> Self {
        Self::to(stderr())
    }
}

impl<W: Write> Terminal<W> {
    pub fn to(out: W) -> Self {
        Self {
            out,
            at: Counters::default(),
        }
    }

    fn rewrite(&mut self, line: &str) {
        let _ = write!(self.out, "\r{line}");
        let _ = self.out.flush();
    }
}

impl<W: Write> Progress for Terminal<W> {
    fn start(&mut self, assets: usize, total_bytes: u64) {
        let line = self.at.start(assets, total_bytes);
        self.rewrite(&line);
    }

    fn asset_done(&mut self, _index: usize, _name: &str, bytes: u64) {
        let line = self.at.asset_done(bytes);
        self.rewrite(&line);
    }

    fn finish(&mut self) {
        let _ = writeln!(self.out);
        let _ = self.out.flush();
    }
}

/// One plain line per completed asset — for anything that is not a terminal.
///
/// D6 is explicit that non-TTY output degrades to periodic lines and never a
/// spinner, because the macOS menu bar captures this command's output as a
/// subprocess and carriage returns and escape sequences make that capture
/// unreadable. The cadence is one line per asset, which at `PACK_MAX` bodies is
/// already the right rate — there is no timer here and no second knob.
#[derive(Debug)]
pub struct Plain<W = Stderr> {
    out: W,
    at: Counters,
}

impl Default for Plain<Stderr> {
    fn default() -> Self {
        Self::to(stderr())
    }
}

impl<W: Write> Plain<W> {
    pub fn to(out: W) -> Self {
        Self {
            out,
            at: Counters::default(),
        }
    }
}

impl<W: Write> Progress for Plain<W> {
    fn start(&mut self, assets: usize, total_bytes: u64) {
        let line = self.at.start(assets, total_bytes);
        let _ = writeln!(self.out, "{line}");
    }

    fn asset_done(&mut self, _index: usize, _name: &str, bytes: u64) {
        let line = self.at.asset_done(bytes);
        let _ = writeln!(self.out, "{line}");
    }

    fn finish(&mut self) {
        let _ = writeln!(self.out, "{} — done", self.at.line());
        let _ = self.out.flush();
    }
}

/// The reporter for this run, chosen from an **injected** flag.
///
/// `is_terminal` is supplied by the CLI from `std::io::IsTerminal` at the one
/// production call site. It is deliberately not read here: an ambient
/// environment read inside a constructor is exactly what makes a test
/// non-hermetic, and this project's convention is to inject the fact — the same
/// reason `Cache::at` exists beside `Cache::for_vendor`.
pub fn reporter(is_terminal: bool) -> Box<dyn Progress> {
    if is_terminal {
        Box::new(Terminal::default())
    } else {
        Box::new(Plain::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drive(reporter: &mut dyn Progress) {
        reporter.start(3, 300);
        reporter.asset_done(0, "pack-aa.bin", 100);
        reporter.asset_done(1, "pack-bb.bin", 100);
        reporter.finish();
    }

    #[test]
    fn the_renderer_is_a_pure_function_of_the_counters() {
        assert_eq!(
            render(0, 3, 0, 3 * 1024 * 1024),
            "uploading 0/3 assets — 0 B of 3.0 MiB"
        );
        assert_eq!(
            render(2, 3, 2 * 1024 * 1024, 3 * 1024 * 1024),
            "uploading 2/3 assets — 2.0 MiB of 3.0 MiB"
        );
        assert_eq!(
            render(1, 1, 512, 512),
            "uploading 1/1 asset — 512 B of 512 B"
        );
        // Pure: same arguments, same string, no clock and no environment.
        assert_eq!(render(1, 2, 5, 9), render(1, 2, 5, 9));
    }

    #[test]
    fn the_terminal_reporter_rewrites_one_line_and_ends_it_once() {
        let mut out = Vec::new();
        drive(&mut Terminal::to(&mut out));
        let written = String::from_utf8(out).unwrap();

        assert_eq!(
            written,
            "\ruploading 0/3 assets — 0 B of 300 B\
             \ruploading 1/3 assets — 100 B of 300 B\
             \ruploading 2/3 assets — 200 B of 300 B\n"
        );
        assert_eq!(written.matches('\n').count(), 1, "one newline, from finish");
    }

    /// D6: the macOS menu bar captures this command's output as a subprocess,
    /// and a carriage return or an escape sequence makes that capture
    /// unreadable.
    #[test]
    fn the_non_terminal_reporter_emits_plain_lines_and_nothing_else() {
        let mut out = Vec::new();
        drive(&mut Plain::to(&mut out));
        let written = String::from_utf8(out).unwrap();

        assert!(!written.contains('\r'), "no carriage returns: {written:?}");
        assert!(
            !written.contains('\x1b'),
            "no escape sequences: {written:?}"
        );
        assert!(written.ends_with('\n'));
        let lines: Vec<&str> = written.lines().collect();
        assert_eq!(lines.len(), 4, "one per asset, plus start and the summary");
        assert_eq!(lines[2], "uploading 2/3 assets — 200 B of 300 B");
        assert!(
            lines[3].contains("done"),
            "a final summary line: {:?}",
            lines[3]
        );
    }

    /// T-4-25: the counters are the whole of what is rendered. The asset name
    /// reaches `asset_done` and must not reach the output — it is a content
    /// address today, and a reporter that prints its argument is one refactor
    /// away from printing something else.
    #[test]
    fn nothing_the_uploader_passes_by_name_reaches_the_output() {
        let secret = "github_pat_not_a_real_token";
        let mut out = Vec::new();
        let mut plain = Plain::to(&mut out);
        plain.start(1, 10);
        plain.asset_done(0, secret, 10);
        plain.finish();
        let written = String::from_utf8(out).unwrap();
        assert!(!written.contains(secret), "{written}");
        assert!(!written.contains("github_pat"), "{written}");
    }

    /// The choice is an injected flag, never an ambient `IsTerminal` read: the
    /// project's convention is to inject the fact so the test is hermetic.
    #[test]
    fn the_reporter_is_chosen_from_an_injected_flag() {
        let mut tty = reporter(true);
        let mut piped = reporter(false);
        // Both satisfy the same trait, which is why `upload::run` needs no
        // branch. Driving them proves neither panics on the real streams.
        tty.start(0, 0);
        tty.finish();
        piped.start(0, 0);
        piped.finish();
    }
}
