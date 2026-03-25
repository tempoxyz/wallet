//! Fund command — open browser to fund your Tempo wallet.

use std::time::{Duration, Instant};

use url::Url;

use crate::{
    analytics,
    analytics::{WalletFundFailurePayload, WalletFundPayload},
    wallet::{query_all_balances, TokenBalance},
};
use tempo_common::{
    cli::{context::Context, output::OutputFormat},
    error::{ConfigError, InputError, TempoError},
    keys::Keystore,
    security::sanitize_error,
};

/// Interval between balance poll attempts (seconds).
const POLL_INTERVAL_SECS: u64 = 3;

/// Maximum time to wait for balance change (seconds).
const CALLBACK_TIMEOUT_SECS: u64 = 900;

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub(crate) async fn run(
    ctx: &Context,
    address: Option<String>,
    no_browser: bool,
) -> Result<(), TempoError> {
    let method = fund_method(no_browser);
    track_fund_start(ctx, method);
    let result = run_inner(ctx, address, no_browser).await;
    track_fund_result(ctx, method, &result);
    result
}

async fn run_inner(
    ctx: &Context,
    address: Option<String>,
    no_browser: bool,
) -> Result<(), TempoError> {
    let wallet_address = resolve_address(address, &ctx.keys)?;

    let before = query_all_balances(&ctx.config, ctx.network, &wallet_address).await;

    let auth_server_url =
        std::env::var("TEMPO_AUTH_URL").unwrap_or_else(|_| ctx.network.auth_url().to_string());

    let parsed_url = Url::parse(&auth_server_url).map_err(|source| InputError::UrlParseFor {
        context: "auth server",
        source,
    })?;
    let base_url = parsed_url.origin().ascii_serialization();
    let fund_url = format!("{base_url}/?action=fund");

    if ctx.output_format == OutputFormat::Text {
        eprintln!("Fund URL: {fund_url}");
    }

    super::auth::try_open_browser(&fund_url, no_browser);

    if ctx.output_format == OutputFormat::Text {
        eprintln!("Waiting for funding...");
    }

    let start = Instant::now();
    let timeout = Duration::from_secs(CALLBACK_TIMEOUT_SECS);
    let interval = Duration::from_secs(POLL_INTERVAL_SECS);

    loop {
        if start.elapsed() >= timeout {
            if ctx.output_format == OutputFormat::Text {
                eprintln!(
                    "Timed out waiting for funding after {} minutes.",
                    CALLBACK_TIMEOUT_SECS / 60
                );
            }
            return Ok(());
        }

        tokio::time::sleep(interval).await;

        let current = query_all_balances(&ctx.config, ctx.network, &wallet_address).await;

        if has_balance_changed(&before, &current) {
            if ctx.output_format == OutputFormat::Text {
                eprintln!("\nFunding received!");
                render_balance_diff(&before, &current);
            }
            return Ok(());
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolve the target wallet address from an explicit arg or the keystore default.
fn resolve_address(address: Option<String>, keys: &Keystore) -> Result<String, TempoError> {
    if let Some(addr) = address {
        let parsed = tempo_common::security::parse_address_input(&addr, "wallet address")?;
        return Ok(format!("{parsed:#x}"));
    }

    keys.wallet_address_hex().ok_or_else(|| {
        ConfigError::Missing("No wallet configured. Run 'tempo wallet login'.".to_string()).into()
    })
}

/// Returns `true` if any token balance differs between `initial` and `current`.
fn has_balance_changed(initial: &[TokenBalance], current: &[TokenBalance]) -> bool {
    if current.len() != initial.len() {
        return true;
    }
    for cur in current {
        let prev = initial.iter().find(|b| b.token == cur.token);
        match prev {
            Some(prev) if prev.balance != cur.balance => return true,
            None => return true,
            _ => {}
        }
    }
    false
}

/// Render per-token balance changes to stderr.
fn render_balance_diff(before: &[TokenBalance], after: &[TokenBalance]) {
    for cur in after {
        let prev = before
            .iter()
            .find(|b| b.token == cur.token)
            .map_or("0", |b| b.balance.as_str());
        if cur.balance != prev {
            eprintln!("  {} balance: {} -> {}", cur.symbol, prev, cur.balance);
        }
    }
}

fn fund_method(no_browser: bool) -> &'static str {
    if no_browser {
        "manual"
    } else {
        "browser"
    }
}

// ---------------------------------------------------------------------------
// Analytics
// ---------------------------------------------------------------------------

fn track_fund_start(ctx: &Context, method: &str) {
    ctx.track(
        analytics::WALLET_FUND_STARTED,
        WalletFundPayload {
            network: ctx.network.as_str().to_string(),
            method: method.to_string(),
        },
    );
}

fn track_fund_result(ctx: &Context, method: &str, result: &Result<(), TempoError>) {
    match result {
        Ok(()) => {
            ctx.track(
                analytics::WALLET_FUND_SUCCESS,
                WalletFundPayload {
                    network: ctx.network.as_str().to_string(),
                    method: method.to_string(),
                },
            );
        }
        Err(e) => {
            ctx.track(
                analytics::WALLET_FUND_FAILURE,
                WalletFundFailurePayload {
                    network: ctx.network.as_str().to_string(),
                    method: method.to_string(),
                    error: sanitize_error(&e.to_string()),
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fund_method;

    #[test]
    fn fund_method_uses_manual_only_when_no_browser_is_true() {
        assert_eq!(fund_method(true), "manual");
        assert_eq!(fund_method(false), "browser");
    }
}
