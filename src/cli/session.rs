//! Session management commands.

use anyhow::{Context, Result};
use serde::Serialize;

use super::OutputFormat;
use crate::config::Config;
use crate::network::Network;
use crate::payment::session::{
    close_channel_by_id, close_discovered_channel, close_session_from_record,
    find_all_channels_for_payer, CloseOutcome,
};
use crate::payment::session_store;
use crate::util::format_u256_with_decimals;
use crate::wallet::credentials::WalletCredentials;

/// Resolved view of a session record for display.
struct SessionView<'a> {
    origin: &'a str,
    network: &'a str,
    channel_id: &'a str,
    symbol: &'a str,
    unlimited: bool,
    limit: String,
    spent: String,
    remaining: String,
}

impl<'a> SessionView<'a> {
    fn from_record(session: &'a session_store::SessionRecord) -> Self {
        let token_config = session
            .network_name
            .parse::<Network>()
            .ok()
            .and_then(|n| n.token_config_by_address(&session.currency));
        let symbol = token_config.map(|t| t.symbol).unwrap_or("tokens");
        let decimals = token_config.map(|t| t.decimals).unwrap_or(6);

        let spent_u = session.cumulative_amount_u128().unwrap_or(0);
        let limit_u = session.deposit_u128().unwrap_or(0);
        let remaining_u = limit_u.saturating_sub(spent_u);

        Self {
            origin: &session.origin,
            network: &session.network_name,
            channel_id: &session.channel_id,
            symbol,
            unlimited: limit_u == 0,
            limit: format_u256_with_decimals(alloy::primitives::U256::from(limit_u), decimals),
            spent: format_u256_with_decimals(alloy::primitives::U256::from(spent_u), decimals),
            remaining: format_u256_with_decimals(
                alloy::primitives::U256::from(remaining_u),
                decimals,
            ),
        }
    }
}

/// Format seconds as a human-readable duration (e.g., "15m 0s", "2m 30s").
fn format_duration(secs: u64) -> String {
    if secs >= 60 {
        let m = secs / 60;
        let s = secs % 60;
        if s == 0 {
            format!("{m}m")
        } else {
            format!("{m}m {s}s")
        }
    } else {
        format!("{secs}s")
    }
}

/// Resolve the grace period for an escrow contract, falling back to 900s.
async fn resolve_grace_period(config: &Config, network_name: &str, escrow_hex: &str) -> u64 {
    use crate::payment::session::read_grace_period;

    let network_info = match config.resolve_network(network_name) {
        Ok(info) => info,
        Err(_) => return 900,
    };
    let rpc_url: url::Url = match network_info.rpc_url.parse() {
        Ok(u) => u,
        Err(_) => return 900,
    };
    let provider = alloy::providers::RootProvider::<alloy::network::Ethereum>::new_http(rpc_url);
    let escrow: alloy::primitives::Address = match escrow_hex.parse() {
        Ok(a) => a,
        Err(_) => return 900,
    };
    read_grace_period(&provider, escrow).await.unwrap_or(900)
}

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
    let views: Vec<SessionView> = filtered.iter().map(SessionView::from_record).collect();

    // Cross-reference pending closes so we can show "closed" instead of "active"
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let pending_map: std::collections::HashMap<String, u64> =
        session_store::list_all_pending_closes()
            .unwrap_or_default()
            .into_iter()
            .map(|p| (p.channel_id.to_lowercase(), p.ready_at.saturating_sub(now)))
            .collect();

    match output_format {
        OutputFormat::Json => {
            #[derive(Serialize)]
            struct Item<'a> {
                channel_id: &'a str,
                network: &'a str,
                origin: &'a str,
                symbol: &'a str,
                deposit: &'a str,
                spent: &'a str,
                remaining: &'a str,
                status: &'a str,
                #[serde(skip_serializing_if = "Option::is_none")]
                remaining_secs: Option<u64>,
            }

            let items: Vec<Item> = views
                .iter()
                .map(|v| {
                    let (status, remaining_secs) =
                        if let Some(&secs) = pending_map.get(v.channel_id) {
                            ("closed", Some(secs))
                        } else {
                            ("active", None)
                        };
                    Item {
                        channel_id: v.channel_id,
                        network: v.network,
                        origin: v.origin,
                        symbol: v.symbol,
                        deposit: &v.limit,
                        spent: &v.spent,
                        remaining: &v.remaining,
                        status,
                        remaining_secs,
                    }
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string(&serde_json::json!({
                    "sessions": items,
                    "total": items.len(),
                }))?
            );
        }
        OutputFormat::Text => {
            if views.is_empty() {
                println!("No active sessions.");
                return Ok(());
            }

            for v in &views {
                let status = if let Some(&secs) = pending_map.get(v.channel_id) {
                    if secs == 0 {
                        "closed — ready to finalize".to_string()
                    } else {
                        format!("closed — {} remaining", format_duration(secs))
                    }
                } else {
                    "active".to_string()
                };
                println!("{}", v.origin);
                println!("{:>10}: {}", "Network", v.network);
                println!("{:>10}: {}", "Channel", v.channel_id);
                if v.unlimited {
                    println!("{:>10}: unlimited", "Deposit");
                } else {
                    let w = [v.limit.len(), v.spent.len(), v.remaining.len()]
                        .into_iter()
                        .max()
                        .unwrap_or(0);
                    println!("{:>10}: {:>w$} {}", "Deposit", v.limit, v.symbol);
                    println!("{:>10}: {:>w$} {}", "Spent", v.spent, v.symbol);
                    println!("{:>10}: {:>w$} {}", "Remaining", v.remaining, v.symbol);
                }
                println!("{:>10}: {}", "Status", status);
                println!();
            }

            println!("{} session(s) total.", views.len());
        }
    }
    Ok(())
}

/// Unified channel view for `--all` listing.
struct UnifiedView {
    channel_id: String,
    network: String,
    origin: String,
    symbol: &'static str,
    deposit: String,
    spent: String,
    remaining: String,
    status: &'static str,
    /// Seconds until the channel is withdrawable (0 = ready now, None = not closing).
    remaining_secs: Option<u64>,
}

/// List all channels in a unified view: active, orphaned, and closed.
async fn list_all_channels(
    config: &Config,
    output_format: OutputFormat,
    network: Option<&str>,
) -> Result<()> {
    use crate::payment::session::query_channel_state;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut views: Vec<UnifiedView> = Vec::new();

    // Phase 1: local active sessions
    let sessions = session_store::list_sessions()?;
    let local_ids: std::collections::HashSet<String> = sessions
        .iter()
        .map(|s| s.channel_id.to_lowercase())
        .collect();

    // Cross-reference pending closes to mark sessions as "closed"
    let pending_map: std::collections::HashMap<String, u64> =
        session_store::list_all_pending_closes()
            .unwrap_or_default()
            .into_iter()
            .map(|p| (p.channel_id.to_lowercase(), p.ready_at.saturating_sub(now)))
            .collect();

    for session in &sessions {
        if let Some(net) = network {
            if session.network_name != net {
                continue;
            }
        }
        let token_config = session
            .network_name
            .parse::<Network>()
            .ok()
            .and_then(|n| n.token_config_by_address(&session.currency));
        let symbol = token_config.map(|t| t.symbol).unwrap_or("tokens");
        let decimals = token_config.map(|t| t.decimals).unwrap_or(6);
        let spent_u = session.cumulative_amount_u128().unwrap_or(0);
        let limit_u = session.deposit_u128().unwrap_or(0);
        let remaining_u = limit_u.saturating_sub(spent_u);

        let (status, close_remaining_secs) =
            if let Some(&secs) = pending_map.get(&session.channel_id) {
                ("closed", Some(secs))
            } else {
                ("active", None)
            };

        views.push(UnifiedView {
            channel_id: session.channel_id.clone(),
            network: session.network_name.clone(),
            origin: session.origin.clone(),
            symbol,
            deposit: format_u256_with_decimals(alloy::primitives::U256::from(limit_u), decimals),
            spent: format_u256_with_decimals(alloy::primitives::U256::from(spent_u), decimals),
            remaining: format_u256_with_decimals(
                alloy::primitives::U256::from(remaining_u),
                decimals,
            ),
            status,
            remaining_secs: close_remaining_secs,
        });
    }

    // Phase 2: on-chain orphaned channels (requires wallet)
    if let Ok(creds) = WalletCredentials::load() {
        if creds.has_wallet() {
            if let Ok(wallet_addr) = creds.wallet_address().parse() {
                let channels = find_all_channels_for_payer(config, wallet_addr, network).await;

                // Cache grace period per escrow contract to avoid redundant RPC calls
                let mut grace_cache: std::collections::HashMap<String, u64> =
                    std::collections::HashMap::new();

                for ch in &channels {
                    if local_ids.contains(&ch.channel_id) {
                        continue;
                    }
                    let token_config = ch
                        .network
                        .parse::<Network>()
                        .ok()
                        .and_then(|n| n.token_config_by_address(&ch.token));
                    let symbol = token_config.map(|t| t.symbol).unwrap_or("tokens");
                    let decimals = token_config.map(|t| t.decimals).unwrap_or(6);
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
                    } else {
                        ("orphaned", None)
                    };
                    views.push(UnifiedView {
                        channel_id: ch.channel_id.clone(),
                        network: ch.network.clone(),
                        origin: String::new(),
                        symbol,
                        deposit: format_u256_with_decimals(
                            alloy::primitives::U256::from(ch.deposit),
                            decimals,
                        ),
                        spent: format_u256_with_decimals(
                            alloy::primitives::U256::from(ch.settled),
                            decimals,
                        ),
                        remaining: format_u256_with_decimals(
                            alloy::primitives::U256::from(remaining_u),
                            decimals,
                        ),
                        status,
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
            Ok(Some((token, dep, set, net))) => {
                let token_config = net
                    .parse::<Network>()
                    .ok()
                    .and_then(|n| n.token_config_by_address(&token));
                let sym = token_config.map(|t| t.symbol).unwrap_or("tokens");
                let dec = token_config.map(|t| t.decimals).unwrap_or(6);
                let rem = dep.saturating_sub(set);
                (
                    sym,
                    format_u256_with_decimals(alloy::primitives::U256::from(dep), dec),
                    format_u256_with_decimals(alloy::primitives::U256::from(set), dec),
                    format_u256_with_decimals(alloy::primitives::U256::from(rem), dec),
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
        views.push(UnifiedView {
            channel_id: p.channel_id.clone(),
            network: p.network.clone(),
            origin: String::new(),
            symbol,
            deposit,
            spent,
            remaining,
            status: "closed",
            remaining_secs: Some(p.ready_at.saturating_sub(now)),
        });
    }

    match output_format {
        OutputFormat::Json => {
            #[derive(Serialize)]
            struct Item<'a> {
                channel_id: &'a str,
                network: &'a str,
                origin: Option<&'a str>,
                symbol: &'a str,
                deposit: &'a str,
                spent: &'a str,
                remaining: &'a str,
                status: &'a str,
                #[serde(skip_serializing_if = "Option::is_none")]
                remaining_secs: Option<u64>,
            }
            let items: Vec<Item> = views
                .iter()
                .map(|v| Item {
                    channel_id: &v.channel_id,
                    network: &v.network,
                    origin: if v.origin.is_empty() {
                        None
                    } else {
                        Some(&v.origin)
                    },
                    symbol: v.symbol,
                    deposit: &v.deposit,
                    spent: &v.spent,
                    remaining: &v.remaining,
                    status: v.status,
                    remaining_secs: v.remaining_secs,
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string(&serde_json::json!({
                    "sessions": items,
                    "total": items.len(),
                }))?
            );
        }
        OutputFormat::Text => {
            if views.is_empty() {
                println!("No sessions found.");
                return Ok(());
            }
            for v in &views {
                if v.origin.is_empty() {
                    println!("{}", v.channel_id);
                } else {
                    println!("{}", v.origin);
                }
                println!("{:>10}: {}", "Network", v.network);
                println!("{:>10}: {}", "Channel", v.channel_id);
                let w = [v.deposit.len(), v.spent.len(), v.remaining.len()]
                    .into_iter()
                    .max()
                    .unwrap_or(0);
                println!("{:>10}: {:>w$} {}", "Deposit", v.deposit, v.symbol);
                println!("{:>10}: {:>w$} {}", "Spent", v.spent, v.symbol);
                println!("{:>10}: {:>w$} {}", "Remaining", v.remaining, v.symbol);
                let status = match v.remaining_secs {
                    Some(0) => format!("{} — ready to finalize", v.status),
                    Some(secs) => {
                        format!("{} — {} remaining", v.status, format_duration(secs))
                    }
                    None => v.status.to_string(),
                };
                println!("{:>10}: {}", "Status", status);
                println!();
            }
            println!("{} session(s) total.", views.len());
        }
    }
    Ok(())
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
    let local_ids: std::collections::HashSet<String> = local_sessions
        .iter()
        .map(|s| s.channel_id.to_lowercase())
        .collect();

    let channels = find_all_channels_for_payer(config, wallet_addr, network).await;
    let orphaned: Vec<_> = channels
        .iter()
        .filter(|ch| !local_ids.contains(&ch.channel_id.to_lowercase()))
        .collect();

    // Resolve token symbols and format amounts
    struct OrphanedView<'a> {
        ch: &'a crate::payment::session::DiscoveredChannel,
        symbol: &'static str,
        deposit: String,
        settled: String,
        remaining: String,
        status: &'static str,
    }

    let views: Vec<OrphanedView> = orphaned
        .iter()
        .map(|ch| {
            let token_config = ch
                .network
                .parse::<Network>()
                .ok()
                .and_then(|n| n.token_config_by_address(&ch.token));
            let symbol = token_config.map(|t| t.symbol).unwrap_or("tokens");
            let decimals = token_config.map(|t| t.decimals).unwrap_or(6);
            let remaining_u = ch.deposit.saturating_sub(ch.settled);
            let status = if ch.close_requested_at > 0 {
                "closed"
            } else {
                "orphaned"
            };
            OrphanedView {
                ch,
                symbol,
                deposit: format_u256_with_decimals(
                    alloy::primitives::U256::from(ch.deposit),
                    decimals,
                ),
                settled: format_u256_with_decimals(
                    alloy::primitives::U256::from(ch.settled),
                    decimals,
                ),
                remaining: format_u256_with_decimals(
                    alloy::primitives::U256::from(remaining_u),
                    decimals,
                ),
                status,
            }
        })
        .collect();

    match output_format {
        OutputFormat::Json => {
            #[derive(Serialize)]
            struct Item<'a> {
                channel_id: &'a str,
                network: &'a str,
                symbol: &'a str,
                deposit: &'a str,
                spent: &'a str,
                remaining: &'a str,
                status: &'a str,
            }
            let items: Vec<Item> = views
                .iter()
                .map(|v| Item {
                    channel_id: &v.ch.channel_id,
                    network: &v.ch.network,
                    symbol: v.symbol,
                    deposit: &v.deposit,
                    spent: &v.settled,
                    remaining: &v.remaining,
                    status: v.status,
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string(&serde_json::json!({
                    "sessions": items,
                    "total": items.len(),
                }))?
            );
        }
        OutputFormat::Text => {
            if views.is_empty() {
                println!("No orphaned sessions found.");
                return Ok(());
            }
            for v in &views {
                println!("{}", v.ch.channel_id);
                println!("{:>10}: {}", "Network", v.ch.network);
                let w = [v.deposit.len(), v.settled.len(), v.remaining.len()]
                    .into_iter()
                    .max()
                    .unwrap_or(0);
                println!("{:>10}: {:>w$} {}", "Deposit", v.deposit, v.symbol);
                println!("{:>10}: {:>w$} {}", "Spent", v.settled, v.symbol);
                println!("{:>10}: {:>w$} {}", "Remaining", v.remaining, v.symbol);
                println!("{:>10}: {}", "Status", v.status);
                println!();
            }
            println!("{} orphaned session(s).", views.len());
        }
    }
    Ok(())
}

/// List channels pending finalization (requestClose submitted).
///
/// Queries on-chain state for each pending channel to show deposit/settled/remaining.
async fn list_pending_closes(config: &Config, output_format: OutputFormat) -> Result<()> {
    use crate::payment::session::query_channel_state;

    let pending = session_store::list_all_pending_closes()?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    struct PendingView {
        channel_id: String,
        network_name: String,
        symbol: &'static str,
        deposit: String,
        settled: String,
        remaining: String,
        status: String,
        remaining_secs: u64,
    }

    let mut views = Vec::new();
    for p in &pending {
        let remaining_secs = p.ready_at.saturating_sub(now);
        let status = if remaining_secs == 0 {
            "ready to finalize".to_string()
        } else {
            format!("{} remaining", format_duration(remaining_secs))
        };

        // Try to get on-chain state for richer display
        let (symbol, deposit, settled, remaining) = match query_channel_state(
            config,
            &p.channel_id,
            &p.network,
        )
        .await
        {
            Ok(Some((token, dep, set, net))) => {
                let token_config = net
                    .parse::<Network>()
                    .ok()
                    .and_then(|n| n.token_config_by_address(&token));
                let sym = token_config.map(|t| t.symbol).unwrap_or("tokens");
                let dec = token_config.map(|t| t.decimals).unwrap_or(6);
                let rem = dep.saturating_sub(set);
                (
                    sym,
                    format_u256_with_decimals(alloy::primitives::U256::from(dep), dec),
                    format_u256_with_decimals(alloy::primitives::U256::from(set), dec),
                    format_u256_with_decimals(alloy::primitives::U256::from(rem), dec),
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

        views.push(PendingView {
            channel_id: p.channel_id.clone(),
            network_name: p.network.clone(),
            symbol,
            deposit,
            settled,
            remaining,
            status,
            remaining_secs,
        });
    }

    match output_format {
        OutputFormat::Json => {
            #[derive(Serialize)]
            struct Item<'a> {
                channel_id: &'a str,
                network: &'a str,
                symbol: &'a str,
                deposit: &'a str,
                spent: &'a str,
                remaining: &'a str,
                status: &'a str,
                remaining_secs: u64,
            }
            let items: Vec<Item> = views
                .iter()
                .map(|v| Item {
                    channel_id: &v.channel_id,
                    network: &v.network_name,
                    symbol: v.symbol,
                    deposit: &v.deposit,
                    spent: &v.settled,
                    remaining: &v.remaining,
                    status: "closed",
                    remaining_secs: v.remaining_secs,
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string(&serde_json::json!({
                    "sessions": items,
                    "total": items.len(),
                }))?
            );
        }
        OutputFormat::Text => {
            if views.is_empty() {
                println!("No sessions pending finalization.");
                return Ok(());
            }
            for v in &views {
                println!("{}", v.channel_id);
                println!("{:>10}: {}", "Network", v.network_name);
                let w = [v.deposit.len(), v.settled.len(), v.remaining.len()]
                    .into_iter()
                    .max()
                    .unwrap_or(0);
                println!("{:>10}: {:>w$} {}", "Deposit", v.deposit, v.symbol);
                println!("{:>10}: {:>w$} {}", "Spent", v.settled, v.symbol);
                println!("{:>10}: {:>w$} {}", "Remaining", v.remaining, v.symbol);
                println!("{:>10}: {}", "Status", v.status);
                println!();
            }
            println!("{} session(s) pending.", views.len());
        }
    }
    Ok(())
}

/// Close only orphaned on-chain channels (channels with no local session record).
async fn close_orphaned_channels(
    config: &Config,
    output_format: OutputFormat,
    show_output: bool,
    network: Option<&str>,
) -> Result<()> {
    let creds = WalletCredentials::load().context("No wallet configured")?;
    anyhow::ensure!(creds.has_wallet(), "No wallet configured");
    let wallet_addr = creds
        .wallet_address()
        .parse()
        .context("Invalid wallet address")?;

    let local_sessions = session_store::list_sessions()?;
    let local_ids: std::collections::HashSet<String> = local_sessions
        .iter()
        .map(|s| s.channel_id.to_lowercase())
        .collect();

    if show_output {
        eprintln!("Scanning on-chain for orphaned channels...");
    }

    let channels = find_all_channels_for_payer(config, wallet_addr, network).await;
    let orphaned: Vec<_> = channels
        .iter()
        .filter(|ch| !local_ids.contains(&ch.channel_id.to_lowercase()))
        .collect();

    if orphaned.is_empty() {
        match output_format {
            OutputFormat::Json => println!(
                "{}",
                serde_json::json!({"closed": 0, "pending": 0, "failed": 0, "results": []})
            ),
            OutputFormat::Text => println!("No orphaned channels found."),
        }
        return Ok(());
    }

    let mut closed = 0u32;
    let mut pending = 0u32;
    let mut failed = 0u32;
    let mut results: Vec<serde_json::Value> = Vec::new();
    // Track nonce offsets per network so sequential txs don't collide.
    let mut nonce_offsets: std::collections::HashMap<String, u64> =
        std::collections::HashMap::new();

    for ch in &orphaned {
        if show_output {
            eprintln!("Closing {}...", ch.channel_id);
        }
        let offset = nonce_offsets.get(&ch.network).copied().unwrap_or(0);
        match close_discovered_channel(ch, config, offset).await {
            Ok(CloseOutcome::Closed) => {
                closed += 1;
                *nonce_offsets.entry(ch.network.clone()).or_default() += 1;
                // Clean up any pending close and session records
                let _ = session_store::delete_pending_close(&ch.channel_id);
                let _ = session_store::delete_session_by_channel_id(&ch.channel_id);
                results.push(serde_json::json!({
                    "channel_id": ch.channel_id,
                    "status": "closed",
                }));
            }
            Ok(CloseOutcome::Pending { remaining_secs }) => {
                pending += 1;
                *nonce_offsets.entry(ch.network.clone()).or_default() += 1;
                if show_output {
                    eprintln!(
                        "  Pending — {} remaining, run `presto session close --closed` to finalize.",
                        format_duration(remaining_secs)
                    );
                }
                results.push(serde_json::json!({
                    "channel_id": ch.channel_id,
                    "status": "pending",
                    "remaining_secs": remaining_secs,
                }));
            }
            Err(e) => {
                if show_output {
                    eprintln!("  Error: {e}");
                }
                failed += 1;
                results.push(serde_json::json!({
                    "channel_id": ch.channel_id,
                    "status": "error",
                    "error": e.to_string()
                }));
            }
        }
    }

    match output_format {
        OutputFormat::Json => println!(
            "{}",
            serde_json::json!({"closed": closed, "pending": pending, "failed": failed, "results": results})
        ),
        OutputFormat::Text => {
            let total = closed + pending + failed;
            if total == 0 {
                println!("No orphaned sessions found.");
            } else {
                let mut parts = Vec::new();
                if closed > 0 {
                    parts.push(format!("{closed} closed"));
                }
                if pending > 0 {
                    parts.push(format!("{pending} pending"));
                }
                if failed > 0 {
                    parts.push(format!("{failed} failed"));
                }
                println!("{}", parts.join(", "));
            }
        }
    }
    Ok(())
}

/// Close a session by URL or close all sessions.
///
/// When `--all` is used, this first closes local sessions, then scans on-chain
/// for any orphaned channels belonging to the current wallet and closes those too.
#[allow(clippy::too_many_arguments)]
pub async fn close_sessions(
    config: &Config,
    url: Option<String>,
    all: bool,
    orphaned: bool,
    closed: bool,
    output_format: OutputFormat,
    show_output: bool,
    network: Option<&str>,
) -> Result<()> {
    if closed {
        return finalize_closed_channels(config, output_format, show_output, network).await;
    }
    if orphaned {
        return close_orphaned_channels(config, output_format, show_output, network).await;
    }
    if all {
        let mut closed = 0u32;
        let mut pending = 0u32;
        let mut failed = 0u32;
        let mut results: Vec<serde_json::Value> = Vec::new();

        // Phase 1: close local sessions
        let sessions = session_store::list_sessions()?;
        for session in &sessions {
            let key = session_store::session_key(&session.origin);
            if show_output {
                eprintln!("Closing {}...", session.origin);
            }
            match close_session_from_record(session, config).await {
                Ok(CloseOutcome::Closed) => {
                    closed += 1;
                    if let Err(e) = session_store::delete_session(&key) {
                        if show_output {
                            eprintln!("  Failed to remove local session: {e}");
                        }
                    }
                    let _ = session_store::delete_pending_close(&session.channel_id);
                    results.push(serde_json::json!({
                        "origin": session.origin,
                        "channel_id": session.channel_id,
                        "status": "closed",
                    }));
                }
                Ok(CloseOutcome::Pending { remaining_secs }) => {
                    pending += 1;
                    if show_output {
                        eprintln!(
                            "  Pending — {} remaining, run `presto session close --closed` to finalize.",
                            format_duration(remaining_secs)
                        );
                    }
                    results.push(serde_json::json!({
                        "origin": session.origin,
                        "channel_id": session.channel_id,
                        "status": "pending",
                        "remaining_secs": remaining_secs,
                    }));
                }
                Err(e) => {
                    if show_output {
                        eprintln!("  Error: {e}");
                    }
                    failed += 1;
                    results.push(serde_json::json!({
                        "origin": session.origin,
                        "channel_id": session.channel_id,
                        "status": "error",
                        "error": e.to_string(),
                    }));
                }
            }
        }

        // Phase 2: scan on-chain for orphaned channels
        let local_channel_ids: std::collections::HashSet<&str> =
            sessions.iter().map(|s| s.channel_id.as_str()).collect();

        if let Ok(creds) = WalletCredentials::load() {
            if creds.has_wallet() {
                if let Ok(wallet_addr) = creds.wallet_address().parse() {
                    if show_output {
                        eprintln!("Scanning on-chain for orphaned channels...");
                    }

                    let channels = find_all_channels_for_payer(config, wallet_addr, network).await;

                    let mut nonce_offsets: std::collections::HashMap<String, u64> =
                        std::collections::HashMap::new();
                    for ch in &channels {
                        if local_channel_ids.contains(ch.channel_id.as_str()) {
                            continue;
                        }
                        if show_output {
                            eprintln!("Closing {}...", ch.channel_id);
                        }
                        let offset = nonce_offsets.get(&ch.network).copied().unwrap_or(0);
                        match close_discovered_channel(ch, config, offset).await {
                            Ok(CloseOutcome::Closed) => {
                                closed += 1;
                                *nonce_offsets.entry(ch.network.clone()).or_default() += 1;
                                let _ = session_store::delete_pending_close(&ch.channel_id);
                                results.push(serde_json::json!({
                                    "channel_id": ch.channel_id,
                                    "status": "closed",
                                }));
                            }
                            Ok(CloseOutcome::Pending { remaining_secs }) => {
                                pending += 1;
                                *nonce_offsets.entry(ch.network.clone()).or_default() += 1;
                                if show_output {
                                    eprintln!(
                                        "  Pending — {} remaining, run `presto session close --closed` to finalize.",
                                        format_duration(remaining_secs)
                                    );
                                }
                                results.push(serde_json::json!({
                                    "channel_id": ch.channel_id,
                                    "status": "pending",
                                    "remaining_secs": remaining_secs,
                                }));
                            }
                            Err(e) => {
                                if show_output {
                                    eprintln!("  Error: {e}");
                                }
                                failed += 1;
                                results.push(serde_json::json!({
                                    "channel_id": ch.channel_id,
                                    "status": "error",
                                    "error": e.to_string()
                                }));
                            }
                        }
                    }
                }
            }
        }

        match output_format {
            OutputFormat::Json => println!(
                "{}",
                serde_json::json!({"closed": closed, "pending": pending, "failed": failed, "results": results})
            ),
            OutputFormat::Text => {
                let total = closed + pending + failed;
                if total == 0 {
                    println!("No active sessions to close.");
                } else {
                    let mut parts = Vec::new();
                    if closed > 0 {
                        parts.push(format!("{closed} closed"));
                    }
                    if pending > 0 {
                        parts.push(format!("{pending} pending"));
                    }
                    if failed > 0 {
                        parts.push(format!("{failed} failed"));
                    }
                    println!("{}", parts.join(", "));
                }
            }
        }
        return Ok(());
    }

    if let Some(ref target) = url {
        // If the target looks like a channel ID (0x-prefixed hex), close on-chain directly
        if target.starts_with("0x") && target.len() == 66 {
            if show_output {
                eprintln!("Closing {target}...");
            }
            match close_channel_by_id(config, target, network, None).await {
                Ok(CloseOutcome::Closed) => {
                    let _ = session_store::delete_pending_close(target);
                    let _ = session_store::delete_session_by_channel_id(target);
                    if output_format == OutputFormat::Json {
                        println!(
                            "{}",
                            serde_json::json!({"closed": 1, "pending": 0, "failed": 0, "results": [{"channel_id": target, "status": "closed"}]})
                        );
                    } else {
                        println!("Channel {target} closed.");
                    }
                }
                Ok(CloseOutcome::Pending { remaining_secs }) => {
                    if output_format == OutputFormat::Json {
                        println!(
                            "{}",
                            serde_json::json!({"closed": 0, "pending": 1, "failed": 0, "results": [{"channel_id": target, "status": "pending", "remaining_secs": remaining_secs}]})
                        );
                    } else {
                        println!(
                            "Channel {target}: close requested — {} remaining, run `presto session close --closed` to finalize.",
                            format_duration(remaining_secs)
                        );
                    }
                }
                Err(e) => {
                    // "not found on any network" means the channel is already
                    // fully closed on-chain. Clean up stale local records.
                    let err_msg = e.to_string();
                    if err_msg.contains("not found on any network") {
                        let _ = session_store::delete_pending_close(target);
                        let _ = session_store::delete_session_by_channel_id(target);
                        if output_format == OutputFormat::Json {
                            println!(
                                "{}",
                                serde_json::json!({"closed": 1, "pending": 0, "failed": 0, "results": [{"channel_id": target, "status": "closed"}]})
                            );
                        } else {
                            println!("Channel {target} already closed.");
                        }
                    } else if output_format == OutputFormat::Json {
                        println!(
                            "{}",
                            serde_json::json!({"closed": 0, "pending": 0, "failed": 1, "results": [{"channel_id": target, "status": "error", "error": err_msg}]})
                        );
                    } else {
                        anyhow::bail!("{e}");
                    }
                }
            }
            return Ok(());
        }

        // Otherwise treat as a URL — close the local session
        let key = session_store::session_key(target);
        let session = session_store::load_session(&key)?;

        if let Some(record) = session {
            if show_output {
                eprintln!("Closing {target}...");
            }
            match close_session_from_record(&record, config).await {
                Ok(CloseOutcome::Closed) => {
                    if let Err(e) = session_store::delete_session(&key) {
                        if show_output {
                            eprintln!("  Failed to remove local session: {e}");
                        }
                    }
                    let _ = session_store::delete_pending_close(&record.channel_id);
                    if output_format == OutputFormat::Json {
                        println!(
                            "{}",
                            serde_json::json!({"closed": 1, "pending": 0, "failed": 0, "results": [{"origin": target, "channel_id": record.channel_id, "status": "closed"}]})
                        );
                    } else {
                        println!("Session for {target} closed.");
                    }
                }
                Ok(CloseOutcome::Pending { remaining_secs }) => {
                    if output_format == OutputFormat::Json {
                        println!(
                            "{}",
                            serde_json::json!({"closed": 0, "pending": 1, "failed": 0, "results": [{"origin": target, "channel_id": record.channel_id, "status": "pending", "remaining_secs": remaining_secs}]})
                        );
                    } else {
                        println!(
                            "Session for {target}: close requested — {} remaining, run `presto session close --closed` to finalize.",
                            format_duration(remaining_secs)
                        );
                    }
                }
                Err(e) => {
                    if output_format == OutputFormat::Json {
                        println!(
                            "{}",
                            serde_json::json!({"closed": 0, "pending": 0, "failed": 1, "results": [{"origin": target, "channel_id": record.channel_id, "status": "error", "error": e.to_string()}]})
                        );
                    } else {
                        anyhow::bail!("{e}");
                    }
                }
            }
        } else if output_format == OutputFormat::Json {
            println!(
                "{}",
                serde_json::json!({"closed": 0, "pending": 0, "failed": 1, "results": [{"origin": target, "status": "error", "error": "no active session"}]})
            );
        } else {
            println!("No active session for {target}");
        }

        return Ok(());
    }

    anyhow::bail!(
        "Specify a URL, channel ID (0x...), or use --all/--orphaned/--closed to close sessions"
    );
}

/// Finalize channels that have had requestClose() submitted and whose grace period has elapsed.
async fn finalize_closed_channels(
    config: &Config,
    output_format: OutputFormat,
    show_output: bool,
    network: Option<&str>,
) -> Result<()> {
    let all_pending = session_store::list_all_pending_closes()?;
    let pending: Vec<_> = if let Some(net) = network {
        all_pending
            .into_iter()
            .filter(|p| p.network == net)
            .collect()
    } else {
        all_pending
    };

    if pending.is_empty() {
        match output_format {
            OutputFormat::Json => println!(
                "{}",
                serde_json::json!({"closed": 0, "pending": 0, "failed": 0, "results": []})
            ),
            OutputFormat::Text => println!("No channels pending finalization."),
        }
        return Ok(());
    }

    let mut closed = 0u32;
    let mut still_pending = 0u32;
    let mut failed = 0u32;
    let mut results: Vec<serde_json::Value> = Vec::new();

    // Cache wallet signers per network to avoid redundant disk I/O
    let mut signer_cache: std::collections::HashMap<String, crate::wallet::signer::WalletSigner> =
        std::collections::HashMap::new();

    for record in &pending {
        if show_output {
            eprintln!("Finalizing {}...", record.channel_id);
        }

        // Load signer once per network
        if !signer_cache.contains_key(&record.network) {
            match crate::wallet::signer::load_wallet_signer(&record.network) {
                Ok(w) => {
                    signer_cache.insert(record.network.clone(), w);
                }
                Err(e) => {
                    if show_output {
                        eprintln!("  Error loading wallet for {}: {e}", record.network);
                    }
                    failed += 1;
                    results.push(serde_json::json!({
                        "channel_id": record.channel_id,
                        "status": "error",
                        "error": e.to_string(),
                    }));
                    continue;
                }
            }
        }
        let wallet = signer_cache.get(&record.network);

        match close_channel_by_id(config, &record.channel_id, Some(&record.network), wallet).await {
            Ok(CloseOutcome::Closed) => {
                if let Err(e) = session_store::delete_pending_close(&record.channel_id) {
                    tracing::warn!(%e, "failed to delete pending close record");
                }
                if let Err(e) = session_store::delete_session_by_channel_id(&record.channel_id) {
                    tracing::warn!(%e, "failed to delete session record");
                }
                closed += 1;
                results.push(serde_json::json!({
                    "channel_id": record.channel_id,
                    "status": "closed",
                }));
            }
            Ok(CloseOutcome::Pending { remaining_secs }) => {
                still_pending += 1;
                if show_output {
                    eprintln!("  Pending — {} remaining.", format_duration(remaining_secs));
                }
                results.push(serde_json::json!({
                    "channel_id": record.channel_id,
                    "status": "pending",
                    "remaining_secs": remaining_secs,
                }));
            }
            Err(e) => {
                let err_msg = e.to_string();
                if err_msg.contains("not found on any network") {
                    // Channel already finalized externally — clean up stale record
                    let _ = session_store::delete_pending_close(&record.channel_id);
                    let _ = session_store::delete_session_by_channel_id(&record.channel_id);
                    closed += 1;
                    results.push(serde_json::json!({
                        "channel_id": record.channel_id,
                        "status": "closed",
                    }));
                } else {
                    if show_output {
                        eprintln!("  Error: {e}");
                    }
                    failed += 1;
                    results.push(serde_json::json!({
                        "channel_id": record.channel_id,
                        "status": "error",
                        "error": err_msg,
                    }));
                }
            }
        }
    }

    match output_format {
        OutputFormat::Json => println!(
            "{}",
            serde_json::json!({"closed": closed, "pending": still_pending, "failed": failed, "results": results})
        ),
        OutputFormat::Text => {
            let total = closed + still_pending + failed;
            if total == 0 {
                println!("No sessions pending finalization.");
            } else {
                let mut parts = Vec::new();
                if closed > 0 {
                    parts.push(format!("{closed} finalized"));
                }
                if still_pending > 0 {
                    parts.push(format!("{still_pending} pending"));
                }
                if failed > 0 {
                    parts.push(format!("{failed} failed"));
                }
                println!("{}", parts.join(", "));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration_zero() {
        assert_eq!(format_duration(0), "0s");
    }

    #[test]
    fn test_format_duration_seconds() {
        assert_eq!(format_duration(1), "1s");
        assert_eq!(format_duration(59), "59s");
    }

    #[test]
    fn test_format_duration_exact_minutes() {
        assert_eq!(format_duration(60), "1m");
        assert_eq!(format_duration(120), "2m");
        assert_eq!(format_duration(900), "15m");
    }

    #[test]
    fn test_format_duration_minutes_and_seconds() {
        assert_eq!(format_duration(61), "1m 1s");
        assert_eq!(format_duration(90), "1m 30s");
        assert_eq!(format_duration(125), "2m 5s");
    }

    #[test]
    fn test_format_duration_large() {
        assert_eq!(format_duration(3600), "60m");
        assert_eq!(format_duration(3661), "61m 1s");
    }
}
