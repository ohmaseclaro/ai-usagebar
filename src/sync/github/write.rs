//! **The only file in the crate that sends a request body, and the only one
//! that can delete remote data.**
//!
//! Everything outbound goes through here: the release the bundle lives in, the
//! pack assets, the keyfile asset, and the Contents-API pointer whose
//! compare-and-swap `PUT` is the format's single linearization point. A guard
//! test in [`super`] walks this directory and fails if a request body appears in
//! any other file, because this is the file that gets reviewed *as* an upload
//! path — D7's retry discipline, the response-size caps, and the token-free
//! redirect hop all live here and nowhere else.
//!
//! Nothing in this module logs, prints, or formats a header map. The bearer
//! token is attached by [`Client::authed`](super::Client), which is the one
//! place in the crate that turns it into a header value; no code below ever
//! names it.
//!
//! **Every verb that can write takes a [`gate::Pushing`](super::gate::Pushing).**
//! That is plan 3-08's contract, and it is what makes the private-repo gate
//! structurally prior to the first byte rather than merely earlier in a
//! narrative: a `Pushing` exists only as the output of
//! `assert_pushable(…)?.0.spend(now)?`, so a call that reaches this file at all
//! proves a visibility check that was fresh within `MAX_CLEARANCE_AGE` **and was
//! about this repository** — every verb below opens with `permit.covers(repo)?`,
//! so the permit proves its subject as well as its instant.
//!
//! It is taken by **reference**, not by value, and the distinction is deliberate.
//! One push legitimately writes many times under one clearance — a release, `n`
//! pack assets, then the pointer — so by-value consumption at this level would
//! mean either one write per gate call or a permit handed back and forth. The
//! by-value consumption lives where "once" is the real semantics: at the entry
//! points in `src/sync/push/`, each of which mints its own permit and holds it
//! for exactly the span of writes it is gating. The flip gets a **second**
//! permit, minted by the re-gate after the uploads, which is D3.
//!
//! The three read verbs — [`Client::list_assets`], [`Client::download_asset`],
//! [`Client::get_contents`] — take no permit. Reading a private repository is
//! what the gate exists to *establish*, not something it needs to authorise.
//!
//! Owned by plan 4-01. Its six signatures are frozen: plans 4-03 through 4-06
//! build against them in parallel worktrees.

use std::future::Future;
use std::time::Duration;

use base64::Engine;
use chrono::{DateTime, Utc};
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};
use crate::vendor::{HTTP_CLIENT_TIMEOUT, MAX_BODY_BYTES, read_body_capped};

use super::gate::Pushing;
use super::http::{self, GithubError};
use super::{ACCEPT_JSON, Client, RepoRef};

const B64: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::STANDARD;

/// Upper bound on a downloaded release asset.
///
/// Comfortably above [`pack::PACK_MAX`](crate::sync::pack::PACK_MAX) = 48 MiB,
/// and checked against `Content-Length` *before* allocating as well as while
/// reading, so a lying header cannot exhaust memory either (T-4-05).
///
/// [`MAX_BODY_BYTES`] is for kilobytes of JSON and is deliberately not reused
/// here. Equally, this cap is not the one to relax for Phase 5's bundle
/// download: that streams to a file instead of buffering.
pub const MAX_ASSET_BYTES: u64 = 64 * 1024 * 1024;

/// Upper bound on a Contents-API response. One mebibyte is the Contents API's
/// own full-support threshold, and far above anything this format writes.
pub const MAX_POINTER_BYTES: u64 = 1024 * 1024;

/// The `state` a release asset reports once its body has finished landing.
/// Compared as a string rather than parsed into an enum — see [`Asset::state`].
pub const ASSET_STATE_UPLOADED: &str = "uploaded";

/// How many times a retryable failure is re-attempted before the last error is
/// returned unchanged.
const RETRY_ATTEMPTS: u32 = 4;

/// Assets per page on the listing endpoint — GitHub's documented maximum.
const ASSETS_PER_PAGE: u32 = 100;

/// A release holds at most 1,000 assets, so ten full pages is the whole of it.
/// An unbounded pagination loop against a hostile remote is a denial of service
/// (T-4-06).
const MAX_ASSET_PAGES: u32 = 1_000 / ASSETS_PER_PAGE;

/// The error type the retry helper reasons about. The public verbs convert into
/// [`AppError`] at their edge, which is what renders Phase 3's actionable text.
type GhResult<T> = std::result::Result<T, GithubError>;

/// One release asset, as GitHub describes it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Asset {
    pub id: u64,
    pub name: String,
    pub size: u64,
    /// `"uploaded"` once the body has landed; other values are undocumented.
    ///
    /// A `String` and not an enum, deliberately: the research rates the state
    /// transitions MEDIUM confidence, and an enum that errored on an
    /// undocumented value would turn a surprise into an outage. Compare against
    /// [`ASSET_STATE_UPLOADED`].
    pub state: String,
    /// **Not decoration, and not optional.** This is the field prune's grace
    /// window is computed from.
    ///
    /// Without it prune cannot tell a pack that no snapshot references because
    /// it is garbage from a pack no snapshot references *yet* because another
    /// machine uploaded it thirty seconds ago and has not flipped. Deleting the
    /// second produces a live snapshot pointing at deleted data — D2's single
    /// worst outcome. It is frozen here because [`Asset`] is a frozen type and
    /// plan 4-05 cannot add a field to it.
    pub created_at: DateTime<Utc>,
    /// GitHub does not populate this on every asset. Nothing in this phase
    /// depends on it; plan 4-03's live probe records whether it arrives.
    #[serde(default)]
    pub digest: Option<String>,
}

// ---------------------------------------------------------------------------
// D7 in one place
// ---------------------------------------------------------------------------

/// Retry exactly the two failures that re-running the same request can fix.
///
/// `sleep` is injected, so **no test sleeps**; production passes
/// [`tokio::time::sleep`]. `op` is re-invoked from scratch on each attempt, so
/// it must be idempotent — every verb below is, which is why the one that is not
/// obviously so ([`Client::upload_asset`]) has its own already-exists arm.
///
/// Retries [`GithubError::RateLimited`] and [`GithubError::Transport`] and
/// nothing else, which is [`http::is_retryable`]'s rule rather than a second
/// one. Returns immediately on `Unauthorized`, `Forbidden`, `NotFound`, and
/// `Conflict`: a retried 401 is a slower failure, and a retried 409 would
/// overwrite the very state the precondition exists to protect (T-4-08).
///
/// On exhausting `attempts` the **last error is returned unchanged**, so the
/// caller still gets Phase 3's actionable text rather than a wrapper about
/// retries.
pub(crate) async fn with_retry<T, F, Fut, S, SFut>(
    attempts: u32,
    sleep: S,
    now: DateTime<Utc>,
    op: F,
) -> GhResult<T>
where
    F: Fn() -> Fut,
    Fut: Future<Output = GhResult<T>>,
    S: Fn(Duration) -> SFut,
    SFut: Future<Output = ()>,
{
    let mut last = None;
    for attempt in 0..attempts.max(1) {
        match op().await {
            Ok(value) => return Ok(value),
            Err(err) if http::is_retryable(&err) => {
                // A rate limit already carries the delay `retry_delay` derived
                // from its own headers; a transport failure has no headers at
                // all, so it falls to the same function's backoff rung.
                let delay = match &err {
                    GithubError::RateLimited { retry_after, .. } => *retry_after,
                    _ => http::retry_delay(&HeaderMap::new(), attempt, now),
                };
                last = Some(err);
                sleep(delay).await;
            }
            Err(err) => return Err(err),
        }
    }
    Err(last.unwrap_or_else(|| GithubError::Transport {
        message: "the request was never attempted".into(),
    }))
}

/// `with_retry` with this module's production settings.
async fn retried<T, F, Fut>(now: DateTime<Utc>, op: F) -> GhResult<T>
where
    F: Fn() -> Fut,
    Fut: Future<Output = GhResult<T>>,
{
    with_retry(RETRY_ATTEMPTS, tokio::time::sleep, now, op).await
}

// ---------------------------------------------------------------------------
// The six verbs
// ---------------------------------------------------------------------------

impl Client {
    /// Send an already-built request and read its body under `cap`.
    ///
    /// The status is returned rather than interpreted: every caller has its own
    /// non-2xx arms (a 404 that means first push, a 422 that means "already
    /// there"), and a helper that classified eagerly would have to be unwound at
    /// each of them.
    async fn send_capped(
        &self,
        req: reqwest::RequestBuilder,
        cap: u64,
    ) -> GhResult<(StatusCode, HeaderMap, Vec<u8>)> {
        let resp = req.send().await.map_err(|e| http::from_transport(&e))?;
        let status = resp.status();
        let headers = resp.headers().clone();
        // An oversized body is *not* retryable: re-issuing the request produces
        // the same oversized body. `Unexpected` carries the real status so the
        // message says what actually answered.
        let body =
            read_body_capped(resp, cap as usize)
                .await
                .map_err(|e| GithubError::Unexpected {
                    status: status.as_u16(),
                    message: e.to_string(),
                })?;
        Ok((status, headers, body))
    }

    /// [`Client::authed`](super::Client) in this module's error type.
    ///
    /// Its one failure — a stored token that is not a valid header value — is a
    /// dead credential, and never retryable.
    fn authed_gh(
        &self,
        method: Method,
        url: &str,
        accept: HeaderValue,
    ) -> GhResult<reqwest::RequestBuilder> {
        self.authed(method, url, accept)
            .map_err(|e| GithubError::Unauthorized {
                message: e.to_string(),
            })
    }

    /// `{api_base}/repos/{owner}/{name}{suffix}`.
    fn api_url(&self, repo: &RepoRef, suffix: &str) -> String {
        format!(
            "{}/repos/{}/{}{suffix}",
            self.endpoints.api_base.trim_end_matches('/'),
            repo.owner,
            repo.name
        )
    }

    /// The release the whole bundle lives in, created on first use.
    ///
    /// One published release under one fixed tag, never a draft: a draft release
    /// has no git tag until it is published, so `GET /releases/tags/{tag}`
    /// returns 404 for it and the resume scan could not find its own crashed
    /// predecessor. Atomicity comes from the pointer flip (D3), not from draft
    /// state.
    pub async fn ensure_release(
        &self,
        repo: &RepoRef,
        tag: &str,
        permit: &Pushing,
        now: DateTime<Utc>,
    ) -> Result<u64> {
        permit.covers(repo)?;
        let existing = retried(now, || async {
            let url = self.api_url(repo, &format!("/releases/tags/{tag}"));
            let (status, headers, body) = self
                .send_capped(
                    self.authed_gh(Method::GET, &url, ACCEPT_JSON)?,
                    MAX_BODY_BYTES as u64,
                )
                .await?;
            if status == StatusCode::NOT_FOUND {
                return Ok(None);
            }
            if !status.is_success() {
                return Err(http::classify(status, &headers, &body, now));
            }
            decode::<ReleaseRef>(&body).map(|r| Some(r.id))
        })
        .await?;
        if let Some(id) = existing {
            return Ok(id);
        }

        let created = retried(now, || async {
            let url = self.api_url(repo, "/releases");
            let (status, headers, body) = self
                .send_capped(
                    self.authed_gh(Method::POST, &url, ACCEPT_JSON)?
                        .json(&NewRelease {
                            tag_name: tag,
                            name: tag,
                            body: RELEASE_NOTE,
                        }),
                    MAX_BODY_BYTES as u64,
                )
                .await?;
            if status == StatusCode::UNPROCESSABLE_ENTITY {
                // GitHub will not tag a repository that has no commits, and it
                // says only "Validation Failed". An empty repository is exactly
                // what `gh repo create --private` leaves behind, so this is the
                // ordinary first-run state — and the generic 422 text sends the
                // user to the status page for something that is not an outage.
                //
                // `NotFound` because its `Display` is the message verbatim and
                // `is_retryable` never retries it, and because what GitHub
                // cannot find is a commit to tag.
                return Err(GithubError::NotFound {
                    message: format!(
                        "{}/{} has no commits yet, so GitHub will not create the release \
                     the packs hang off.\n\
                     Give it one — a README is enough — and re-run this command:\n\
                     \x20 gh api repos/{}/{}/contents/README.md -X PUT \\\n\
                     \x20   -f message=init -f content=\"$(printf '# sync' | base64)\"\n\
                     Nothing was uploaded. A repository created with `--add-readme` \
                     does not hit this.",
                        repo.owner, repo.name, repo.owner, repo.name
                    ),
                });
            }
            if !status.is_success() {
                return Err(http::classify(status, &headers, &body, now));
            }
            decode::<ReleaseRef>(&body).map(|r| r.id)
        })
        .await?;
        Ok(created)
    }

    /// Every asset on the release, following pages until a short one.
    pub async fn list_assets(
        &self,
        repo: &RepoRef,
        release_id: u64,
        now: DateTime<Utc>,
    ) -> Result<Vec<Asset>> {
        let mut all = Vec::new();
        for page in 1..=MAX_ASSET_PAGES {
            let batch: Vec<Asset> = retried(now, || async {
                let url = self.api_url(
                    repo,
                    &format!(
                        "/releases/{release_id}/assets?per_page={ASSETS_PER_PAGE}&page={page}"
                    ),
                );
                let (status, headers, body) = self
                    .send_capped(
                        self.authed_gh(Method::GET, &url, ACCEPT_JSON)?,
                        MAX_BODY_BYTES as u64,
                    )
                    .await?;
                if !status.is_success() {
                    return Err(http::classify(status, &headers, &body, now));
                }
                decode::<Vec<Asset>>(&body)
            })
            .await?;

            let short = batch.len() < ASSETS_PER_PAGE as usize;
            all.extend(batch);
            if short {
                return Ok(all);
            }
        }
        Err(AppError::Other(format!(
            "this release reports more than the documented {} assets a GitHub release can \
             hold; refusing to follow further pages. Run `ai-usagebar sync prune` — or, if \
             the release really is that full, the bundle needs a second release, which this \
             build does not write.",
            MAX_ASSET_PAGES * ASSETS_PER_PAGE
        )))
    }

    /// Upload one pack or keyfile asset.
    ///
    /// Goes to the **uploads** host, which is a different origin from the API
    /// host — that is why [`Endpoints`](super::Endpoints) has carried
    /// `uploads_base` since plan 3-01, and it is what makes this path testable
    /// against `mockito` at all.
    ///
    /// A `422` naming an `already_exists` error is **not** a failure: a retried
    /// upload whose first response was lost is the ordinary case, so the asset
    /// is re-listed and the existing one returned.
    pub async fn upload_asset(
        &self,
        repo: &RepoRef,
        release_id: u64,
        name: &str,
        body: Vec<u8>,
        permit: &Pushing,
        now: DateTime<Utc>,
    ) -> Result<Asset> {
        permit.covers(repo)?;
        // Our asset names are a literal prefix plus 64 hex characters plus an
        // extension, so this never fires in practice — but the value is
        // interpolated into a query string, and a check is cheaper to reason
        // about than a percent-encoding dependency.
        if !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        {
            return Err(AppError::Other(format!(
                "refusing to upload an asset under the name {name:?}: asset names are a \
                 fixed prefix plus a content address, and nothing else is written"
            )));
        }

        let uploaded = retried(now, || async {
            let url = format!(
                "{}/repos/{}/{}/releases/{release_id}/assets?name={name}",
                self.endpoints.uploads_base.trim_end_matches('/'),
                repo.owner,
                repo.name
            );
            // ponytail: one copy of the body per attempt, because the retry
            // closure is `Fn` and `reqwest::Body` consumes its `Vec`. At
            // `PACK_MAX` = 48 MiB that is a 48 MiB transient. If a future phase
            // raises `PACK_MAX` past ~256 MiB, this is one of the two places to
            // switch to a streaming body (the other is `PackWriter::finish`).
            let (status, headers, response) = self
                .send_capped(
                    self.authed_gh(Method::POST, &url, ACCEPT_JSON)?
                        .header(
                            reqwest::header::CONTENT_TYPE,
                            HeaderValue::from_static("application/octet-stream"),
                        )
                        .body(body.clone()),
                    MAX_BODY_BYTES as u64,
                )
                .await?;
            if status == StatusCode::UNPROCESSABLE_ENTITY && already_exists(&response) {
                return Ok(None);
            }
            if !status.is_success() {
                return Err(http::classify(status, &headers, &response, now));
            }
            decode::<Asset>(&response).map(Some)
        })
        .await?;

        // `permit` is bound rather than `_`-prefixed because the already-exists
        // arm below re-lists, and a reader of this function should see that the
        // permit governs the POST above and nothing else.
        let _ = permit;
        if let Some(asset) = uploaded {
            return Ok(asset);
        }
        self.list_assets(repo, release_id, now)
            .await?
            .into_iter()
            .find(|a| a.name == name)
            .ok_or_else(|| {
                AppError::Other(format!(
                    "GitHub reported that the asset {name} already exists, but it is not in \
                     the release's asset listing. Re-run the command."
                ))
            })
    }

    /// **The one destructive remote verb in the crate.**
    ///
    /// Its only callers are prune (plan 4-05), which deletes packs no surviving
    /// snapshot references and none younger than
    /// `push::PRUNE_GRACE`, and rekey (plan 4-06), which deletes the superseded
    /// keyfile asset after the pointer already names its replacement. Nothing
    /// else may call it, and nothing calls it before a successful flip.
    ///
    /// A 404 is success: the asset is gone, which is the outcome the caller
    /// asked for.
    pub async fn delete_asset(
        &self,
        repo: &RepoRef,
        asset_id: u64,
        permit: &Pushing,
        now: DateTime<Utc>,
    ) -> Result<()> {
        permit.covers(repo)?;
        retried(now, || async {
            let url = self.api_url(repo, &format!("/releases/assets/{asset_id}"));
            let (status, headers, body) = self
                .send_capped(
                    self.authed_gh(Method::DELETE, &url, ACCEPT_JSON)?,
                    MAX_BODY_BYTES as u64,
                )
                .await?;
            if status.is_success() || status == StatusCode::NOT_FOUND {
                return Ok(());
            }
            Err(http::classify(status, &headers, &body, now))
        })
        .await?;
        Ok(())
    }

    /// Fetch an asset's bytes.
    ///
    /// GitHub answers with a 302 to signed storage on a host this project does
    /// not control. [`Client`]'s reqwest instance carries
    /// [`same_origin_redirect_policy`](crate::vendor::same_origin_redirect_policy),
    /// so that hop is **refused** — and this method does not work around it by
    /// relaxing the policy. It reads `Location` off the 3xx and issues the second
    /// request from a separate client that carries **no `Authorization` header at
    /// all** (T-4-01). The signed URL already carries its own authorization;
    /// replaying a `Contents: write` bearer token to storage we do not control is
    /// exactly the leak the same-origin policy exists to prevent.
    pub async fn download_asset(
        &self,
        repo: &RepoRef,
        asset_id: u64,
        now: DateTime<Utc>,
    ) -> Result<Vec<u8>> {
        let bytes = retried(now, || async {
            let url = self.api_url(repo, &format!("/releases/assets/{asset_id}"));
            let (status, headers, body) = self
                .send_capped(
                    self.authed_gh(
                        Method::GET,
                        &url,
                        HeaderValue::from_static("application/octet-stream"),
                    )?,
                    MAX_ASSET_BYTES,
                )
                .await?;

            if status.is_redirection() {
                let location = headers
                    .get(reqwest::header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .ok_or_else(|| GithubError::Unexpected {
                        status: status.as_u16(),
                        message: "GitHub redirected the asset download without naming a \
                                  destination"
                            .into(),
                    })?
                    .to_owned();
                return self.follow_unauthenticated(&location, now).await;
            }
            if !status.is_success() {
                return Err(http::classify(status, &headers, &body, now));
            }
            Ok(body)
        })
        .await?;
        Ok(bytes)
    }

    /// The token-free second hop of [`download_asset`](Client::download_asset).
    ///
    /// A fresh `reqwest::Client` built here, with no default headers and no
    /// bearer token in scope. Redirects are disabled outright: signed storage
    /// answers directly, and following further hops would be a second place to
    /// reason about where the request ends up.
    async fn follow_unauthenticated(&self, url: &str, now: DateTime<Utc>) -> GhResult<Vec<u8>> {
        let anonymous = reqwest::Client::builder()
            .timeout(HTTP_CLIENT_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| GithubError::Transport {
                message: format!("could not build the storage client ({e})"),
            })?;
        let resp = anonymous
            .get(url)
            .send()
            .await
            .map_err(|e| http::from_transport(&e))?;
        let status = resp.status();
        let headers = resp.headers().clone();
        let body = read_body_capped(resp, MAX_ASSET_BYTES as usize)
            .await
            .map_err(|e| GithubError::Unexpected {
                status: status.as_u16(),
                message: e.to_string(),
            })?;
        if !status.is_success() {
            return Err(http::classify(status, &headers, &body, now));
        }
        Ok(body)
    }

    /// Read the pointer file, with the blob `sha` a later `PUT` needs.
    ///
    /// `Ok(None)` on 404, which is first push and not an error.
    pub async fn get_contents(
        &self,
        repo: &RepoRef,
        path: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<(String, Vec<u8>)>> {
        let found = retried(now, || async {
            let url = self.api_url(repo, &format!("/contents/{path}"));
            let (status, headers, body) = self
                .send_capped(
                    self.authed_gh(Method::GET, &url, ACCEPT_JSON)?,
                    MAX_POINTER_BYTES,
                )
                .await?;
            if status == StatusCode::NOT_FOUND {
                return Ok(None);
            }
            if !status.is_success() {
                return Err(http::classify(status, &headers, &body, now));
            }
            let doc = decode::<ContentsFile>(&body)?;
            // The Contents API wraps its base64 at 60 columns.
            let raw: String = doc.content.chars().filter(|c| !c.is_whitespace()).collect();
            let decoded = B64.decode(raw).map_err(|_| GithubError::Unexpected {
                status: status.as_u16(),
                message: "the pointer file's content was not valid base64".into(),
            })?;
            Ok(Some((doc.sha, decoded)))
        })
        .await?;
        Ok(found)
    }

    /// Write the pointer file with a compare-and-swap precondition.
    ///
    /// `sha` `Some` is "replace exactly this blob"; `None` is "create, and fail
    /// if it exists". The field is **omitted** rather than sent as null in the
    /// second case — those are different requests to the Contents API — which is
    /// why the body is a struct with `skip_serializing_if` and not a hand-built
    /// map.
    ///
    /// **A 422 from this endpoint is a conflict.** GitHub answers a stale `sha`
    /// with 409, but answers a `PUT` that *omits* `sha` against a path that
    /// already exists with 422 — which is exactly first-push-with-lost-local-state.
    /// Left unclassified it would surface as an unrecognised hard error at the
    /// last step of a long push, when the right response is the same
    /// re-read-and-rebuild retry a 409 gets. Mapped here, at the one call site
    /// that knows it omitted a `sha`, and nowhere else: 422 is also GitHub's
    /// generic validation failure, so mapping it inside `classify` would relabel
    /// every malformed request as a conflict.
    ///
    /// Returns the new blob `sha`, which the caller keeps for its next flip.
    // Eight, and every one of them is load-bearing: three name the object, one
    // is the precondition the whole design turns on, one is the capability, one
    // is the clock. A struct wrapper here would be a struct with one call site.
    #[allow(clippy::too_many_arguments)]
    pub async fn put_contents(
        &self,
        repo: &RepoRef,
        path: &str,
        message: &str,
        body: &[u8],
        sha: Option<&str>,
        permit: &Pushing,
        now: DateTime<Utc>,
    ) -> Result<String> {
        permit.covers(repo)?;
        let content = B64.encode(body);
        let new_sha = retried(now, || async {
            let url = self.api_url(repo, &format!("/contents/{path}"));
            let (status, headers, response) = self
                .send_capped(
                    self.authed_gh(Method::PUT, &url, ACCEPT_JSON)?
                        .json(&PutContents {
                            message,
                            content: &content,
                            sha,
                        }),
                    MAX_POINTER_BYTES,
                )
                .await?;
            if !status.is_success() {
                let err = http::classify(status, &headers, &response, now);
                return Err(match err {
                    GithubError::Unexpected {
                        status: 422,
                        message,
                    } => GithubError::Conflict { message },
                    other => other,
                });
            }
            decode::<PutResponse>(&response).map(|r| r.content.sha)
        })
        .await?;
        Ok(new_sha)
    }
}

// ---------------------------------------------------------------------------
// Wire shapes
// ---------------------------------------------------------------------------

/// Lands in the repository's git history in the clear forever, so it names the
/// tool and nothing about the machine that wrote it.
const RELEASE_NOTE: &str = "Encrypted ai-usagebar sync data. Written by `ai-usagebar sync push`; not meant to be \
     downloaded by hand.";

#[derive(Deserialize)]
struct ReleaseRef {
    id: u64,
}

#[derive(Serialize)]
struct NewRelease<'a> {
    tag_name: &'a str,
    name: &'a str,
    body: &'a str,
}

#[derive(Deserialize)]
struct ContentsFile {
    sha: String,
    content: String,
}

#[derive(Serialize)]
struct PutContents<'a> {
    message: &'a str,
    content: &'a str,
    /// Omitted entirely when absent. A `sha` of `null` is not the same request.
    #[serde(skip_serializing_if = "Option::is_none")]
    sha: Option<&'a str>,
}

#[derive(Deserialize)]
struct PutResponse {
    content: PutResponseContent,
}

#[derive(Deserialize)]
struct PutResponseContent {
    sha: String,
}

/// Parse a response body, turning a shape surprise into a classified error
/// rather than a panic. The remote is hostile by assumption.
fn decode<T: serde::de::DeserializeOwned>(body: &[u8]) -> GhResult<T> {
    serde_json::from_slice(body).map_err(|e| GithubError::Unexpected {
        status: 200,
        message: format!("GitHub's answer was not the shape this build expects ({e})"),
    })
}

/// GitHub reports a duplicate asset name as a 422 whose top-level `message` is
/// the generic "Validation Failed"; the discriminating value is in `errors`.
fn already_exists(body: &[u8]) -> bool {
    #[derive(Deserialize)]
    struct Errors {
        #[serde(default)]
        errors: Vec<ErrorEntry>,
    }
    #[derive(Deserialize)]
    struct ErrorEntry {
        #[serde(default)]
        code: String,
    }
    serde_json::from_slice::<Errors>(body)
        .map(|e| e.errors.iter().any(|entry| entry.code == "already_exists"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::github::Endpoints;
    use crate::sync::github::token::TokenSource;
    use zeroize::Zeroizing;

    const TOKEN: &str = "github_pat_fixture_not_a_real_token";

    const NOW: DateTime<Utc> = match DateTime::from_timestamp(1_700_000_000, 0) {
        Some(t) => t,
        None => panic!("a fixed timestamp"),
    };

    fn repo() -> RepoRef {
        RepoRef::parse("o/n").unwrap()
    }

    /// A permit through the only door there is: a private repository, gated and
    /// spent at the test's fixed clock. There is no constructor to shortcut, so
    /// this is also a standing check that the gate still clears a private repo.
    fn permit() -> crate::sync::github::gate::Pushing {
        let facts = crate::sync::github::gate::RepoFacts {
            id: 1,
            private: true,
            visibility: "private".into(),
            owner_login: "o".into(),
            owner_id: 7,
            archived: false,
            fork: false,
            admin_permission: false,
        };
        crate::sync::github::gate::assert_pushable(&facts, &repo(), true, NOW)
            .expect("a private repository clears")
            .0
            .spend(NOW)
            .expect("freshly minted")
    }

    /// Both bases at one server — which is exactly what makes the uploads host
    /// testable, and the reason `Endpoints` has carried two fields since 3-01.
    fn client_at(base: &str) -> Client {
        client_split(base, base)
    }

    fn client_split(api: &str, uploads: &str) -> Client {
        Client::new(
            &Endpoints {
                api_base: api.into(),
                uploads_base: uploads.into(),
            },
            Zeroizing::new(TOKEN.into()),
            TokenSource::Env,
        )
        .unwrap()
    }

    /// Never sleeps. Every `with_retry` test passes this.
    async fn no_sleep(_: Duration) {}

    const ASSET_JSON: &str = r#"{"id":42,"name":"pack-aa.bin","size":9,"state":"uploaded",
        "created_at":"2023-11-14T22:13:20Z","digest":"sha256:beef"}"#;

    // ---- ensure_release ---------------------------------------------------

    #[tokio::test]
    async fn an_existing_tag_yields_its_id_and_issues_no_post() {
        let mut server = mockito::Server::new_async().await;
        let get = server
            .mock("GET", "/repos/o/n/releases/tags/ai-usagebar-sync-v1")
            .with_status(200)
            .with_body(r#"{"id":77}"#)
            .expect(1)
            .create_async()
            .await;
        let post = server
            .mock("POST", "/repos/o/n/releases")
            .with_status(201)
            .with_body(r#"{"id":999}"#)
            .expect(0)
            .create_async()
            .await;

        let id = client_at(&server.url())
            .ensure_release(&repo(), "ai-usagebar-sync-v1", &permit(), NOW)
            .await
            .unwrap();
        assert_eq!(id, 77);
        get.assert_async().await;
        post.assert_async().await;
    }

    #[tokio::test]
    async fn a_missing_tag_creates_one_published_release_and_yields_its_id() {
        let mut server = mockito::Server::new_async().await;
        let _get = server
            .mock("GET", "/repos/o/n/releases/tags/ai-usagebar-sync-v1")
            .with_status(404)
            .with_body(r#"{"message":"Not Found"}"#)
            .create_async()
            .await;
        let post = server
            .mock("POST", "/repos/o/n/releases")
            .match_body(mockito::Matcher::PartialJsonString(
                r#"{"tag_name":"ai-usagebar-sync-v1","name":"ai-usagebar-sync-v1"}"#.into(),
            ))
            .with_status(201)
            .with_body(r#"{"id":5}"#)
            .expect(1)
            .create_async()
            .await;

        let id = client_at(&server.url())
            .ensure_release(&repo(), "ai-usagebar-sync-v1", &permit(), NOW)
            .await
            .unwrap();
        assert_eq!(id, 5);
        post.assert_async().await;
    }

    /// A repository with no commits cannot be tagged, so GitHub answers the
    /// release `POST` with a bare 422 "Validation Failed".
    ///
    /// This is the **ordinary first-run state**, not an exotic one:
    /// `gh repo create --private` leaves a repository with zero commits, and a
    /// user who follows the README's own recipe lands here on their first push.
    /// The generic 422 text told them to check GitHub's status page and retry,
    /// which is advice for an outage that is not happening.
    ///
    /// Asserted on the *content* of the message rather than the variant: what
    /// failed here is that the user could not tell what to do next.
    #[tokio::test]
    async fn a_repository_with_no_commits_says_so_instead_of_blaming_github() {
        let mut server = mockito::Server::new_async().await;
        let _get = server
            .mock("GET", "/repos/o/n/releases/tags/ai-usagebar-sync-v1")
            .with_status(404)
            .with_body(r#"{"message":"Not Found"}"#)
            .create_async()
            .await;
        // Exactly what GitHub returns for a tag against an empty repository:
        // no `errors` array, no field, no hint.
        let post = server
            .mock("POST", "/repos/o/n/releases")
            .with_status(422)
            .with_body(r#"{"message":"Validation Failed"}"#)
            .expect(1)
            .create_async()
            .await;

        let err = client_at(&server.url())
            .ensure_release(&repo(), "ai-usagebar-sync-v1", &permit(), NOW)
            .await
            .expect_err("an empty repository cannot hold a release");
        let text = err.to_string();

        assert!(text.contains("o/n has no commits yet"), "{text}");
        assert!(
            text.contains("gh api repos/o/n/contents/README.md"),
            "the message carries the command that fixes it: {text}"
        );
        assert!(
            text.contains("Nothing was uploaded"),
            "a refusal says what did not happen: {text}"
        );
        assert!(
            !text.contains("githubstatus"),
            "an empty repository is not an outage: {text}"
        );
        // One attempt: retrying cannot make commits appear.
        post.assert_async().await;
    }

    // ---- list_assets ------------------------------------------------------

    #[tokio::test]
    async fn a_listing_parses_every_field_and_tolerates_only_the_digest_missing() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", mockito::Matcher::Regex(r"/releases/9/assets".into()))
            .with_status(200)
            .with_body(format!(
                "[{ASSET_JSON},{{\"id\":43,\"name\":\"pack-bb.bin\",\"size\":1,\
                 \"state\":\"starter\",\"created_at\":\"2023-11-14T22:13:20Z\"}}]"
            ))
            .create_async()
            .await;

        let assets = client_at(&server.url())
            .list_assets(&repo(), 9, NOW)
            .await
            .unwrap();
        assert_eq!(assets.len(), 2);
        assert_eq!(assets[0].id, 42);
        assert_eq!(assets[0].name, "pack-aa.bin");
        assert_eq!(assets[0].size, 9);
        assert_eq!(assets[0].state, ASSET_STATE_UPLOADED);
        assert_eq!(assets[0].created_at, NOW);
        assert_eq!(assets[0].digest.as_deref(), Some("sha256:beef"));
        assert_eq!(assets[1].digest, None, "only the digest may be absent");
    }

    /// T-4-06: a remote that answers every page with a full page is a denial of
    /// service against an unbounded loop.
    #[tokio::test]
    async fn a_remote_that_never_runs_out_of_pages_is_refused_by_the_page_cap() {
        let mut server = mockito::Server::new_async().await;
        let full_page = format!("[{}]", vec![ASSET_JSON; 100].join(","));
        let _m = server
            .mock("GET", mockito::Matcher::Regex(r"/releases/9/assets".into()))
            .with_status(200)
            .with_body(full_page)
            .expect(MAX_ASSET_PAGES as usize)
            .create_async()
            .await;

        let err = client_at(&server.url())
            .list_assets(&repo(), 9, NOW)
            .await
            .expect_err("the page cap must stop this");
        assert!(err.to_string().contains("1000"), "{err}");
    }

    // ---- upload_asset -----------------------------------------------------

    #[tokio::test]
    async fn an_upload_goes_to_the_uploads_base_as_an_octet_stream() {
        let mut api = mockito::Server::new_async().await;
        let mut uploads = mockito::Server::new_async().await;
        let api_hit = api
            .mock("POST", mockito::Matcher::Any)
            .expect(0)
            .create_async()
            .await;
        let m = uploads
            .mock("POST", "/repos/o/n/releases/9/assets?name=pack-aa.bin")
            .match_header("content-type", "application/octet-stream")
            .match_body("packbytes")
            .with_status(201)
            .with_body(ASSET_JSON)
            .expect(1)
            .create_async()
            .await;

        let asset = client_split(&api.url(), &uploads.url())
            .upload_asset(
                &repo(),
                9,
                "pack-aa.bin",
                b"packbytes".to_vec(),
                &permit(),
                NOW,
            )
            .await
            .unwrap();
        assert_eq!(asset.id, 42);
        m.assert_async().await;
        api_hit.assert_async().await;
    }

    /// A retried upload after a lost response is the ordinary case.
    #[tokio::test]
    async fn a_422_already_exists_relists_and_returns_the_existing_asset() {
        let mut server = mockito::Server::new_async().await;
        let _post = server
            .mock("POST", mockito::Matcher::Regex("/assets".into()))
            .with_status(422)
            .with_body(
                r#"{"message":"Validation Failed",
                    "errors":[{"resource":"ReleaseAsset","code":"already_exists"}]}"#,
            )
            .expect(1)
            .create_async()
            .await;
        let list = server
            .mock("GET", mockito::Matcher::Regex("per_page=100".into()))
            .with_status(200)
            .with_body(format!("[{ASSET_JSON}]"))
            .expect(1)
            .create_async()
            .await;

        let asset = client_at(&server.url())
            .upload_asset(
                &repo(),
                9,
                "pack-aa.bin",
                b"packbytes".to_vec(),
                &permit(),
                NOW,
            )
            .await
            .unwrap();
        assert_eq!(asset.id, 42);
        list.assert_async().await;
    }

    #[tokio::test]
    async fn an_asset_name_outside_the_generated_shape_is_refused_before_any_request() {
        let err = client_at("http://127.0.0.1:1")
            .upload_asset(
                &repo(),
                9,
                "pack-../../etc.bin",
                b"x".to_vec(),
                &permit(),
                NOW,
            )
            .await
            .expect_err("that is not a generated asset name");
        assert!(err.to_string().contains("content address"), "{err}");
    }

    // ---- delete_asset -----------------------------------------------------

    #[tokio::test]
    async fn a_delete_succeeds_and_a_404_delete_is_also_success() {
        let mut server = mockito::Server::new_async().await;
        let _gone = server
            .mock("DELETE", "/repos/o/n/releases/assets/1")
            .with_status(204)
            .create_async()
            .await;
        let _absent = server
            .mock("DELETE", "/repos/o/n/releases/assets/2")
            .with_status(404)
            .with_body(r#"{"message":"Not Found"}"#)
            .create_async()
            .await;

        let client = client_at(&server.url());
        client
            .delete_asset(&repo(), 1, &permit(), NOW)
            .await
            .unwrap();
        client
            .delete_asset(&repo(), 2, &permit(), NOW)
            .await
            .expect("gone is the outcome the caller asked for");
    }

    // ---- download_asset ---------------------------------------------------

    /// T-4-01, the critical one: the bearer token must not follow the 302 to
    /// storage the project does not control.
    #[tokio::test]
    async fn the_redirect_to_storage_is_followed_by_a_client_carrying_no_authorization() {
        let mut api = mockito::Server::new_async().await;
        let mut storage = mockito::Server::new_async().await;
        let signed = storage
            .mock("GET", "/signed/blob")
            .match_header("authorization", mockito::Matcher::Missing)
            .with_status(200)
            .with_body("the pack bytes")
            .expect(1)
            .create_async()
            .await;
        let _redirect = api
            .mock("GET", "/repos/o/n/releases/assets/42")
            .match_header("accept", "application/octet-stream")
            .with_status(302)
            .with_header("location", &format!("{}/signed/blob", storage.url()))
            .create_async()
            .await;

        let bytes = client_at(&api.url())
            .download_asset(&repo(), 42, NOW)
            .await
            .unwrap();
        assert_eq!(bytes, b"the pack bytes");
        // Matched with `authorization` *missing*: had the token been replayed,
        // the mock would not have matched and the request would have 501'd.
        signed.assert_async().await;
    }

    /// T-4-05: the asset cap and the pointer cap are different numbers, and
    /// `download_asset` carries the larger one.
    ///
    /// The refusal itself is proved once, cheaply, by the pointer test below —
    /// both verbs go through the same `read_body_capped`. What is left to pin is
    /// that this verb is not accidentally bounded at the pointer's 1 MiB, which a
    /// body over that limit landing intact demonstrates. Exceeding 64 MiB for
    /// real would move that many bytes through the AUR `check()` on every
    /// installer's machine to learn nothing new.
    #[tokio::test]
    async fn an_asset_is_bounded_by_the_asset_cap_and_not_by_the_pointer_cap() {
        let mut server = mockito::Server::new_async().await;
        let big = "x".repeat(MAX_POINTER_BYTES as usize + 1);
        let _m = server
            .mock("GET", "/repos/o/n/releases/assets/42")
            .with_status(200)
            .with_body(&big)
            .create_async()
            .await;

        let bytes = client_at(&server.url())
            .download_asset(&repo(), 42, NOW)
            .await
            .expect("an asset over the pointer cap is still well under the asset cap");
        assert_eq!(bytes.len(), big.len());
        assert!(
            MAX_ASSET_BYTES > crate::sync::pack::PACK_MAX as u64,
            "the cap must sit above the largest pack this format writes"
        );
    }

    // ---- get_contents / put_contents --------------------------------------

    #[tokio::test]
    async fn a_missing_pointer_is_first_push_and_a_present_one_yields_its_sha_and_bytes() {
        let mut server = mockito::Server::new_async().await;
        let _absent = server
            .mock("GET", "/repos/o/n/contents/sync/absent.json")
            .with_status(404)
            .with_body(r#"{"message":"Not Found"}"#)
            .create_async()
            .await;
        let _present = server
            .mock("GET", "/repos/o/n/contents/sync/pointer.json")
            .with_status(200)
            // Wrapped at 60 columns, exactly as the Contents API answers.
            .with_body(format!(
                r#"{{"sha":"abc123","content":"{}\n{}"}}"#,
                &B64.encode(b"{\"format\":1}")[..8],
                &B64.encode(b"{\"format\":1}")[8..]
            ))
            .create_async()
            .await;

        let client = client_at(&server.url());
        assert!(
            client
                .get_contents(&repo(), "sync/absent.json", NOW)
                .await
                .unwrap()
                .is_none()
        );
        let (sha, bytes) = client
            .get_contents(&repo(), "sync/pointer.json", NOW)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(sha, "abc123");
        assert_eq!(bytes, br#"{"format":1}"#);
    }

    /// T-4-05, second half: a body that declares no length at all is bounded
    /// **while it is read**, so a chunked stream cannot exhaust memory either.
    #[tokio::test]
    async fn a_pointer_body_past_the_cap_is_refused() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/repos/o/n/contents/sync/pointer.json")
            .with_status(200)
            .with_chunked_body(|w| {
                let block = vec![b'x'; 64 * 1024];
                for _ in 0..=(MAX_POINTER_BYTES / block.len() as u64) {
                    w.write_all(&block)?;
                }
                Ok(())
            })
            .create_async()
            .await;

        let err = client_at(&server.url())
            .get_contents(&repo(), "sync/pointer.json", NOW)
            .await
            .expect_err("the cap must refuse this");
        assert!(err.to_string().contains("exceeds"), "{err}");
    }

    /// The compare-and-swap, both halves: `Some` sends the `sha`, `None` omits
    /// the field **entirely** rather than sending null. Asserted against the raw
    /// request body rather than through a JSON matcher, because "the key is
    /// absent" and "the key is null" are equal as JSON and different as requests.
    #[tokio::test]
    async fn a_first_push_omits_the_sha_and_a_later_one_sends_it() {
        use std::sync::{Arc, Mutex};

        let mut server = mockito::Server::new_async().await;
        let seen: Arc<Mutex<Vec<String>>> = Arc::default();
        let recorder = Arc::clone(&seen);
        let m = server
            .mock("PUT", "/repos/o/n/contents/sync/pointer.json")
            .with_status(201)
            .with_body(r#"{"content":{"sha":"new1"}}"#)
            .match_request(move |req| {
                recorder
                    .lock()
                    .unwrap()
                    .push(req.utf8_lossy_body().unwrap().into_owned());
                true
            })
            .expect(2)
            .create_async()
            .await;

        let client = client_at(&server.url());
        let sha = client
            .put_contents(
                &repo(),
                "sync/pointer.json",
                "first",
                b"{}",
                None,
                &permit(),
                NOW,
            )
            .await
            .unwrap();
        assert_eq!(sha, "new1");
        client
            .put_contents(
                &repo(),
                "sync/pointer.json",
                "second",
                b"{}",
                Some("old1"),
                &permit(),
                NOW,
            )
            .await
            .unwrap();
        m.assert_async().await;

        let bodies = seen.lock().unwrap();
        assert!(
            !bodies[0].contains("sha"),
            "a first push sends no sha at all: {}",
            bodies[0]
        );
        assert!(bodies[0].contains(r#""message":"first""#), "{}", bodies[0]);
        assert!(bodies[1].contains(r#""sha":"old1""#), "{}", bodies[1]);
    }

    /// A `PUT` that omitted `sha` against an existing path answers 422, and the
    /// caller needs the same re-read-and-rebuild retry a 409 gets.
    #[tokio::test]
    async fn a_422_from_put_contents_is_a_conflict_and_not_an_unexpected_status() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("PUT", "/repos/o/n/contents/sync/pointer.json")
            .with_status(422)
            .with_body(r#"{"message":"sha wasn't supplied"}"#)
            .create_async()
            .await;

        let err = client_at(&server.url())
            .put_contents(
                &repo(),
                "sync/pointer.json",
                "m",
                b"{}",
                None,
                &permit(),
                NOW,
            )
            .await
            .expect_err("422 here is a conflict");
        assert!(
            err.to_string().contains("conflicting remote state"),
            "{err}"
        );
        assert!(!err.to_string().contains("unexpected HTTP"), "{err}");
    }

    // ---- with_retry -------------------------------------------------------

    #[tokio::test]
    async fn a_rate_limit_is_retried_to_the_cap_and_the_last_error_is_returned() {
        let mut server = mockito::Server::new_async().await;
        let m = server
            .mock("GET", "/repos/o/n")
            .with_status(429)
            .with_header("retry-after", "90")
            .with_body(r#"{"message":"slow down"}"#)
            .expect(3)
            .create_async()
            .await;

        let client = client_at(&server.url());
        let slept = std::cell::RefCell::new(Vec::new());
        let err = with_retry(
            3,
            |d| {
                slept.borrow_mut().push(d);
                no_sleep(d)
            },
            NOW,
            || async {
                let (status, headers, body) =
                    client
                        .get_json("/repos/o/n")
                        .await
                        .map_err(|_| GithubError::Transport {
                            message: "unreachable".into(),
                        })?;
                Err::<(), _>(http::classify(status, &headers, &body, NOW))
            },
        )
        .await
        .expect_err("three rate limits in a row");

        m.assert_async().await;
        assert_eq!(
            slept.into_inner(),
            vec![Duration::from_secs(90); 3],
            "the delay is the one `retry_delay` computed from `retry-after`"
        );
        assert!(err.to_string().contains("rate-limited"), "{err}");
    }

    /// A retried 401 is a slower failure; a retried 409 would overwrite the
    /// state the precondition exists to protect.
    #[tokio::test]
    async fn the_four_terminal_failures_are_never_retried() {
        for (status, body) in [
            (401, r#"{"message":"Bad credentials"}"#),
            (403, r#"{"message":"Resource not accessible"}"#),
            (404, r#"{"message":"Not Found"}"#),
            (409, r#"{"message":"is at abc but expected def"}"#),
        ] {
            let mut server = mockito::Server::new_async().await;
            let m = server
                .mock("GET", "/repos/o/n")
                .with_status(status)
                .with_body(body)
                .expect(1)
                .create_async()
                .await;

            let client = client_at(&server.url());
            let slept = std::cell::Cell::new(0usize);
            let err = with_retry(
                4,
                |d| {
                    slept.set(slept.get() + 1);
                    no_sleep(d)
                },
                NOW,
                || async {
                    let (s, h, b) = client.get_json("/repos/o/n").await.map_err(|_| {
                        GithubError::Transport {
                            message: "unreachable".into(),
                        }
                    })?;
                    Err::<(), _>(http::classify(s, &h, &b, NOW))
                },
            )
            .await
            .expect_err("a terminal failure");

            m.assert_async().await;
            assert_eq!(slept.get(), 0, "{status} must not sleep: {err}");
        }
    }

    #[tokio::test]
    async fn a_retry_that_succeeds_returns_the_value_rather_than_the_error() {
        let attempts = std::cell::Cell::new(0u32);
        let value = with_retry(4, no_sleep, NOW, || async {
            attempts.set(attempts.get() + 1);
            if attempts.get() < 3 {
                return Err(GithubError::Transport {
                    message: "reset".into(),
                });
            }
            Ok(7)
        })
        .await
        .unwrap();
        assert_eq!(value, 7);
        assert_eq!(attempts.get(), 3);
    }

    /// T-4-10: nothing this module produces may carry the token.
    ///
    /// A 403 rather than a dead port on purpose — a transport failure is
    /// retryable, and the production verbs sleep for real between attempts.
    #[tokio::test]
    async fn no_error_this_module_returns_contains_the_token() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/repos/o/n/contents/sync/pointer.json")
            .with_status(403)
            .with_body(r#"{"message":"Resource not accessible by personal access token"}"#)
            .create_async()
            .await;

        let err = client_at(&server.url())
            .get_contents(&repo(), "sync/pointer.json", NOW)
            .await
            .expect_err("a forbidden read");
        let rendered = format!("{err} {err:?}");
        assert!(!rendered.contains(TOKEN), "{rendered}");
        assert!(!rendered.contains(&TOKEN[..8]), "{rendered}");
    }
}
