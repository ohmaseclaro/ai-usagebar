//! Progress reporting for a long push, at **asset** granularity (D6).
//!
//! A first push moves ~115 MB in a handful of large assets, so "asset i of n,
//! bytes done of total" is the whole of what a user needs. There is deliberately
//! no per-chunk hook and one must not be added: 5,000 chunk callbacks per push
//! is chatter, and the thing a user is waiting on is the upload, which happens
//! one asset at a time.
//!
//! Plan 4-01 defines the trait and [`Silent`]. Plan 4-03 owns this file
//! afterwards and adds the terminal and non-terminal implementations behind it,
//! choosing between them from an injected `is_terminal` flag.

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
