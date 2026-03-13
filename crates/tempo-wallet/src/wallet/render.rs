//! Display and formatting helpers for wallet account data.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use alloy::primitives::utils::format_units;
use alloy::primitives::U256;

use tempo_common::config::Config;
use tempo_common::keys::KeyEntry;
use tempo_common::network::NetworkId;
use tempo_common::payment::session::SessionRecord;

use super::query;
use super::types::{BalanceBreakdown, KeyInfo, TokenBalance};

// ---------------------------------------------------------------------------
// Key info builder
// ---------------------------------------------------------------------------

/// Build a `KeyInfo` from a key entry, querying on-chain data if on the current network.
pub(crate) async fn build_key_info(
    config: &Config,
    network: NetworkId,
    current_chain_id: Option<u64>,
    entry: &KeyEntry,
    balance_cache: &HashMap<(String, u64), Vec<TokenBalance>>,
) -> KeyInfo {
    let address = entry
        .key_address_hex()
        .unwrap_or_else(|| "none".to_string());

    let wt = entry.wallet_type.as_str();

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

    let (wallet_addr, balance) = entry.wallet_address_hex().map_or((None, None), |wallet| {
        let cache_key = (wallet.clone(), entry.chain_id);
        let bal = currency.as_ref().and_then(|cur| {
            balance_cache
                .get(&cache_key)
                .and_then(|all| all.iter().find(|tb| tb.currency == *cur))
                .map(|tb| tb.balance.clone())
        });
        (Some(wallet), bal)
    });

    let expires_at =
        key_expiry_timestamp(entry).map(tempo_common::cli::format::format_utc_timestamp);

    KeyInfo {
        address,
        key: entry.key.as_deref().cloned(),
        chain_id: current_chain_id,
        network: Some(network.as_str().to_string()),
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

/// Label width used by keys/whoami text rendering.
const LABEL_WIDTH: usize = 10;

/// Print spending limits in compact format to stdout.
pub(crate) fn print_key_limits(key: &KeyInfo) {
    let _ = print_key_limits_to(key, &mut std::io::stdout());
}

/// Print spending limits to a writer.
pub(crate) fn print_key_limits_to(
    key: &KeyInfo,
    w: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    let sym = key.symbol.as_deref().unwrap_or("tokens");
    if let Some(sl) = &key.spending_limit {
        if sl.unlimited {
            writeln!(
                w,
                "{:>width$}: unlimited {sym}",
                "Limit",
                width = LABEL_WIDTH
            )?;
        } else if let Some(remaining) = sl.remaining.as_deref() {
            let limit = sl.limit.clone().unwrap_or_else(|| "?".to_string());
            let spent = sl.spent.clone().unwrap_or_else(|| "0".to_string());
            writeln!(
                w,
                "{:>width$}: {spent} / {limit} {sym} ({remaining} remaining)",
                "Limit",
                width = LABEL_WIDTH,
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
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
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
    sessions: &[SessionRecord],
) -> Option<BalanceBreakdown> {
    let (locked_raw, locked_str, session_count, decimals) =
        compute_locked(sym, chain_id, sessions)?;
    let available_raw = parse_fixed_amount(available_str, decimals)?;
    let total_raw = available_raw.saturating_add(locked_raw);
    let total_str = format_units(U256::from(total_raw), decimals as u8).expect("decimals <= 77");

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
fn compute_locked(
    sym: &str,
    chain_id: Option<u64>,
    sessions: &[SessionRecord],
) -> Option<(u128, String, usize, usize)> {
    if sessions.is_empty() {
        return None;
    }

    let decimals = chain_id
        .and_then(NetworkId::from_chain_id)
        .map(|n| n.token())
        .filter(|t| t.symbol == sym)
        .map_or(6, |t| t.decimals as usize);

    let locked_raw: u128 = sessions
        .iter()
        .filter(|s| chain_id.is_none_or(|cid| s.chain_id == cid))
        .map(|s| s.deposit_u128().saturating_sub(s.cumulative_amount_u128()))
        .sum();

    if locked_raw == 0 {
        return None;
    }

    let locked_str = format_units(U256::from(locked_raw), decimals as u8).expect("decimals <= 77");

    let count = sessions
        .iter()
        .filter(|s| chain_id.is_none_or(|cid| s.chain_id == cid))
        .count();
    Some((locked_raw, locked_str, count, decimals))
}

/// Parse a fixed-point decimal string into atomic units with the given scale.
fn parse_fixed_amount(value: &str, decimals: usize) -> Option<u128> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.starts_with('-') {
        return None;
    }

    let mut parts = trimmed.split('.');
    let int_part = parts.next().unwrap_or_default();
    let frac_part = parts.next().unwrap_or_default();
    if parts.next().is_some() {
        return None;
    }

    if !int_part.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if !frac_part.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }

    let int_val = int_part.parse::<u128>().ok()?;
    let scale = 10u128.checked_pow(decimals as u32)?;
    let mut frac = frac_part.to_string();
    if frac.len() > decimals {
        frac.truncate(decimals);
    } else {
        frac.extend(std::iter::repeat_n(
            '0',
            decimals.saturating_sub(frac.len()),
        ));
    }
    let frac_val = if frac.is_empty() {
        0
    } else {
        frac.parse::<u128>().ok()?
    };

    int_val.checked_mul(scale)?.checked_add(frac_val)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== key_expiry_timestamp ====================

    #[test]
    fn test_key_expiry_timestamp_with_value() {
        let entry = KeyEntry {
            expiry: Some(1_750_000_000),
            ..Default::default()
        };
        assert_eq!(key_expiry_timestamp(&entry), Some(1_750_000_000));
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
