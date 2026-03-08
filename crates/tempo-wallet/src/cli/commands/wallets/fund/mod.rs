//! Fund command — request testnet faucet tokens or bridge USDC to Tempo mainnet.

mod bridge;
mod faucet;
mod relay;

use std::time::{Duration, Instant};

use serde::Serialize;

use crate::account::TokenBalance;
use crate::cli::Context;
use tempo_common::error::TempoError;
use tempo_common::keys::Keystore;
use tempo_common::network::NetworkId;

use relay::DepositStatus;

/// Interval between balance/status poll attempts (seconds).
const POLL_INTERVAL_SECS: u64 = 3;

// ---------------------------------------------------------------------------
// JSON response
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub(super) struct FundResponse {
    network: String,
    address: String,
    action: &'static str,
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    deposit_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_chain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bridge_status: Option<DepositStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    balances_before: Option<Vec<TokenBalance>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    balances_after: Option<Vec<TokenBalance>>,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub(super) async fn run(
    ctx: &Context,
    address: Option<String>,
    no_wait: bool,
) -> anyhow::Result<()> {
    let wallet_address = resolve_address(address, &ctx.keys)?;

    let wait = !no_wait;
    match ctx.network {
        NetworkId::TempoModerato => faucet::run(ctx, &wallet_address, wait).await,
        NetworkId::Tempo => bridge::run(ctx, &wallet_address, wait).await,
    }
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Resolve the target wallet address from an explicit arg or the keystore default.
fn resolve_address(address: Option<String>, keys: &Keystore) -> anyhow::Result<String> {
    if let Some(addr) = address {
        return Ok(addr);
    }

    let wallet_addr = keys.wallet_address();

    if wallet_addr.is_empty() {
        anyhow::bail!(TempoError::ConfigMissing(
            "No wallet configured. Run 'tempo-wallet login' or 'tempo-wallet wallets create'."
                .to_string(),
        ));
    }

    Ok(wallet_addr.to_string())
}

/// Generic polling helper: calls `poll_fn` every `interval` until it returns
/// `Some(T)` or `timeout` elapses.
pub(super) async fn poll_until<F, Fut, T>(
    timeout: Duration,
    interval: Duration,
    mut poll_fn: F,
) -> Option<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Option<T>>,
{
    let start = Instant::now();
    loop {
        if start.elapsed() > timeout {
            return None;
        }
        tokio::time::sleep(interval).await;
        if let Some(result) = poll_fn().await {
            return Some(result);
        }
    }
}

/// Returns `true` if any token balance differs between `initial` and `current`.
pub(super) fn has_balance_changed(initial: &[TokenBalance], current: &[TokenBalance]) -> bool {
    if current.len() != initial.len() {
        return true;
    }
    for cur in current {
        let prev = initial.iter().find(|b| b.currency == cur.currency);
        match prev {
            Some(prev) if !balances_equal(&prev.balance, &cur.balance) => return true,
            None => return true,
            _ => {}
        }
    }
    false
}

/// Compare two balance strings numerically to handle different decimal
/// representations (e.g. "1.0" vs "1.000000").
fn balances_equal(a: &str, b: &str) -> bool {
    match (a.parse::<f64>(), b.parse::<f64>()) {
        (Ok(va), Ok(vb)) => (va - vb).abs() < 1e-9,
        _ => a == b,
    }
}

/// Render per-token balance changes to stderr.
pub(super) fn render_balance_diff(before: &[TokenBalance], after: &[TokenBalance]) {
    for cur in after {
        let prev = before
            .iter()
            .find(|b| b.currency == cur.currency)
            .map(|b| b.balance.as_str())
            .unwrap_or("0");
        if !balances_equal(&cur.balance, prev) {
            eprintln!("  {} balance: {} -> {}", cur.symbol, prev, cur.balance);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tb(symbol: &str, currency: &str, balance: &str) -> TokenBalance {
        TokenBalance {
            symbol: symbol.to_string(),
            currency: currency.to_string(),
            balance: balance.to_string(),
        }
    }

    #[test]
    fn balances_equal_numeric_equivalence() {
        assert!(balances_equal("1.0", "1.000000"));
        assert!(balances_equal("0", "0.000000"));
        assert!(balances_equal("100.5", "100.5"));
        assert!(!balances_equal("1.0", "2.0"));
    }

    #[test]
    fn balances_equal_non_numeric_fallback() {
        assert!(balances_equal("abc", "abc"));
        assert!(!balances_equal("abc", "def"));
    }

    #[test]
    fn has_balance_changed_same() {
        let a = vec![tb("USDC", "0xabc", "1.000000")];
        let b = vec![tb("USDC", "0xabc", "1.0")];
        assert!(!has_balance_changed(&a, &b));
    }

    #[test]
    fn has_balance_changed_different_amount() {
        let a = vec![tb("USDC", "0xabc", "1.0")];
        let b = vec![tb("USDC", "0xabc", "2.0")];
        assert!(has_balance_changed(&a, &b));
    }

    #[test]
    fn has_balance_changed_new_currency() {
        let a = vec![tb("USDC", "0xabc", "1.0")];
        let b = vec![tb("USDC", "0xabc", "1.0"), tb("ETH", "0xdef", "0.5")];
        assert!(has_balance_changed(&a, &b));
    }

    #[test]
    fn has_balance_changed_zero_unchanged() {
        let a = vec![tb("USDC", "0xabc", "0")];
        let b = vec![tb("USDC", "0xabc", "0.000000")];
        assert!(!has_balance_changed(&a, &b));
    }
}
