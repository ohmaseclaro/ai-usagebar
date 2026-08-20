//! GitHub transport for the encrypted sync bundle.
//!
//! **Zero bytes leave the machine from this module in Phase 3** (D-05). It
//! authenticates, resolves the configured repository, and verifies that the
//! repository reports itself private — nothing more. That is what makes the
//! safety gate provably *prior to* the first byte rather than merely sequenced
//! before it: [`Client`] exposes exactly one request method, a `GET`, and there
//! is no type in this module that can carry a request body.
//!
//! **Phase 4 changed what is true, so it changed what the guard proves.**
//! [`Client`] now has six body-carrying methods — they live in [`write`], in an
//! inherent `impl` block a sibling module is allowed to open. The guard at the
//! bottom of this file therefore no longer claims `Client` cannot send a body;
//! it claims that every request body in this directory lives in `write.rs`, and
//! it fails on a body call site in any other file here.
//!
//! Layout — one file per plan, so parallel work never collides:
//! - [`http`] — the frozen [`GithubError`](http::GithubError) taxonomy, the
//!   classifier, and the actionable message table (plan 3-03).
//! - [`token`] — D-02's resolution order behind an injected chain.
//! - `keychain` — macOS token storage (plan 3-02); compiled on macOS only.
//! - [`gate`] — [`RepoFacts`](gate::RepoFacts), the private-repo assertion, and
//!   the [`PushClearance`](gate::PushClearance) it alone can mint (plan 3-04).
//! - [`pairing`] — the mode-0600 record and the drift check (plan 3-04).
//! - [`setup`] — the `ai-usagebar sync setup` flow (plan 3-07).
//! - [`write`] — the six write verbs and the shared retry helper (plan 4-01).
//!   The **only** file in the crate that sends a request body, and the only one
//!   that can delete remote data.

pub mod gate;
pub mod http;
#[cfg(target_os = "macos")]
pub mod keychain;
pub mod pairing;
pub mod setup;
pub mod token;
pub mod write;

use std::fmt;

use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderValue, USER_AGENT};
use zeroize::Zeroizing;

use crate::error::{AppError, Result};
use crate::vendor::{
    HTTP_CLIENT_TIMEOUT, MAX_BODY_BYTES, read_body_capped, same_origin_redirect_policy,
};

use token::TokenSource;

/// Sent on every request. GitHub rate-limits hard, and rejects outright, a
/// request without a `User-Agent`.
const UA: &str = concat!("ai-usagebar/", env!("CARGO_PKG_VERSION"));
/// The API version this code was written against. Pinning it means a future
/// default cannot silently reshape a response we assert on.
const API_VERSION: &str = "2022-11-28";
/// The `Accept` every JSON endpoint takes. `write.rs` uses it for five of its
/// six verbs; the asset download is the one that asks for bytes instead.
pub(crate) const ACCEPT_JSON: HeaderValue = HeaderValue::from_static("application/vnd.github+json");

/// Both GitHub hosts, injected so a test can point them at one mockito server.
#[derive(Debug, Clone)]
pub struct Endpoints {
    /// `https://api.github.com` in production.
    pub api_base: String,
    /// `https://uploads.github.com` in production — a **separate host** from
    /// [`api_base`](Endpoints::api_base).
    ///
    /// Nothing in Phase 3 reads this field: D-05 forbids an upload here. It
    /// exists from day one anyway, because a Phase 4 that hard-codes the upload
    /// host at its call site has an untestable upload path.
    pub uploads_base: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            api_base: "https://api.github.com".into(),
            uploads_base: "https://uploads.github.com".into(),
        }
    }
}

/// The `owner/name` from `[sync] repo`. Named by the user, never guessed (D-01).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoRef {
    pub owner: String,
    pub name: String,
}

impl RepoRef {
    /// Exactly one `owner/name` pair, both segments drawn from GitHub's own
    /// character set.
    ///
    /// This value is interpolated into a URL path, so the check is a strict
    /// allow-list rather than an escape: a segment carrying whitespace, a path
    /// separator, a control character, or a `?`/`#` could otherwise walk out of
    /// the path it was substituted into (T-3-03).
    pub fn parse(raw: &str) -> Result<RepoRef> {
        let bad = |why: &str| {
            AppError::Other(format!(
                "{why} — expected exactly \"owner/name\", for example \"octocat/ai-usagebar-sync\""
            ))
        };
        let mut parts = raw.split('/');
        let (Some(owner), Some(name), None) = (parts.next(), parts.next(), parts.next()) else {
            return Err(bad(
                format!("{raw:?} is not a repository reference").as_str()
            ));
        };
        for segment in [owner, name] {
            if segment.is_empty() {
                return Err(bad(format!("{raw:?} has an empty half").as_str()));
            }
            if segment == "." || segment == ".." {
                return Err(bad(
                    format!("{raw:?} contains a path-traversal segment").as_str()
                ));
            }
            if let Some(c) = segment
                .chars()
                .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')))
            {
                return Err(bad(
                    format!("{raw:?} contains {c:?}, which cannot appear in a GitHub owner or repository name").as_str(),
                ));
            }
        }
        Ok(RepoRef {
            owner: owner.to_owned(),
            name: name.to_owned(),
        })
    }
}

impl fmt::Display for RepoRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.owner, self.name)
    }
}

/// An authenticated, **read-only** GitHub client.
///
/// There is no `post`, `put`, `patch`, or multipart method here and no call site
/// that hands this type a request body. That absence *is* D-05 — the type itself
/// cannot upload — and it is enforced by a test in this file rather than by
/// review, so a write verb added here fails the suite instead of shipping.
///
/// `Clone` because plan 4-03 uploads four packs concurrently and each task takes
/// an owned client; the inner `reqwest::Client` is an `Arc` handle, so a clone
/// shares one connection pool rather than opening a second.
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    endpoints: Endpoints,
    token: Zeroizing<String>,
    source: TokenSource,
}

/// Hand-written: the derived one would print the token.
impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("endpoints", &self.endpoints)
            .field("token", &format_args!("<{}>", self.source.label()))
            .finish()
    }
}

impl Client {
    pub fn new(
        endpoints: &Endpoints,
        token: Zeroizing<String>,
        source: TokenSource,
    ) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(HTTP_CLIENT_TIMEOUT)
            // T-3-02: without this a bearer token follows a cross-host redirect
            // and is replayed at whatever the redirect names.
            .redirect(same_origin_redirect_policy())
            .build()
            .map_err(|e| AppError::Transport(e.to_string()))?;
        Ok(Self {
            http,
            endpoints: endpoints.clone(),
            token,
            source,
        })
    }

    pub fn source(&self) -> TokenSource {
        self.source
    }

    pub fn endpoints(&self) -> &Endpoints {
        &self.endpoints
    }

    /// **The one place the bearer token becomes a header value**, for every
    /// request the crate makes — reads here, writes in [`write`].
    ///
    /// The header is marked sensitive, so `reqwest`'s own `Debug` redacts it.
    /// `url` is fully built by the caller; nothing interpolated into it comes
    /// from a remote response.
    ///
    /// It returns a builder rather than sending, which is what lets `write.rs`
    /// attach a body without a second copy of this. The guard test below is what
    /// keeps that body from being attached anywhere else.
    ///
    /// `accept` is a parameter rather than a constant because
    /// [`reqwest::RequestBuilder::header`] *appends*: a caller that wanted
    /// `application/octet-stream` and set it afterwards would send two `Accept`
    /// headers, and the asset-download endpoint answers the first one.
    fn authed(
        &self,
        method: reqwest::Method,
        url: &str,
        accept: HeaderValue,
    ) -> Result<reqwest::RequestBuilder> {
        let mut auth = HeaderValue::from_str(&Zeroizing::new(format!("Bearer {}", &*self.token)))
            .map_err(|_| {
            AppError::Credentials("the stored sync token is not a valid HTTP header value".into())
        })?;
        auth.set_sensitive(true);
        Ok(self
            .http
            .request(method, url)
            .header(AUTHORIZATION, auth)
            .header(ACCEPT, accept)
            .header(
                "X-GitHub-Api-Version",
                HeaderValue::from_static(API_VERSION),
            )
            .header(USER_AGENT, HeaderValue::from_static(UA)))
    }

    /// The single **read** call site for the whole crate.
    ///
    /// `path` is absolute and already validated — every caller builds it from a
    /// parsed [`RepoRef`]. The body is capped at
    /// [`MAX_BODY_BYTES`](crate::vendor::MAX_BODY_BYTES): every response on this
    /// path is a few kilobytes of JSON (T-3-06). `Vec<u8>` because that is what
    /// [`read_body_capped`](crate::vendor::read_body_capped) returns.
    pub async fn get_json(&self, path: &str) -> Result<(reqwest::StatusCode, HeaderMap, Vec<u8>)> {
        let url = format!("{}{}", self.endpoints.api_base.trim_end_matches('/'), path);
        let resp = self
            .authed(reqwest::Method::GET, &url, ACCEPT_JSON)?
            .send()
            .await
            .map_err(|e| AppError::from(http::from_transport(&e)))?;

        let status = resp.status();
        let headers = resp.headers().clone();
        let body = read_body_capped(resp, MAX_BODY_BYTES).await?;
        Ok((status, headers, body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client_at(base: &str) -> Client {
        Client::new(
            &Endpoints {
                api_base: base.into(),
                uploads_base: base.into(),
            },
            Zeroizing::new("github_pat_fixture_not_a_real_token".into()),
            TokenSource::Env,
        )
        .unwrap()
    }

    /// **Every request body in this directory lives in `write.rs`.**
    ///
    /// This replaces plan 3-01's guard, which read `include_str!("mod.rs")` and
    /// proved that [`Client`] had no body-carrying method. Rust lets a sibling
    /// module open an inherent `impl Client`, so plan 4-01 gave `Client` six
    /// such methods *next door* and 3-01's guard stayed green while the sentence
    /// it was named for stopped being true. A green test making a false claim is
    /// worse than no test, so the claim moved rather than the code.
    ///
    /// What it buys, now that bytes do leave the machine: an upload or a delete
    /// added anywhere in this directory but `write.rs` fails the suite. That
    /// matters because `write.rs` is the file reviewed *as* the outbound path —
    /// it is where the retry discipline, the body caps, and the one destructive
    /// verb are, and a seventh write verb grown quietly in `gate.rs` would
    /// inherit none of it.
    ///
    /// A directory walk rather than a per-file `include_str!`, so a file added
    /// to this module later is covered without anyone remembering to add it.
    /// Reading the crate's own source is hermetic: `CARGO_MANIFEST_DIR` is a
    /// compile-time constant, so this passes inside `makepkg`'s `check()` and
    /// depends on no working directory.
    #[test]
    fn every_request_body_in_this_directory_lives_in_write_rs() {
        // `.delete(` is in the list even though a DELETE carries no body: it is
        // the crate's only destructive remote verb, and it belongs beside the
        // uploads for the same reason.
        const BODY_CALL_SITES: [&str; 7] = [
            ".post(",
            ".put(",
            ".patch(",
            ".delete(",
            ".body(",
            ".multipart(",
            ".json(",
        ];

        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/sync/github");
        let mut scanned = 0usize;
        let mut skipped = 0usize;
        let mut saw_mod = false;

        for entry in std::fs::read_dir(&dir).expect("src/sync/github must exist") {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            if path.file_name().is_some_and(|n| n == "write.rs") {
                skipped += 1;
                continue;
            }
            scanned += 1;
            let text = std::fs::read_to_string(&path).unwrap();
            // Everything before the test module is the shipped surface — and it
            // is also what keeps *this* test's own needle list from tripping it.
            let production = text.split("#[cfg(test)]").next().unwrap();
            if path.file_name().is_some_and(|n| n == "mod.rs") {
                saw_mod = production.contains("pub async fn get_json");
            }
            for needle in BODY_CALL_SITES {
                assert!(
                    !production.contains(needle),
                    "REQUEST BODIES LIVE ONLY IN write.rs: `{needle}` appeared in {}. \
                     Every outbound body and every remote delete goes through \
                     src/sync/github/write.rs, which is where the retry discipline (D7), \
                     the response-size caps, and the token-free redirect hop are. A write \
                     path grown anywhere else inherits none of that and will not be \
                     reviewed as one. Move it into write.rs.",
                    path.display()
                );
            }
        }

        // Non-vacuity, both ways: a guard that asserts an absence must prove it
        // looked at something, and that the one file it excludes was actually
        // there to exclude. Without these a renamed directory reports green.
        assert_eq!(
            skipped, 1,
            "write.rs must be present and excluded exactly once"
        );
        assert!(scanned >= 5, "only {scanned} files walked under {dir:?}");
        assert!(saw_mod, "mod.rs's production half was not located");
    }

    #[test]
    fn a_well_formed_reference_parses_and_renders_back() {
        let repo = RepoRef::parse("octocat.dev/ai-usagebar_sync-1").unwrap();
        assert_eq!(repo.owner, "octocat.dev");
        assert_eq!(repo.name, "ai-usagebar_sync-1");
        assert_eq!(repo.to_string(), "octocat.dev/ai-usagebar_sync-1");
    }

    /// T-3-03: the value is interpolated into a URL path.
    #[test]
    fn everything_that_is_not_one_owner_slash_name_pair_is_refused_by_shape() {
        for bad in [
            "",
            "name",
            "a/b/c",
            "a/",
            "/b",
            "own er/name",
            "owner/na\nme",
            "owner/../etc",
            "owner/name?x=1",
            "owner/name#frag",
            "own%2Fer/name",
        ] {
            let err = RepoRef::parse(bad).expect_err("that is not one owner/name pair");
            assert!(err.to_string().contains("owner/name"), "{bad:?}: {err}");
        }
    }

    /// T-3-01: the token must not be reachable through a rendering of the client.
    #[test]
    fn the_clients_debug_reports_the_token_source_and_never_the_token() {
        let rendered = format!("{:?}", client_at("https://example.invalid"));
        assert!(
            !rendered.contains("github_pat_fixture_not_a_real_token"),
            "{rendered}"
        );
        assert!(rendered.contains("env"), "{rendered}");
        assert!(rendered.contains("example.invalid"), "{rendered}");
    }

    #[tokio::test]
    async fn get_json_returns_the_status_headers_and_capped_body() {
        let mut server = mockito::Server::new_async().await;
        let m = server
            .mock("GET", "/repos/o/n")
            .match_header("accept", "application/vnd.github+json")
            .match_header("x-github-api-version", API_VERSION)
            .match_header(
                "authorization",
                "Bearer github_pat_fixture_not_a_real_token",
            )
            .match_header("user-agent", UA)
            .with_status(200)
            .with_header("x-ratelimit-remaining", "4999")
            .with_body(r#"{"ok":true}"#)
            .create_async()
            .await;

        let (status, headers, body) = client_at(&server.url())
            .get_json("/repos/o/n")
            .await
            .unwrap();
        assert_eq!(status, 200);
        assert_eq!(headers["x-ratelimit-remaining"], "4999");
        assert_eq!(body, br#"{"ok":true}"#);
        m.assert_async().await;
    }

    /// A dead port is the cheapest transport failure, and it must not become a
    /// status-shaped error.
    #[tokio::test]
    async fn an_unreachable_host_becomes_a_transport_failure() {
        let err = client_at("http://127.0.0.1:1")
            .get_json("/repos/o/n")
            .await
            .expect_err("nothing listens there");
        assert!(err.is_transient(), "{err}");
    }
}
