use std::collections::{HashMap, HashSet};

use alloy::primitives::{Address, U256};
use anyhow::{Context, Result};

use super::super::OutputFormat;
use crate::config::Config;
use crate::network::resolve_token_meta;
use crate::payment::session::store as session_store;
use crate::payment::session::{
    find_all_channels_for_payer, query_channel_state, read_grace_period,
};
use crate::util::format_u256_with_decimals;
use crate::wallet::credentials::WalletCredentials;

use super::render::{render_channel_list, ChannelView};

// ---------------------------------------------------------------------------
// Utilities (list-only)
// ---------------------------------------------------------------------------

/// Build a `ChannelView` from a local session record, cross-referencing pending closes.
fn view_from_session(
    session: &session_store::SessionRecord,
    pending_map: &HashMap<String, u64>,
) -> ChannelView {
    let (symbol, decimals) = resolve_token_meta(&session.network_name, &session.currency);

    let spent_u = session.cumulative_amount_u128().unwrap_or(0);
    let limit_u = session.deposit_u128().unwrap_or(0);
    let remaining_u = limit_u.saturating_sub(spent_u);

    let (status, remaining_secs) = if let Some(&secs) = pending_map.get(&session.channel_id) {
        ("closed".to_string(), Some(secs))
    } else {
        ("active".to_string(), None)
    };

    ChannelView {
        channel_id: session.channel_id.clone(),
        network: session.network_name.clone(),
        origin: Some(session.origin.clone()),
        symbol,
        deposit: format_u256_with_decimals(U256::from(limit_u), decimals),
        spent: format_u256_with_decimals(U256::from(spent_u), decimals),
        remaining: format_u256_with_decimals(U256::from(remaining_u), decimals),
        status,
        remaining_secs,
    }
}

/// Build a pending-close lookup map: channel_id (lowercase) → seconds remaining.
fn build_pending_map() -> HashMap<String, u64> {
    let now = session_store::now_secs();
    session_store::list_all_pending_closes()
        .unwrap_or_default()
        .into_iter()
        .map(|p| (p.channel_id.to_lowercase(), p.ready_at.saturating_sub(now)))
        .collect()
}

/// Resolve the grace period for an escrow contract, falling back to 900s.
async fn resolve_grace_period(config: &Config, network_name: &str, escrow_hex: &str) -> u64 {
    let network_info = match config.resolve_network(network_name) {
        Ok(info) => info,
        Err(_) => return 900,
    };
    let rpc_url: url::Url = match network_info.rpc_url.parse() {
        Ok(u) => u,
        Err(_) => return 900,
    };
    let provider = alloy::providers::RootProvider::<alloy::network::Ethereum>::new_http(rpc_url);
    let escrow: Address = match escrow_hex.parse() {
        Ok(a) => a,
        Err(_) => return 900,
    };
    read_grace_period(&provider, escrow).await.unwrap_or(900)
}

// ---------------------------------------------------------------------------
// List commands
// ---------------------------------------------------------------------------

/// List payment sessions.
///
/// By default lists local active sessions. With `--all`, shows a unified view
/// of active, orphaned, and closing channels. With `--orphaned`, scans on-chain
/// for channels without a local session. With `--closed`, shows channels
/// pending finalization (requestClose submitted, awaiting grace period).
pub async fn list_sessions(
    config: &Config,
    output_format: OutputFormat,
    all: bool,
    orphaned: bool,
    closed: bool,
    network: Option<&str>,
) -> Result<()> {
    if all {
        return list_all_channels(config, output_format, network).await;
    }
    if orphaned {
        return list_orphaned_channels(config, output_format, network).await;
    }
    if closed {
        return list_pending_closes(config, output_format).await;
    }

    let sessions = session_store::list_sessions()?;
    let filtered: Vec<_> = if let Some(net) = network {
        sessions
            .into_iter()
            .filter(|s| s.network_name == net)
            .collect()
    } else {
        sessions
    };

    let pending_map = build_pending_map();
    let views: Vec<ChannelView> = filtered
        .iter()
        .map(|s| view_from_session(s, &pending_map))
        .collect();

    render_channel_list(
        &views,
        output_format,
        "No active sessions.",
        "session(s) total",
    )
}

/// List all channels in a unified view: active, orphaned, and closed.
async fn list_all_channels(
    config: &Config,
    output_format: OutputFormat,
    network: Option<&str>,
) -> Result<()> {
    let now = session_store::now_secs();
    let mut views: Vec<ChannelView> = Vec::new();

    // Phase 1: local active sessions
    let sessions = session_store::list_sessions()?;
    let local_ids: HashSet<String> = sessions
        .iter()
        .map(|s| s.channel_id.to_lowercase())
        .collect();

    let pending_map = build_pending_map();

    for session in &sessions {
        if let Some(net) = network {
            if session.network_name != net {
                continue;
            }
        }
        views.push(view_from_session(session, &pending_map));
    }

    // Phase 2: on-chain orphaned channels (requires wallet)
    if let Ok(creds) = WalletCredentials::load() {
        if creds.has_wallet() {
            if let Ok(wallet_addr) = creds.wallet_address().parse() {
                let channels = find_all_channels_for_payer(config, wallet_addr, network).await;

                // Cache grace period per escrow contract to avoid redundant RPC calls
                let mut grace_cache: HashMap<String, u64> = HashMap::new();

                for ch in &channels {
                    if local_ids.contains(&ch.channel_id) {
                        continue;
                    }
                    let (symbol, decimals) = resolve_token_meta(&ch.network, &ch.token);
                    let remaining_u = ch.deposit.saturating_sub(ch.settled);
                    let (status, close_remaining_secs) = if ch.close_requested_at > 0 {
                        // Use pending_map if available; otherwise compute from on-chain data
                        let secs = match pending_map.get(&ch.channel_id).copied() {
                            Some(s) => Some(s),
                            None => {
                                // Look up the grace period (cached per escrow contract)
                                let grace = match grace_cache.get(&ch.escrow_contract) {
                                    Some(&g) => g,
                                    None => {
                                        let g = resolve_grace_period(
                                            config,
                                            &ch.network,
                                            &ch.escrow_contract,
                                        )
                                        .await;
                                        grace_cache.insert(ch.escrow_contract.clone(), g);
                                        g
                                    }
                                };
                                let ready_at = ch.close_requested_at + grace;
                                Some(ready_at.saturating_sub(now))
                            }
                        };
                        ("closed", secs)
                    } else if let Some(secs) = pending_map.get(&ch.channel_id).copied() {
                        // requestClose tx was submitted but not yet mined
                        ("closed", Some(secs))
                    } else {
                        ("orphaned", None)
                    };
                    views.push(ChannelView {
                        channel_id: ch.channel_id.clone(),
                        network: ch.network.clone(),
                        origin: Some(String::new()),
                        symbol,
                        deposit: format_u256_with_decimals(U256::from(ch.deposit), decimals),
                        spent: format_u256_with_decimals(U256::from(ch.settled), decimals),
                        remaining: format_u256_with_decimals(U256::from(remaining_u), decimals),
                        status: status.to_string(),
                        remaining_secs: close_remaining_secs,
                    });
                }
            }
        }
    }

    // Phase 3: pending closes not already covered
    let pending = session_store::list_all_pending_closes()?;
    for p in &pending {
        if views.iter().any(|v| v.channel_id == p.channel_id) {
            continue;
        }
        if let Some(net) = network {
            if p.network != net {
                continue;
            }
        }
        let (symbol, deposit, spent, remaining) = match query_channel_state(
            config,
            &p.channel_id,
            &p.network,
        )
        .await
        {
            Ok(Some((token, dep, set))) => {
                let (sym, dec) = resolve_token_meta(&p.network, &token);
                let rem = dep.saturating_sub(set);
                (
                    sym,
                    format_u256_with_decimals(U256::from(dep), dec),
                    format_u256_with_decimals(U256::from(set), dec),
                    format_u256_with_decimals(U256::from(rem), dec),
                )
            }
            Ok(None) => {
                // Channel confirmed not on-chain (finalized) — clean up stale record
                let _ = session_store::delete_pending_close(&p.channel_id);
                let _ = session_store::delete_session_by_channel_id(&p.channel_id);
                continue;
            }
            Err(e) => {
                // RPC/config error — skip but don't delete (may be transient)
                tracing::warn!(%e, channel_id = %p.channel_id, "failed to query channel state, skipping");
                continue;
            }
        };
        views.push(ChannelView {
            channel_id: p.channel_id.clone(),
            network: p.network.clone(),
            origin: Some(String::new()),
            symbol,
            deposit,
            spent,
            remaining,
            status: "closed".to_string(),
            remaining_secs: Some(p.ready_at.saturating_sub(now)),
        });
    }

    render_channel_list(
        &views,
        output_format,
        "No sessions found.",
        "session(s) total",
    )
}

/// List orphaned on-chain channels (no local session record).
async fn list_orphaned_channels(
    config: &Config,
    output_format: OutputFormat,
    network: Option<&str>,
) -> Result<()> {
    let creds = WalletCredentials::load().context("No wallet configured")?;
    anyhow::ensure!(creds.has_wallet(), "No wallet configured");
    let wallet_addr = creds
        .wallet_address()
        .parse()
        .context("Invalid wallet address")?;

    let local_sessions = session_store::list_sessions()?;
    let local_ids: HashSet<String> = local_sessions
        .iter()
        .map(|s| s.channel_id.to_lowercase())
        .collect();

    let channels = find_all_channels_for_payer(config, wallet_addr, network).await;
    let orphaned: Vec<_> = channels
        .iter()
        .filter(|ch| !local_ids.contains(&ch.channel_id.to_lowercase()))
        .collect();

    let views: Vec<ChannelView> = orphaned
        .iter()
        .map(|ch| {
            let (symbol, decimals) = resolve_token_meta(&ch.network, &ch.token);
            let remaining_u = ch.deposit.saturating_sub(ch.settled);
            let status = if ch.close_requested_at > 0 {
                "closed"
            } else {
                "orphaned"
            };
            ChannelView {
                channel_id: ch.channel_id.clone(),
                network: ch.network.clone(),
                origin: None,
                symbol,
                deposit: format_u256_with_decimals(U256::from(ch.deposit), decimals),
                spent: format_u256_with_decimals(U256::from(ch.settled), decimals),
                remaining: format_u256_with_decimals(U256::from(remaining_u), decimals),
                status: status.to_string(),
                remaining_secs: None,
            }
        })
        .collect();

    render_channel_list(
        &views,
        output_format,
        "No orphaned sessions found.",
        "orphaned session(s)",
    )
}

/// List channels pending finalization (requestClose submitted).
///
/// Queries on-chain state for each pending channel to show deposit/settled/remaining.
async fn list_pending_closes(config: &Config, output_format: OutputFormat) -> Result<()> {
    let pending = session_store::list_all_pending_closes()?;
    let now = session_store::now_secs();

    let mut views = Vec::new();
    for p in &pending {
        let remaining_secs = p.ready_at.saturating_sub(now);

        // Try to get on-chain state for richer display
        let (symbol, deposit, settled, remaining) = match query_channel_state(
            config,
            &p.channel_id,
            &p.network,
        )
        .await
        {
            Ok(Some((token, dep, set))) => {
                let (sym, dec) = resolve_token_meta(&p.network, &token);
                let rem = dep.saturating_sub(set);
                (
                    sym,
                    format_u256_with_decimals(U256::from(dep), dec),
                    format_u256_with_decimals(U256::from(set), dec),
                    format_u256_with_decimals(U256::from(rem), dec),
                )
            }
            Ok(None) => {
                // Channel confirmed not on-chain (finalized) — clean up stale record
                let _ = session_store::delete_pending_close(&p.channel_id);
                let _ = session_store::delete_session_by_channel_id(&p.channel_id);
                continue;
            }
            Err(e) => {
                // RPC/config error — skip but don't delete (may be transient)
                tracing::warn!(%e, channel_id = %p.channel_id, "failed to query channel state, skipping");
                continue;
            }
        };

        views.push(ChannelView {
            channel_id: p.channel_id.clone(),
            network: p.network.clone(),
            origin: None,
            symbol,
            deposit,
            spent: settled,
            remaining,
            status: "closed".to_string(),
            remaining_secs: Some(remaining_secs),
        });
    }

    render_channel_list(
        &views,
        output_format,
        "No sessions pending finalization.",
        "session(s) pending",
    )
}
