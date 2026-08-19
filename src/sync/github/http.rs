//! The GitHub failure taxonomy.
//!
//! The *type* is frozen by plan 3-01 and every later plan matches on it, so the
//! variant set below is closed for Phase 3: plan 3-03 fills in the classifier's
//! arithmetic and the message text, and adds no variant. [`Conflict`] is unreached
//! in this phase and exists so Phase 4's compare-and-swap pointer write does not
//! have to widen an enum three other files match on.
//!
//! [`Conflict`]: GithubError::Conflict

use std::time::Duration;

use chrono::{DateTime, Utc};
use reqwest::StatusCode;
use reqwest::header::HeaderMap;

use crate::error::AppError;

/// Everything a GitHub request can fail as. **Seven variants, frozen.**
#[derive(Debug, thiserror::Error)]
pub enum GithubError {
    /// 401 — the token is missing, revoked, or expired.
    #[error("GitHub rejected the token (401): {message}")]
    Unauthorized { message: String },

    /// 403 or 429 carrying rate-limit headers. `retry_after` is how long to
    /// wait, computed by plan 3-03 from `retry-after` / `x-ratelimit-reset`.
    #[error("GitHub rate-limited this token: {message}")]
    RateLimited {
        retry_after: Duration,
        message: String,
    },

    /// 403 *without* rate-limit headers — a permission the token does not carry.
    #[error("GitHub refused this request (403): {message}")]
    Forbidden { message: String },

    /// 404 — the repository is absent, or the token is not scoped to it.
    /// GitHub deliberately returns the same status for both.
    #[error("{message}")]
    NotFound { message: String },

    /// 409 — a compare-and-swap precondition failed. Unreached in Phase 3.
    #[error("GitHub reported a conflicting remote state (409): {message}")]
    Conflict { message: String },

    /// Any other non-2xx.
    #[error("GitHub returned an unexpected HTTP {status}: {message}")]
    Unexpected { status: u16, message: String },

    /// DNS, TLS, connect, or timeout — the request never got a status.
    #[error("could not reach GitHub: {message}")]
    Transport { message: String },
}

/// Every variant is a failure. None of them may land on a success-shaped value.
impl From<GithubError> for AppError {
    fn from(err: GithubError) -> Self {
        let body = err.to_string();
        match err {
            GithubError::Unauthorized { .. } => AppError::Credentials(body),
            GithubError::RateLimited { .. } => AppError::Http { status: 429, body },
            GithubError::Forbidden { .. } => AppError::Http { status: 403, body },
            GithubError::NotFound { .. } => AppError::Http { status: 404, body },
            GithubError::Conflict { .. } => AppError::Http { status: 409, body },
            GithubError::Unexpected { status, .. } => AppError::Http { status, body },
            GithubError::Transport { .. } => AppError::Transport(body),
        }
    }
}

/// Turn a non-2xx response into the right variant.
///
/// `now` is a parameter and never `Utc::now()` inside — the rate-limit reset
/// header is an absolute instant, and a test that cannot pin the clock cannot
/// assert the delay. Same shape as `antigravity::parse_cache_at`.
///
/// **Plan 3-01 fills only the statuses its tracer exercises** — 401, 404, and
/// the catch-all. Plan 3-03 owns this file next and adds the 403/429 rate-limit
/// arithmetic and the 409 arm behind this unchanged signature.
pub fn classify(
    status: StatusCode,
    headers: &HeaderMap,
    body: &[u8],
    now: DateTime<Utc>,
) -> GithubError {
    // Both are plan 3-03's inputs: `headers` carries `retry-after` /
    // `x-ratelimit-reset`, and `now` is what turns the latter into a delay.
    let _ = (headers, now);

    let message = message_of(body);
    match status.as_u16() {
        401 => GithubError::Unauthorized { message },
        404 => GithubError::NotFound { message },
        status => GithubError::Unexpected { status, message },
    }
}

/// The one place a `reqwest` failure becomes a [`GithubError`]. `Client::get_json`
/// is its only call site, so plan 3-03 can give the transport arm actionable text
/// without touching any other file.
pub fn from_transport(e: &reqwest::Error) -> GithubError {
    GithubError::Transport {
        message: e.to_string(),
    }
}

/// The user-facing line for a failure: what happened *and* what to do about it.
///
/// Plan 3-01 renders the error's own display text; plan 3-03 replaces this with
/// the per-variant table that names each fix (D-06).
pub fn actionable(err: &GithubError) -> String {
    err.to_string()
}

/// GitHub's own `{"message": …}` when the body carries one, else the body as
/// lossy text. Bounded so a hostile or confused remote cannot flood a terminal.
fn message_of(body: &[u8]) -> String {
    const MAX: usize = 200;
    let text = serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(str::to_owned))
        .unwrap_or_else(|| String::from_utf8_lossy(body).into_owned());
    let text = text.trim();
    match text.char_indices().nth(MAX) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None if text.is_empty() => "(no message)".into(),
        None => text.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(status: u16, body: &str) -> GithubError {
        classify(
            StatusCode::from_u16(status).unwrap(),
            &HeaderMap::new(),
            body.as_bytes(),
            DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
        )
    }

    #[test]
    fn the_statuses_this_tracer_exercises_classify_distinctly() {
        assert!(matches!(
            at(401, r#"{"message":"Bad credentials"}"#),
            GithubError::Unauthorized { message } if message == "Bad credentials"
        ));
        assert!(matches!(at(404, "{}"), GithubError::NotFound { .. }));
        assert!(matches!(
            at(500, "boom"),
            GithubError::Unexpected { status: 500, .. }
        ));
    }

    /// A hostile remote does not get to fill the terminal.
    #[test]
    fn an_oversized_body_is_truncated_rather_than_echoed_whole() {
        let huge = "x".repeat(10_000);
        let err = at(500, &huge);
        assert!(err.to_string().len() < 400, "{err}");
    }

    /// T-3-18 in advance: no variant may convert into something a caller could
    /// mistake for success.
    #[test]
    fn every_variant_converts_into_a_failure_app_error() {
        let all = [
            GithubError::Unauthorized {
                message: "m".into(),
            },
            GithubError::RateLimited {
                retry_after: Duration::from_secs(60),
                message: "m".into(),
            },
            GithubError::Forbidden {
                message: "m".into(),
            },
            GithubError::NotFound {
                message: "m".into(),
            },
            GithubError::Conflict {
                message: "m".into(),
            },
            GithubError::Unexpected {
                status: 500,
                message: "m".into(),
            },
            GithubError::Transport {
                message: "m".into(),
            },
        ];
        for err in all {
            let text = actionable(&err);
            let app: AppError = err.into();
            assert!(!text.is_empty());
            assert!(matches!(
                app,
                AppError::Credentials(_) | AppError::Http { .. } | AppError::Transport(_)
            ));
        }
    }
}
