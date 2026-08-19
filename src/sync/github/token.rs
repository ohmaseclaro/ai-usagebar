//! Where the sync token comes from, in D-02's order:
//!
//! 1. `AI_USAGEBAR_SYNC_TOKEN` — the explicit override, and the one that works
//!    over SSH with no session keyring, which is exactly the headless-restore
//!    case this feature exists for.
//! 2. the macOS Keychain item `ai-usagebar-sync-token`
//! 3. `~/.config/ai-usagebar/sync-token`, mode 0600
//! 4. `gh auth token`, when `gh` happens to be installed — convenience only
//!
//! Plan 3-01 fills 1 and 3, which need no platform code. Plan 3-02 supplies the
//! two closures for 2 and 4 and owns the write path; the field types below do
//! not change again.
//!
//! The value is a [`Zeroizing<String>`] end to end, and **no type here derives
//! `Debug` while holding it**. It is never logged, not even a prefix: only its
//! [`TokenSource`] is ever reported.

use std::path::PathBuf;

use zeroize::Zeroizing;

use crate::error::{AppError, Result};

/// Which of D-02's four the token actually came from. This — never the value —
/// is what gets printed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenSource {
    Env,
    Keychain,
    File,
    GhCli,
}

impl TokenSource {
    pub fn label(&self) -> &'static str {
        match self {
            TokenSource::Env => "env",
            TokenSource::Keychain => "Keychain",
            TokenSource::File => "file",
            TokenSource::GhCli => "gh",
        }
    }
}

/// The four sources, injected. The two closures are the seam that keeps every
/// test off the real Keychain and out of a subprocess.
///
/// No `Debug`: `env_value` holds the token itself.
#[derive(Default)]
pub struct TokenChain {
    pub env_value: Option<String>,
    #[allow(clippy::type_complexity)]
    pub keychain: Option<Box<dyn Fn() -> Result<Option<String>>>>,
    pub file_path: Option<PathBuf>,
    #[allow(clippy::type_complexity)]
    pub gh: Option<Box<dyn Fn() -> Result<Option<String>>>>,
}

impl TokenChain {
    /// The only function in this module that touches the environment or a real
    /// path. **No test calls it** — the AUR `check()` runs `cargo test` on
    /// installers' machines.
    pub fn production() -> TokenChain {
        TokenChain {
            env_value: std::env::var("AI_USAGEBAR_SYNC_TOKEN").ok(),
            keychain: None, // plan 3-02
            file_path: crate::config::resolved_path()
                .and_then(|p| p.parent().map(|d| d.join("sync-token"))),
            gh: None, // plan 3-02
        }
    }
}

/// Walk the chain in D-02's order and return the first non-empty value with its
/// source. Trailing whitespace is trimmed: an editor-written token file ends in
/// a newline, and a newline in a header value is a request-splitting bug.
pub fn resolve(chain: &TokenChain) -> Result<(Zeroizing<String>, TokenSource)> {
    if let Some(found) = usable(chain.env_value.clone()) {
        return Ok((found, TokenSource::Env));
    }
    if let Some(read) = &chain.keychain
        && let Some(found) = usable(read()?)
    {
        return Ok((found, TokenSource::Keychain));
    }
    if let Some(path) = &chain.file_path {
        match std::fs::read_to_string(path) {
            Ok(raw) => {
                if let Some(found) = usable(Some(raw)) {
                    return Ok((found, TokenSource::File));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            // An unreadable-but-present token file is a misconfiguration the
            // user must see, not a source to silently skip past.
            Err(e) => return Err(AppError::io_at(path, e)),
        }
    }
    if let Some(read) = &chain.gh
        && let Some(found) = usable(read()?)
    {
        return Ok((found, TokenSource::GhCli));
    }

    Err(AppError::Credentials(format!(
        "no GitHub token for sync. Supply one of:\n\
         \x20 - AI_USAGEBAR_SYNC_TOKEN in the environment\n\
         \x20 - {}, mode 0600\n\
         \x20 - `gh auth login`, if you already use the GitHub CLI\n\
         It must be a fine-grained PAT scoped to the single sync repository, \
         with Contents: read/write and Metadata: read — and nothing else.",
        chain
            .file_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "~/.config/ai-usagebar/sync-token".into())
    )))
}

/// Trim, and treat an empty result as "this source had nothing".
fn usable(raw: Option<String>) -> Option<Zeroizing<String>> {
    let raw = Zeroizing::new(raw?);
    let trimmed = Zeroizing::new(raw.trim().to_owned());
    (!trimmed.is_empty()).then_some(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const FIXTURE: &str = "github_pat_fixture_not_a_real_token";

    #[test]
    fn the_environment_wins_and_reports_itself_as_the_source() {
        let chain = TokenChain {
            env_value: Some(format!("{FIXTURE}\n")),
            file_path: Some(PathBuf::from("/nonexistent/sync-token")),
            ..TokenChain::default()
        };
        let (token, source) = resolve(&chain).unwrap();
        assert_eq!(token.as_str(), FIXTURE);
        assert_eq!(source, TokenSource::Env);
    }

    #[test]
    fn a_token_file_answers_when_the_environment_is_unset() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("sync-token");
        std::fs::write(&path, format!("  {FIXTURE}  \n")).unwrap();

        let chain = TokenChain {
            file_path: Some(path),
            ..TokenChain::default()
        };
        let (token, source) = resolve(&chain).unwrap();
        assert_eq!(token.as_str(), FIXTURE);
        assert_eq!(source, TokenSource::File);
    }

    /// An empty file is not a token — fall through, then say how to supply one.
    #[test]
    fn an_exhausted_chain_names_every_way_to_supply_a_token() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("sync-token");
        std::fs::write(&path, "\n\n").unwrap();

        let err = resolve(&TokenChain {
            file_path: Some(path),
            ..TokenChain::default()
        })
        .expect_err("nothing in the chain answered");
        let text = err.to_string();
        assert!(text.contains("AI_USAGEBAR_SYNC_TOKEN"), "{text}");
        assert!(text.contains("sync-token"), "{text}");
        assert!(text.contains("Contents: read/write"), "{text}");
    }

    /// T-3-01: the value is never rendered, not even through a derived `Debug`.
    #[test]
    fn no_rendering_of_the_resolved_token_type_contains_the_token() {
        let chain = TokenChain {
            env_value: Some(FIXTURE.into()),
            ..TokenChain::default()
        };
        let (token, source) = resolve(&chain).unwrap();
        assert!(!format!("{source:?}").contains(FIXTURE));
        assert!(!format!("{:?}", source.label()).contains(FIXTURE));
        // `Zeroizing<String>` is `Debug` by way of `String`, which is exactly
        // why nothing that *holds* one derives `Debug` — see `Client`.
        drop(token);
    }
}
