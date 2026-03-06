//! Wallet account types (balances, keys, spending limits) and on-chain queries.

mod query;

pub(crate) use query::query_all_balances;

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use alloy::primitives::utils::format_units;
use alloy::primitives::U256;
use serde::Serialize;

use crate::config::Config;
use crate::keys::{KeyEntry, WalletType};
use crate::network::NetworkId;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub(crate) struct TokenBalance {
    pub symbol: String,
    pub currency: String,
    pub balance: String,
}

/// Spending limit for the key's authorized token.
#[derive(Debug, Serialize)]
pub(crate) struct SpendingLimitInfo {
    pub(crate) unlimited: bool,
    pub(crate) limit: Option<String>,
    pub(crate) remaining: Option<String>,
    pub(crate) spent: Option<String>,
}

/// Key details for JSON output.
#[derive(Debug, Serialize)]
pub(crate) struct KeyInfo {
    pub label: String,
    pub address: String,
    pub wallet_address: Option<String>,
    pub wallet_type: Option<String>,
    pub symbol: Option<String>,
    pub currency: Option<String>,
    pub balance: Option<String>,
    pub spending_limit: Option<SpendingLimitInfo>,
    /// Key expiry as an ISO-8601 UTC timestamp (JSON).
    pub expires_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct KeysResponse {
    pub keys: Vec<KeyInfo>,
    pub total: usize,
}

/// Balance breakdown with locked/available/total.
pub(crate) struct BalanceBreakdown {
    pub total: String,
    pub locked: String,
    pub available: String,
    pub session_count: usize,
}

// ---------------------------------------------------------------------------
// Key info builder
// ---------------------------------------------------------------------------

/// Build a `KeyInfo` from a key entry, querying on-chain data if on the current network.
pub(crate) async fn build_key_info(
    config: &Config,
    network: NetworkId,
    current_chain_id: Option<u64>,
    label: &str,
    entry: &KeyEntry,
    balance_cache: &HashMap<(String, u64), Vec<TokenBalance>>,
) -> KeyInfo {
    let address = entry
        .key_address
        .clone()
        .unwrap_or_else(|| "none".to_string());

    let wt = match entry.wallet_type {
        WalletType::Passkey => "passkey",
        WalletType::Local => "local",
    };

    let on_current_chain = current_chain_id.is_some_and(|cid| cid == entry.chain_id);
    let key_token_info = if on_current_chain {
        query::query_spending_limit(config, network, entry).await
    } else {
        None
    };
    let (symbol, currency, spending_limit) = match key_token_info {
        Some((sym, cur, sl)) => (Some(sym), Some(cur), Some(sl)),
        None => (None, None, None),
    };

    let (wallet_addr, balance) = if entry.wallet_address.is_empty() {
        (None, None)
    } else {
        let cache_key = (entry.wallet_address.clone(), entry.chain_id);
        let bal = currency.as_ref().and_then(|cur| {
            balance_cache
                .get(&cache_key)
                .and_then(|all| all.iter().find(|tb| tb.currency == *cur))
                .map(|tb| tb.balance.clone())
        });
        (Some(entry.wallet_address.clone()), bal)
    };

    let expires_at = key_expiry_timestamp(entry).map(crate::util::format_utc_timestamp);

    KeyInfo {
        label: label.to_string(),
        address,
        wallet_address: wallet_addr,
        wallet_type: Some(wt.to_string()),
        symbol,
        currency,
        balance,
        spending_limit,
        expires_at,
    }
}

// ---------------------------------------------------------------------------
// Display helpers
// ---------------------------------------------------------------------------

/// Print spending limits in compact format to stdout.
pub(crate) fn print_key_limits(key: &KeyInfo) {
    let _ = print_key_limits_to(key, &mut std::io::stdout());
}

/// Print spending limits to a writer.
pub(crate) fn print_key_limits_to(key: &KeyInfo, w: &mut dyn std::io::Write) -> anyhow::Result<()> {
    let sym = key.symbol.as_deref().unwrap_or("tokens");
    if let Some(sl) = &key.spending_limit {
        if sl.unlimited {
            writeln!(w, "{:>10}: unlimited {sym}", "Limit")?;
        } else if let Some(remaining) = &sl.remaining {
            let limit = sl.limit.as_deref().unwrap_or("?");
            let spent = sl.spent.as_deref().unwrap_or("0");
            writeln!(
                w,
                "{:>10}: {spent} / {limit} {sym} ({remaining} remaining)",
                "Limit"
            )?;
        }
    }
    Ok(())
}

/// Extract the expiry timestamp from a key entry's authorization, if present.
/// Returns `None` for keys without an authorization or without an expiry (unlimited).
pub(crate) fn key_expiry_timestamp(key_entry: &KeyEntry) -> Option<u64> {
    key_entry.expiry.filter(|&e| e > 0)
}

/// Format an expiry timestamp as a human-readable countdown for text output.
pub(crate) fn format_expiry_countdown(timestamp: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if timestamp <= now {
        return "expired".to_string();
    }
    let remaining = timestamp - now;
    let days = remaining / 86400;
    let hours = (remaining % 86400) / 3600;
    let minutes = (remaining % 3600) / 60;
    if days > 0 {
        format!("{}d {}h", days, hours)
    } else if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else {
        format!("{}m", minutes)
    }
}

// ---------------------------------------------------------------------------
// Balance breakdown
// ---------------------------------------------------------------------------

/// Compute total balance = available + locked, returning a breakdown.
///
/// `available_str` is the wallet's on-chain `balanceOf`.
/// Locked = sum of (deposit - spent) for sessions with remaining deposits.
pub(crate) fn balance_breakdown(
    available_str: &str,
    sym: &str,
    chain_id: Option<u64>,
) -> Option<BalanceBreakdown> {
    let (locked_str, session_count, decimals) = compute_locked(sym, chain_id)?;

    let available_f64: f64 = available_str.parse().unwrap_or(0.0);
    let locked_f64: f64 = locked_str.parse().unwrap_or(0.0);
    let total_str = format!("{:.width$}", available_f64 + locked_f64, width = decimals);

    Some(BalanceBreakdown {
        total: total_str,
        locked: locked_str,
        available: available_str.to_string(),
        session_count,
    })
}

/// Compute locked balance from sessions with remaining deposits.
///
/// Returns `(locked_formatted, session_count, decimals)` or `None` if no locked balance.
/// Includes expired sessions because funds remain locked in the channel
/// contract until the channel is settled on-chain.
fn compute_locked(sym: &str, chain_id: Option<u64>) -> Option<(String, usize, usize)> {
    use crate::payment::session::store as session_store;

    let sessions = session_store::list_sessions().ok()?;

    if sessions.is_empty() {
        return None;
    }

    let decimals = chain_id
        .and_then(NetworkId::from_chain_id)
        .map(|n| n.token())
        .filter(|t| t.symbol == sym)
        .map(|t| t.decimals as usize)
        .unwrap_or(6);

    let locked_raw: u128 = sessions
        .iter()
        .filter(|s| chain_id.is_none_or(|cid| s.chain_id == cid))
        .filter_map(|s| {
            let deposit = s.deposit_u128().ok()?;
            let spent = s.cumulative_amount_u128().ok()?;
            Some(deposit.saturating_sub(spent))
        })
        .sum();

    if locked_raw == 0 {
        return None;
    }

    let locked_str = format_units(U256::from(locked_raw), decimals as u8).expect("decimals <= 77");

    let count = sessions
        .iter()
        .filter(|s| chain_id.is_none_or(|cid| s.chain_id == cid))
        .count();
    Some((locked_str, count, decimals))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== key_expiry_timestamp ====================

    #[test]
    fn test_key_expiry_timestamp_with_value() {
        let entry = KeyEntry {
            expiry: Some(1750000000),
            ..Default::default()
        };
        assert_eq!(key_expiry_timestamp(&entry), Some(1750000000));
    }

    #[test]
    fn test_key_expiry_timestamp_zero_filtered() {
        // Zero expiry means "unlimited" → filtered to None.
        let entry = KeyEntry {
            expiry: Some(0),
            ..Default::default()
        };
        assert_eq!(key_expiry_timestamp(&entry), None);
    }

    #[test]
    fn test_key_expiry_timestamp_none() {
        let entry = KeyEntry {
            expiry: None,
            ..Default::default()
        };
        assert_eq!(key_expiry_timestamp(&entry), None);
    }

    // ==================== format_expiry_countdown ====================

    #[test]
    fn test_format_expiry_countdown_expired() {
        // Timestamp in the past
        let result = format_expiry_countdown(1000);
        assert_eq!(result, "expired");
    }

    #[test]
    fn test_format_expiry_countdown_far_future() {
        // Very far future timestamp → shows days
        let result = format_expiry_countdown(u64::MAX / 2);
        assert!(result.contains('d'));
    }

    #[test]
    fn test_format_expiry_countdown_format_days() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        // 3 days and 5 hours from now
        let future = now + 3 * 86400 + 5 * 3600;
        let result = format_expiry_countdown(future);
        assert!(result.starts_with("3d"));
    }

    #[test]
    fn test_format_expiry_countdown_format_hours() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        // 5 hours and 30 minutes from now
        let future = now + 5 * 3600 + 30 * 60;
        let result = format_expiry_countdown(future);
        assert!(result.starts_with("5h"));
    }

    #[test]
    fn test_format_expiry_countdown_format_minutes() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        // 42 minutes from now
        let future = now + 42 * 60;
        let result = format_expiry_countdown(future);
        // Should show ~42m (may be 41m due to timing)
        assert!(result.ends_with('m'));
        assert!(!result.contains('d'));
        assert!(!result.contains('h'));
    }
}
