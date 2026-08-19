//! Live API smoke test suite — DETECTS UNDOCUMENTED-ENDPOINT DRIFT.
//!
//! Hits the real vendor endpoints using credentials from your shell (API keys,
//! or the local CLI/IDE session files for Cursor and Kiro CLI).
//! Asserts only the *fields we depend on* so when a vendor renames or removes
//! one, the failure points at the exact field rather than dumping the whole
//! response.
//!
//! These tests are `#[ignore]` so plain `cargo test` doesn't hit external
//! APIs (and won't fail on machines without creds). Run explicitly:
//!
//! ```bash
//! source ~/.config/zsh/secrets
//! cargo test --test live -- --ignored --nocapture
//! # or:
//! make smoke                         # runs every configured live smoke test
//! cargo test --test live kimi_live -- --ignored --nocapture
//! ```
//!
//! ## When a smoke test fails
//!
//! 1. Re-run with `--nocapture` to see the actual response shape.
//! 2. Paste the response + error into Claude Code and ask it to update the
//!    affected vendor's `types.rs` to match. The error messages here are
//!    deliberately verbose so the update is mechanical.
//! 3. After updating, re-run `cargo test --test live -- --ignored` to confirm.
//!
//! ## What gets tested (only the contract we rely on)
//!
//! - **Anthropic**: `five_hour.utilization` is 0..=100, `resets_at` parses
//!   as RFC3339, `extra_usage.{is_enabled,monthly_limit,used_credits}` round-trip.
//! - **OpenAI**: `rate_limit.primary_window.used_percent` is 0..=100, the
//!   id_token's exp claim is parseable.
//! - **Z.AI**: response is a `{code, data: {limits:[...], level}, success}`
//!   envelope and at least one `TOKENS_LIMIT` entry exists.
//! - **OpenRouter**: `/credits` returns `{data:{total_credits,total_usage}}`
//!   and `/key` returns `{data:{usage,is_free_tier}}`.
//! - **Kimi**: the public snapshot exposes parsed weekly limit/used/remaining
//!   counters and a bounded percentage. Its reset and selected 5-hour rolling
//!   window are optional, so the smoke test validates their public fields only
//!   when present; the snapshot does not expose raw wire duration/unit.
//!   `kimi_live` skips when optional `KIMI_API_KEY` is unset.
//! - **Cursor**: reads the session token from the local `state.vscdb`, then
//!   asserts `premium_pct` is 0..=100 and a future `premium_reset_at` was
//!   derived from `startOfMonth`. `cursor_live` skips when there is no Cursor
//!   credential source (no state DB, no cursor-agent `auth.json`, and neither
//!   `CURSOR_DB_PATH` nor `CURSOR_AGENT_AUTH_PATH` set).
//! - **Kiro CLI**: reads the AWS SSO OIDC session from kiro-cli's local
//!   `data.sqlite3`, then asserts the credit counters are non-negative and the
//!   plan label is non-empty. `kiro_live` skips when there is no kiro-cli
//!   install (no db and no `KIRO_DB_PATH`).
//! - **SuperGrok**: asks the official Grok Build CLI's `x.ai/billing` ACP
//!   extension, then asserts usage percent and plan. Set
//!   `SUPERGROK_GROK_BINARY` to the trusted official executable.
//!
//! ## Calibration probes (encrypted sync, plan 1-08)
//!
//! Two probes at the bottom of this file are not vendor smoke tests. They
//! answer sizing questions the encrypted-sync format would otherwise have to
//! guess at, and they live here because this is where the project keeps every
//! test allowed to cost real seconds, real gibibytes, or a real network call —
//! all of it behind `#[ignore]`, so `cargo test` and the AUR `check()` run
//! neither.
//!
//! - **CAL-3**, `cal3_argon2id_timing_at_production_parameters`: what Argon2id
//!   actually costs at the shipped m = 1 GiB / t = 3 / p = 1, plus the two
//!   steps down a user on constrained hardware would take. Needs nothing but a
//!   release build:
//!   `cargo test --release --test live -- --ignored --nocapture cal3_`
//! - **CAL-1**, `cal1_range_on_private_release_asset`: whether a private-repo
//!   release asset honours `Range:` after the redirect to signed storage.
//!   Credential-gated and skips cleanly when unset — see its own doc comment
//!   for the three variables and `docs/sync-format.md` for what the answer
//!   changes.

use std::time::Duration;

use ai_usagebar::anthropic;
use ai_usagebar::cache::Cache;
use ai_usagebar::cursor;
use ai_usagebar::error::AppError;
use ai_usagebar::kimi;
use ai_usagebar::kiro;
use ai_usagebar::minimax;
use ai_usagebar::openai;
use ai_usagebar::openrouter;
use ai_usagebar::supergrok;
use ai_usagebar::zai;

fn xdg_cache_for(test: &str) -> Cache {
    // Use a per-test scratch dir so smoke tests don't clobber the real cache.
    let base = std::env::temp_dir().join(format!("ai-usagebar-smoke-{test}"));
    let _ = std::fs::remove_dir_all(&base);
    Cache::at(base)
}

fn assert_pct(label: &str, p: i32) {
    assert!(
        (0..=100).contains(&p),
        "{label}: utilization {p} outside [0,100] — vendor shape changed?"
    );
}

fn is_missing_credentials(err: &AppError) -> bool {
    matches!(err, AppError::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound)
}

#[tokio::test]
#[ignore = "live API; run with --ignored"]
async fn anthropic_live() {
    let creds_path = anthropic::creds::default_path().expect("resolve home directory");
    // Creds live in this file (Linux) or the login Keychain (recent macOS, no
    // file) — CredsTarget::Default covers both. Skip cleanly when neither
    // source resolves — a no-op on machines without creds, as the module doc
    // promises, not a hard failure.
    let creds_target = anthropic::creds::CredsTarget::Default(creds_path);
    match anthropic::creds::resolve(&creds_target) {
        Ok(_) => {}
        Err(err) if is_missing_credentials(&err) => {
            eprintln!("anthropic_live: no Claude credentials (file or Keychain) — skipping");
            return;
        }
        Err(err) => panic!("anthropic_live: failed to read Claude credentials: {err}"),
    }
    let cache = xdg_cache_for("anthropic");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap();
    let endpoints = anthropic::fetch::Endpoints::default();
    let out = anthropic::fetch_snapshot(
        &client,
        &creds_target,
        &cache,
        &endpoints,
        Duration::from_secs(0),
    )
    .await
    .expect("anthropic fetch should succeed against the real API");

    assert!(!out.snapshot.plan.is_empty(), "anthropic plan label empty");
    assert_pct("anthropic.session", out.snapshot.session.utilization_pct);
    assert_pct("anthropic.weekly", out.snapshot.weekly.utilization_pct);
    if let Some(s) = out.snapshot.sonnet.as_ref() {
        assert_pct("anthropic.sonnet", s.utilization_pct);
    }
    if let Some(e) = out.snapshot.extra.as_ref() {
        if let Some(l) = e.limit {
            assert!(l.0 >= 0, "anthropic extra.limit < 0");
        }
        // spent can equal or exceed limit briefly during reconciliation; just sanity-check.
        assert!(e.spent.0 >= 0, "anthropic extra.spent < 0");
    }
    println!(
        "✅ anthropic — plan={}, session={}%, weekly={}%, sonnet={:?}, extra={:?}",
        out.snapshot.plan,
        out.snapshot.session.utilization_pct,
        out.snapshot.weekly.utilization_pct,
        out.snapshot.sonnet.as_ref().map(|s| s.utilization_pct),
        out.snapshot
            .extra
            .as_ref()
            .map(|e| (e.fmt_spent(), e.fmt_limit())),
    );
}

#[tokio::test]
#[ignore = "live API; run with --ignored"]
async fn openai_live() {
    let creds_path = openai::creds::default_path().expect("resolve home directory");
    assert!(
        creds_path.exists(),
        "no Codex credentials at {} — log in with `codex login` first",
        creds_path.display()
    );
    let cache = xdg_cache_for("openai");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap();
    let endpoints = openai::fetch::Endpoints::default();
    let out = openai::fetch_snapshot(
        &client,
        &creds_path,
        &cache,
        &endpoints,
        Duration::from_secs(0),
    )
    .await
    .expect("openai fetch should succeed against the real API");

    assert!(!out.snapshot.plan.is_empty(), "openai plan label empty");
    assert!(
        out.snapshot.session.is_some() || out.snapshot.weekly.is_some(),
        "openai returned no 5h or 7d usage window"
    );
    if let Some(session) = out.snapshot.session.as_ref() {
        assert_pct("openai.session", session.utilization_pct);
    }
    if let Some(weekly) = out.snapshot.weekly.as_ref() {
        assert_pct("openai.weekly", weekly.utilization_pct);
    }
    println!(
        "✅ openai — plan={}, session={:?}%, weekly={:?}%, credits={:?}",
        out.snapshot.plan,
        out.snapshot
            .session
            .as_ref()
            .map(|window| window.utilization_pct),
        out.snapshot
            .weekly
            .as_ref()
            .map(|window| window.utilization_pct),
        out.snapshot.credits.map(|c| c.balance),
    );
}

#[tokio::test]
#[ignore = "live API; run with --ignored"]
async fn zai_live() {
    let api_key = std::env::var("ZAI_API_KEY")
        .expect("ZAI_API_KEY must be set (source ~/.config/zsh/secrets)");
    let cache = xdg_cache_for("zai");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap();
    let endpoints = zai::fetch::Endpoints::default();
    let out = zai::fetch_snapshot(
        &client,
        &api_key,
        &cache,
        &endpoints,
        Duration::from_secs(0),
        None,
    )
    .await
    .expect("zai fetch should succeed against the real API");

    assert!(!out.snapshot.plan.is_empty(), "zai plan label empty");
    // Z.AI may legitimately return 0% on a fresh account, but at least one
    // bucket should exist — if all three are None, the schema changed.
    let has_any = out.snapshot.session.is_some()
        || out.snapshot.weekly.is_some()
        || out.snapshot.mcp.is_some();
    assert!(has_any, "zai snapshot has no buckets — shape changed?");
    for (label, w) in [
        ("session", &out.snapshot.session),
        ("weekly", &out.snapshot.weekly),
        ("mcp", &out.snapshot.mcp),
    ] {
        if let Some(w) = w.as_ref() {
            assert_pct(&format!("zai.{label}"), w.utilization_pct);
        }
    }
    println!(
        "✅ zai — plan={}, session={:?}%, weekly={:?}%, mcp={:?}%",
        out.snapshot.plan,
        out.snapshot.session.as_ref().map(|w| w.utilization_pct),
        out.snapshot.weekly.as_ref().map(|w| w.utilization_pct),
        out.snapshot.mcp.as_ref().map(|w| w.utilization_pct),
    );
}

#[tokio::test]
#[ignore = "live API; run with --ignored"]
async fn openrouter_live() {
    let api_key = std::env::var("OPENROUTER_API_KEY")
        .expect("OPENROUTER_API_KEY must be set (source ~/.config/zsh/secrets)");
    let cache = xdg_cache_for("openrouter");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap();
    let endpoints = openrouter::fetch::Endpoints::default();
    let out = openrouter::fetch_snapshot(
        &client,
        &api_key,
        &cache,
        &endpoints,
        Duration::from_secs(0),
    )
    .await
    .expect("openrouter fetch should succeed against the real API");

    assert!(out.snapshot.total_credits >= 0.0, "or.total_credits < 0");
    assert!(out.snapshot.total_usage >= 0.0, "or.total_usage < 0");
    // total_usage being slightly larger than total_credits is possible during
    // reconciliation (debt allowed); don't assert otherwise.
    println!(
        "✅ openrouter — label={}, balance=${:.2}, used=${:.2}, monthly=${:.2}, free={}",
        out.snapshot.label,
        out.snapshot.balance(),
        out.snapshot.total_usage,
        out.snapshot.usage_monthly,
        out.snapshot.is_free_tier,
    );
}

#[tokio::test]
#[ignore = "live API; run with --ignored"]
async fn kimi_live() {
    let Ok(api_key) = std::env::var("KIMI_API_KEY") else {
        eprintln!("kimi_live: KIMI_API_KEY is unset — skipping optional Kimi smoke test");
        return;
    };
    if api_key.trim().is_empty() {
        eprintln!("kimi_live: KIMI_API_KEY is empty — skipping optional Kimi smoke test");
        return;
    }
    let cache = xdg_cache_for("kimi");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap();
    let endpoints = kimi::fetch::Endpoints::default();
    let out = kimi::fetch_snapshot(
        &client,
        &api_key,
        &cache,
        &endpoints,
        Duration::from_secs(0),
    )
    .await
    .expect("kimi fetch should succeed against the real API");

    // Kimi permits missing or inconsistent counters, and the production
    // snapshot deliberately preserves them. Exercise all weekly fields while
    // checking the production-facing normalized percentage only.
    assert_pct("kimi.weekly", out.snapshot.weekly_pct());
    // A nonzero public limit means the parser selected its optional rolling
    // window. The public snapshot does not retain the wire duration/unit, so
    // it can only validate that window's normalized percentage and counters.
    if out.snapshot.window_limit > 0 {
        assert_pct("kimi.window", out.snapshot.window_pct());
    }
    println!(
        "✅ kimi — plan={:?}, weekly={} / {} ({} remaining; reset {:?}), window={} / {} ({} remaining; reset {:?})",
        out.snapshot.plan,
        out.snapshot.weekly_used,
        out.snapshot.weekly_limit,
        out.snapshot.weekly_remaining,
        out.snapshot.weekly_reset_at,
        out.snapshot.window_used,
        out.snapshot.window_limit,
        out.snapshot.window_remaining,
        out.snapshot.window_reset_at,
    );
}

#[tokio::test]
#[ignore = "live API; run with --ignored"]
async fn cursor_live() {
    // Cursor has no API key — the credential is a session token, either the
    // one the Cursor IDE wrote to its local state DB, or (headless machines
    // with no IDE) the one the `cursor-agent` CLI wrote to its own auth.json.
    // So this test needs one of the two installed (or `CURSOR_DB_PATH` /
    // `CURSOR_AGENT_AUTH_PATH` pointing at a copy) and skips otherwise, the
    // same way `kimi_live` skips without a key. Nothing to fetch on a CI box
    // with neither.
    let db_path = match std::env::var("CURSOR_DB_PATH") {
        Ok(p) if !p.trim().is_empty() => std::path::PathBuf::from(p),
        _ => cursor::db::default_db_path().expect("resolve platform config dir"),
    };
    let agent_auth_path = match std::env::var("CURSOR_AGENT_AUTH_PATH") {
        Ok(p) if !p.trim().is_empty() => std::path::PathBuf::from(p),
        _ => cursor::db::default_agent_auth_path().expect("resolve platform config dir"),
    };
    if !db_path.exists() && !agent_auth_path.exists() {
        eprintln!(
            "cursor_live: no Cursor state DB at {} and no cursor-agent auth at {} — skipping \
             (sign in to the Cursor IDE or run `cursor-agent`, or set CURSOR_DB_PATH / \
             CURSOR_AGENT_AUTH_PATH)",
            db_path.display(),
            agent_auth_path.display()
        );
        return;
    }

    let cache = xdg_cache_for("cursor");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap();
    let endpoints = cursor::fetch::Endpoints::default();
    let out = cursor::fetch_snapshot(
        &client,
        &db_path,
        &agent_auth_path,
        &cache,
        &endpoints,
        Duration::from_secs(0),
    )
    .await
    .expect("cursor fetch should succeed against the real API");

    // The fields the widget depends on: two pool percentages (>= 0; a pool can
    // exceed 100 when over its included allowance, so only the low bound is
    // asserted) and a future billing-cycle reset.
    assert!(
        out.snapshot.auto_pct >= 0 && out.snapshot.api_pct >= 0,
        "cursor: negative pool percentage — shape changed? auto={} api={}",
        out.snapshot.auto_pct,
        out.snapshot.api_pct
    );
    assert!(!out.snapshot.plan.is_empty(), "cursor plan label empty");
    assert!(
        out.snapshot
            .reset_at
            .is_some_and(|r| r > chrono::Utc::now()),
        "cursor: reset_at should be a future instant, got {:?}",
        out.snapshot.reset_at
    );
    println!(
        "✅ cursor — plan={}, Cursor Models {}%, Other Models {}%, total {}%, on-demand={}, reset {:?}",
        out.snapshot.plan,
        out.snapshot.auto_pct,
        out.snapshot.api_pct,
        out.snapshot.total_pct,
        out.snapshot.on_demand_enabled,
        out.snapshot.reset_at,
    );
}

#[tokio::test]
#[ignore = "live API; run with --ignored"]
async fn kiro_live() {
    // Kiro has no API key — the credential is the AWS SSO OIDC session
    // kiro-cli wrote to its own local database after `kiro-cli login`. So this
    // test needs kiro-cli installed and signed in (or `KIRO_DB_PATH` pointing
    // at a copied `data.sqlite3`) and skips otherwise, like `cursor_live`.
    let db_path = match std::env::var("KIRO_DB_PATH") {
        Ok(p) if !p.trim().is_empty() => std::path::PathBuf::from(p),
        _ => kiro::db::default_db_path().expect("resolve platform data dir"),
    };
    if !db_path.exists() {
        eprintln!(
            "kiro_live: no kiro-cli database at {} — skipping (run `kiro-cli login`, or set KIRO_DB_PATH)",
            db_path.display()
        );
        return;
    }

    let cache = xdg_cache_for("kiro");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap();
    let out = kiro::fetch_snapshot(&client, &db_path, &cache, Duration::from_secs(0))
        .await
        .expect("kiro fetch should succeed against the real API");

    // The fields the widget depends on: non-negative credit counters, a
    // non-empty plan label, and (when reported) a future reset.
    assert!(
        out.snapshot.used >= 0.0 && out.snapshot.limit >= 0.0,
        "kiro: negative credit counter — shape changed? used={} limit={}",
        out.snapshot.used,
        out.snapshot.limit
    );
    assert!(!out.snapshot.plan.is_empty(), "kiro plan label empty");
    if let Some(reset) = out.snapshot.reset_at {
        assert!(
            reset > chrono::Utc::now(),
            "kiro: reset_at should be a future instant, got {reset:?}"
        );
    }
    println!(
        "✅ kiro — plan={}, credits {} / {} ({}%), reset {:?}",
        out.snapshot.plan,
        out.snapshot.used,
        out.snapshot.limit,
        out.snapshot.pct(),
        out.snapshot.reset_at,
    );
}

#[tokio::test]
#[ignore = "live API; run with --ignored"]
async fn supergrok_live() {
    // Grok Build owns every auth mode and returns only billing data over ACP.
    let binary = std::env::var_os("SUPERGROK_GROK_BINARY")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| ai_usagebar::config::Config::default().supergrok.grok_binary);
    let auth_override = std::env::var_os("SUPERGROK_AUTH_PATH").map(std::path::PathBuf::from);
    let config_override = std::env::var_os("SUPERGROK_CONFIG_PATH").map(std::path::PathBuf::from);
    let scope_paths = supergrok::scope::ScopePaths::with_overrides(
        auth_override.as_deref(),
        config_override.as_deref(),
    )
    .expect("resolve Grok scope paths");
    let cache = xdg_cache_for("supergrok");
    let out = supergrok::fetch_snapshot(&binary, &scope_paths, &cache, Duration::ZERO)
        .await
        .expect("SuperGrok billing should succeed through official Grok Build ACP");

    assert!(
        out.snapshot.weekly_pct >= 0,
        "supergrok: negative weekly_pct — shape changed? {}",
        out.snapshot.weekly_pct
    );
    assert!(!out.snapshot.plan.is_empty(), "supergrok plan label empty");
    if let Some(reset) = out.snapshot.reset_at {
        assert!(
            reset > chrono::Utc::now() - chrono::Duration::days(1),
            "supergrok: reset_at looks implausibly old: {reset:?}"
        );
    }
    println!(
        "✅ supergrok — plan={}, {} {}%, prepaid {:?}, reset {:?}",
        out.snapshot.plan,
        out.snapshot.period.label(),
        out.snapshot.weekly_pct,
        out.snapshot.prepaid_balance,
        out.snapshot.reset_at,
    );
}

/// MiniMax Token Plan — optional: skipped unless a subscription key is present.
///
/// The endpoint answers HTTP 200 even for auth failures, so a green run here is
/// what proves the in-band `base_resp.status_code` check is still doing its job:
/// a wrong key surfaces as an error rather than an all-zero plan.
#[tokio::test]
#[ignore = "live API; run with --ignored"]
async fn minimax_live() {
    let Ok(api_key) = std::env::var("MINIMAX_API_KEY") else {
        eprintln!("minimax_live: MINIMAX_API_KEY is unset — skipping optional MiniMax smoke test");
        return;
    };
    if api_key.trim().is_empty() {
        eprintln!("minimax_live: MINIMAX_API_KEY is empty — skipping optional MiniMax smoke test");
        return;
    }
    let cache = xdg_cache_for("minimax");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap();
    let endpoints = minimax::fetch::Endpoints::default();
    let out = minimax::fetch_snapshot(
        &client,
        &api_key,
        &cache,
        &endpoints,
        Duration::from_secs(0),
    )
    .await
    .expect("minimax fetch should succeed against the real API");

    assert_pct("minimax.session", out.snapshot.session.utilization_pct);
    assert_pct("minimax.weekly", out.snapshot.weekly.utilization_pct);
    // The interval length is read from the payload rather than assumed: it has
    // been observed at both 4h and 5h on the same account. A non-positive one
    // would divide the pace math by nothing.
    assert!(
        out.snapshot.session.window_duration > chrono::Duration::zero(),
        "minimax: interval window has no length — payload shape changed?"
    );
    if let Some(v) = out.snapshot.video_session.as_ref() {
        assert_pct("minimax.video", v.utilization_pct);
    }
    println!(
        "✅ minimax: {} · session {}% · weekly {}% · video {:?}",
        out.snapshot.plan,
        out.snapshot.session.utilization_pct,
        out.snapshot.weekly.utilization_pct,
        out.snapshot
            .video_session
            .as_ref()
            .map(|w| w.utilization_pct),
    );
}

// ---------------------------------------------------------------------------
// Calibration probes — encrypted sync bundle format, plan 1-08.
// ---------------------------------------------------------------------------

/// **CAL-3** — what Argon2id really costs at the parameters this build ships.
///
/// The 1582 ms in the research is one Apple M3 Max number, and the shipped
/// default plus the memory floor should not rest on it alone. This times the
/// production parameters and then two steps down, so a user who must lower
/// `--kdf-memory` on constrained hardware has a curve to choose from rather
/// than a single point.
///
/// `#[ignore]`d because it allocates a gibibyte and takes seconds, and the AUR
/// `check()` runs `cargo test` on other people's machines. Run it in release:
/// a debug-build Argon2 timing measures the optimiser, not the KDF.
///
/// ```bash
/// cargo test --release --test live -- --ignored --nocapture \
///     cal3_argon2id_timing_at_production_parameters
/// ```
#[test]
#[ignore = "calibration; allocates 1 GiB and takes seconds — run with --ignored --release"]
fn cal3_argon2id_timing_at_production_parameters() {
    use ai_usagebar::sync::crypto::{KdfParams, available_memory_kib, derive_kek};

    /// The shipped default, spelled out. If it ever drifts from
    /// [`KdfParams::default`], this probe calibrates something nobody runs.
    const PRODUCTION: KdfParams = KdfParams {
        m_kib: 1_048_576,
        t: 3,
        p: 1,
    };
    assert_eq!(
        PRODUCTION,
        KdfParams::default(),
        "the first row must be the parameters this build actually ships"
    );

    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    let available = available_memory_kib()
        .map(|kib| format!("{} MiB", kib / 1024))
        .unwrap_or_else(|| "unreported on this platform".into());

    println!(
        "CAL-3 — {}/{}, {profile} profile, available memory {available}",
        std::env::consts::ARCH,
        std::env::consts::OS,
    );
    if cfg!(debug_assertions) {
        println!("  WARNING: a debug-build number is meaningless — re-run with --release");
    }

    // A fixed salt and a fixed password: this measures work, not secrecy, and
    // random inputs would only make two runs incomparable.
    let salt = [0x5au8; 16];
    let password = b"calibration only, never a real passphrase";

    for m_kib in [PRODUCTION.m_kib, PRODUCTION.m_kib / 2, PRODUCTION.m_kib / 4] {
        let params = KdfParams {
            m_kib,
            ..PRODUCTION
        };
        let started = std::time::Instant::now();
        let kek = derive_kek(password, &salt, params).expect("derivation must succeed");
        let elapsed = started.elapsed();
        // Read the result so the derivation cannot be optimised away.
        assert_eq!(kek.len(), 32);
        println!(
            "  m={:>5} MiB  t={}  p={}  ->  {:>6} ms",
            m_kib / 1024,
            params.t,
            params.p,
            elapsed.as_millis(),
        );
    }
}

/// **CAL-1** — does a private-repo release asset honour a `Range:` request
/// after the redirect to signed storage?
///
/// It decides pack sizing. If ranged reads work, one chunk can be fetched out
/// of a large pack and packs may grow; if they do not, fetching one chunk means
/// fetching its whole pack, and `sync::pack::PACK_TARGET` stays at the recorded
/// 32 MiB fallback where that waste is tolerable.
///
/// **Setup** — a throwaway private repository with one release carrying an
/// asset a little over 1 MiB (large enough that a whole-body `200` is
/// unmistakable, small enough to download inside the timeout), and a
/// fine-grained read-only PAT scoped to it. Delete the repository and revoke
/// the token afterwards.
///
/// ```bash
/// GSD_CAL1_TOKEN=github_pat_… \
/// GSD_CAL1_REPO=owner/throwaway-repo \
/// GSD_CAL1_ASSET=payload.bin \
///   cargo test --test live -- --ignored --nocapture \
///     cal1_range_on_private_release_asset
/// ```
///
/// Skips with a printed message when the token is absent, so it is never a hard
/// failure on a machine that was not set up for it. Reading an environment
/// variable inside an `#[ignore]`d live test is the same carve-out every other
/// probe in this file already uses; nothing in the default `cargo test` set
/// touches the network.
#[tokio::test]
#[ignore = "live API; needs a throwaway private repo and token — run with --ignored"]
async fn cal1_range_on_private_release_asset() {
    let Some(token) = non_empty_var("GSD_CAL1_TOKEN") else {
        eprintln!(
            "cal1_range_on_private_release_asset: GSD_CAL1_TOKEN is unset — skipping; \
             the 32 MiB pack fallback recorded in docs/sync-format.md stands"
        );
        return;
    };
    let (Some(repo), Some(asset_name)) = (
        non_empty_var("GSD_CAL1_REPO"),
        non_empty_var("GSD_CAL1_ASSET"),
    ) else {
        eprintln!(
            "cal1_range_on_private_release_asset: GSD_CAL1_REPO (owner/name) and \
             GSD_CAL1_ASSET (the asset's file name) must both be set — skipping"
        );
        return;
    };

    // Redirects are *not* followed automatically: the hop to signed storage is
    // the thing being measured, and following it silently would also replay the
    // GitHub token to a storage host that neither needs nor should see it.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    // The asset's *API* url, not `browser_download_url`: the latter is an HTML
    // endpoint a token cannot authenticate.
    let release: serde_json::Value = client
        .get(format!(
            "https://api.github.com/repos/{repo}/releases/latest"
        ))
        .header("authorization", format!("Bearer {token}"))
        .header("accept", "application/vnd.github+json")
        .header("user-agent", CAL1_UA)
        .send()
        .await
        .expect("the release lookup must reach api.github.com")
        // A 401/404 here is a broken setup, and must never be recorded as
        // "Range is unsupported".
        .error_for_status()
        .expect("the release lookup must succeed — check the repo name and the token's scope")
        .json()
        .await
        .expect("the release lookup must return JSON");

    let asset = release["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|a| a["name"].as_str() == Some(asset_name.as_str()))
        .unwrap_or_else(|| {
            panic!("the latest release of {repo} carries no asset named {asset_name:?}")
        });
    let asset_url = asset["url"].as_str().expect("asset api url").to_string();
    let asset_size = asset["size"].as_u64().unwrap_or_default();

    println!("CAL-1 — {repo} asset {asset_name:?}, {asset_size} bytes");
    if asset_size <= 1024 * 1024 {
        println!("  WARNING: the asset is under 1 MiB — a whole-body 200 will be hard to tell");
    }

    let mut response = client
        .get(&asset_url)
        .header("authorization", format!("Bearer {token}"))
        .header("accept", "application/octet-stream")
        .header("user-agent", CAL1_UA)
        .header("range", "bytes=0-1023")
        .send()
        .await
        .expect("the ranged asset request must reach api.github.com");

    if response.status().is_redirection() {
        let location = header_value(&response, "location")
            .expect("a redirect response must carry a Location header");
        // Only the host is printed: a signed storage URL carries its
        // credential in the query string, and this output goes to a terminal.
        println!(
            "  {} -> signed storage at {}",
            response.status(),
            reqwest::Url::parse(&location)
                .ok()
                .and_then(|u| u.host_str().map(str::to_string))
                .unwrap_or_else(|| "an unparseable location".into()),
        );
        response = client
            .get(&location)
            .header("user-agent", CAL1_UA)
            .header("range", "bytes=0-1023")
            .send()
            .await
            .expect("the signed-storage request must succeed");
    }

    let status = response.status();
    let content_range = header_value(&response, "content-range");
    let content_length = header_value(&response, "content-length");
    let received = response.bytes().await.expect("a readable body").len();

    println!(
        "  status {status}, content-range {content_range:?}, \
         content-length {content_length:?}, {received} bytes received"
    );
    assert!(
        status.is_success(),
        "the ranged fetch failed with {status} — that is a broken probe, not an answer about Range"
    );
    if status.as_u16() == 206 && content_range.is_some() {
        println!(
            "  CAL-1 = Range IS honoured. Phase 3 may raise sync::pack::PACK_TARGET above 32 MiB."
        );
    } else {
        println!(
            "  CAL-1 = Range is NOT honoured ({received} of {asset_size} bytes). \
             The 32 MiB PACK_TARGET fallback stands."
        );
    }
}

/// User agent for the CAL-1 probe. GitHub's API requires one.
const CAL1_UA: &str = "ai-usagebar-cal1";

/// `Some` only for a variable that is both set and not blank — an exported but
/// empty variable is how a half-finished setup usually looks, and it should
/// skip rather than send an empty bearer token.
fn non_empty_var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

fn header_value(response: &reqwest::Response, name: &str) -> Option<String> {
    response
        .headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}
