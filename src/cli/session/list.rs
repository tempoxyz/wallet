use std::collections::{HashMap, HashSet};

use alloy::primitives::{Address, U256};
use anyhow::{Context, Result};

use super::super::OutputFormat;
use crate::config::Config;
use crate::network::resolve_token_meta;
use crate::payment::session::store as session_store;
use crate::payment::session::{find_all_channels_for_payer, read_grace_period};
use crate::util::format_u256_with_decimals;
use crate::wallet::credentials::WalletCredentials;

use super::render::{render_channel_list, ChannelView};

// ---------------------------------------------------------------------------
// Utilities (list-only)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Active,
    Closing,
    Finalizable,
    Orphaned,
}

/// Build a `ChannelView` from a local session record.
fn view_from_session(session: &session_store::SessionRecord) -> ChannelView {
    let (symbol, decimals) = resolve_token_meta(&session.network_name, &session.currency);

    let spent_u = session.cumulative_amount_u128().unwrap_or(0);
    let limit_u = session.deposit_u128().unwrap_or(0);
    let remaining_u = limit_u.saturating_sub(spent_u);

    // Determine status from explicit state in the record
    let now = session_store::now_secs();
    let (status, remaining_secs) = match session.state.as_str() {
        "closing" => {
            let rem = session.grace_ready_at.saturating_sub(now);
            if rem == 0 && session.grace_ready_at > 0 {
                ("finalizable".to_string(), Some(0))
            } else {
                ("closing".to_string(), Some(rem))
            }
        }
        _ => ("active".to_string(), None),
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
        created_at: Some(session.created_at),
        last_used_at: Some(session.last_used_at),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_record(
        state: &str,
        grace_ready_at: u64,
        last_used_at: u64,
    ) -> session_store::SessionRecord {
        session_store::SessionRecord {
            version: 1,
            origin: "https://api.example.com".into(),
            request_url: "https://api.example.com/v1".into(),
            network_name: "tempo".into(),
            chain_id: 4217,
            escrow_contract: "0x00".into(),
            currency: "0x00".into(),
            recipient: "0x00".into(),
            payer: "did:pkh:eip155:4217:0x00".into(),
            authorized_signer: "0x00".into(),
            salt: "0x00".into(),
            channel_id: "0xabc".into(),
            deposit: "1000000".into(),
            tick_cost: "100".into(),
            cumulative_amount: "2000".into(),
            did: "did:pkh:eip155:4217:0x00".into(),
            challenge_echo: "{}".into(),
            challenge_id: "id".into(),
            state: state.into(),
            close_requested_at: if state == "closing" {
                grace_ready_at.saturating_sub(900)
            } else {
                0
            },
            grace_ready_at,
            token_decimals: 6,
            created_at: last_used_at.saturating_sub(60),
            last_used_at,
        }
    }

    #[test]
    fn test_view_from_session_active() {
        let now = session_store::now_secs();
        let rec = make_record("active", 0, now);
        let view = super::view_from_session(&rec);
        assert_eq!(view.status, "active");
        assert!(view.remaining_secs.is_none());
    }

    #[test]
    fn test_view_from_session_closing_and_finalizable() {
        let now = session_store::now_secs();
        // Closing with time remaining
        let rec = make_record("closing", now + 120, now);
        let view = super::view_from_session(&rec);
        assert_eq!(view.status, "closing");
        assert_eq!(view.remaining_secs, Some(120));

        // Finalizable (ready_at <= now)
        let rec2 = make_record("closing", now, now);
        let view2 = super::view_from_session(&rec2);
        assert_eq!(view2.status, "finalizable");
        assert_eq!(view2.remaining_secs, Some(0));
    }
}

// pending_closes removed — derive from session state or on-chain scan

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
    states: &[SessionState],
    network: Option<&str>,
) -> Result<()> {
    // Default to active when no state filter is provided
    let selected: Vec<SessionState> = if states.is_empty() {
        vec![SessionState::Active]
    } else {
        states.to_vec()
    };

    // Local sessions (active/closing/finalizable)
    let sessions = session_store::list_sessions()?;
    let filtered_local: Vec<_> = if let Some(net) = network {
        sessions
            .into_iter()
            .filter(|s| s.network_name == net)
            .collect()
    } else {
        sessions
    };

    let mut views: Vec<ChannelView> = Vec::new();

    // Build local views and filter by selected states
    for s in &filtered_local {
        let v = view_from_session(s);
        let status = v.status.as_str();
        let matches = match status {
            "active" => selected.contains(&SessionState::Active),
            "closing" => selected.contains(&SessionState::Closing),
            "finalizable" => selected.contains(&SessionState::Finalizable),
            _ => false,
        };
        if matches {
            views.push(v);
        }
    }

    // Orphaned / on-chain closings if requested
    let need_orphaned = selected.contains(&SessionState::Orphaned)
        || selected.contains(&SessionState::Closing)
        || selected.contains(&SessionState::Finalizable);

    if need_orphaned {
        if let Ok(creds) = WalletCredentials::load() {
            if creds.has_wallet() {
                if let Ok(wallet_addr) = creds.wallet_address().parse() {
                    let channels = find_all_channels_for_payer(config, wallet_addr, network).await;

                    // Avoid duplicates by skipping any with a local session
                    let local_ids: std::collections::HashSet<String> = filtered_local
                        .iter()
                        .map(|s| s.channel_id.to_lowercase())
                        .collect();

                    // Cache grace per escrow to reduce RPC chatter
                    let mut grace_cache: HashMap<String, u64> = HashMap::new();

                    let now = session_store::now_secs();
                    for ch in &channels {
                        if local_ids.contains(&ch.channel_id.to_lowercase()) {
                            continue;
                        }
                        let (symbol, decimals) = resolve_token_meta(&ch.network, &ch.token);
                        let remaining_u = ch.deposit.saturating_sub(ch.settled);
                        let (status, remaining_secs) = if ch.close_requested_at > 0 {
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
                            let remaining = ready_at.saturating_sub(now);
                            if remaining == 0 {
                                ("finalizable", Some(0))
                            } else {
                                ("closing", Some(remaining))
                            }
                        } else {
                            ("orphaned", None)
                        };

                        let include = match status {
                            "orphaned" => selected.contains(&SessionState::Orphaned),
                            "closing" => selected.contains(&SessionState::Closing),
                            "finalizable" => selected.contains(&SessionState::Finalizable),
                            _ => false,
                        };
                        if !include {
                            continue;
                        }

                        views.push(ChannelView {
                            channel_id: ch.channel_id.clone(),
                            network: ch.network.clone(),
                            origin: Some(String::new()),
                            symbol,
                            deposit: format_u256_with_decimals(U256::from(ch.deposit), decimals),
                            spent: format_u256_with_decimals(U256::from(ch.settled), decimals),
                            remaining: format_u256_with_decimals(U256::from(remaining_u), decimals),
                            status: status.to_string(),
                            remaining_secs,
                            created_at: None,
                            last_used_at: None,
                        });
                    }
                }
            }
        }
    }

    // Empty message by primary selection
    let empty_msg = if selected.len() == 1 && selected[0] == SessionState::Active {
        "No active sessions."
    } else if selected
        .iter()
        .all(|s| matches!(s, SessionState::Closing | SessionState::Finalizable))
    {
        "No sessions pending finalization."
    } else if selected.len() == 1 && selected[0] == SessionState::Orphaned {
        "No orphaned sessions found."
    } else {
        "No sessions found."
    };

    render_channel_list(&views, output_format, empty_msg, "session(s) total")
}

/// List all channels in a unified view: active, orphaned, and closed.
#[allow(dead_code)]
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

    for session in &sessions {
        if let Some(net) = network {
            if session.network_name != net {
                continue;
            }
        }
        views.push(view_from_session(session));
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
                        // Compute from on-chain data
                        let grace = match grace_cache.get(&ch.escrow_contract) {
                            Some(&g) => g,
                            None => {
                                let g =
                                    resolve_grace_period(config, &ch.network, &ch.escrow_contract)
                                        .await;
                                grace_cache.insert(ch.escrow_contract.clone(), g);
                                g
                            }
                        };
                        let ready_at = ch.close_requested_at + grace;
                        let remaining = ready_at.saturating_sub(now);
                        let secs = Some(remaining);
                        (
                            if secs == Some(0) {
                                "finalizable"
                            } else {
                                "closing"
                            },
                            secs,
                        )
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
                        created_at: None,
                        last_used_at: None,
                    });
                }
            }
        }
    }

    // Phase 3 removed: pending_closes no longer used; orphaned closings are covered in Phase 2

    render_channel_list(
        &views,
        output_format,
        "No sessions found.",
        "session(s) total",
    )
}

/// List orphaned on-chain channels (no local session record).
#[allow(dead_code)]
async fn list_orphaned_channels(
    config: &Config,
    output_format: OutputFormat,
    network: Option<&str>,
) -> Result<()> {
    let no_wallet_msg = crate::error::no_wallet_message();
    let creds = WalletCredentials::load().context(no_wallet_msg.clone())?;
    anyhow::ensure!(creds.has_wallet(), "{no_wallet_msg}");
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
                created_at: None,
                last_used_at: None,
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
#[allow(dead_code)]
async fn list_pending_closes(config: &Config, output_format: OutputFormat) -> Result<()> {
    let now = session_store::now_secs();
    let mut views = Vec::new();

    // Local sessions with close requested
    let mut local_ids: HashSet<String> = HashSet::new();
    for s in session_store::list_sessions()? {
        if s.state == "closing" {
            local_ids.insert(s.channel_id.to_lowercase());
            let (symbol, decimals) = resolve_token_meta(&s.network_name, &s.currency);
            let dep = s.deposit_u128().unwrap_or(0);
            let spent = s.cumulative_amount_u128().unwrap_or(0);
            let rem = dep.saturating_sub(spent);
            let remaining_secs = s.grace_ready_at.saturating_sub(now);
            views.push(ChannelView {
                channel_id: s.channel_id,
                network: s.network_name,
                origin: Some(s.origin),
                symbol,
                deposit: format_u256_with_decimals(U256::from(dep), decimals),
                spent: format_u256_with_decimals(U256::from(spent), decimals),
                remaining: format_u256_with_decimals(U256::from(rem), decimals),
                status: if remaining_secs == 0 {
                    "finalizable".into()
                } else {
                    "closing".into()
                },
                remaining_secs: Some(remaining_secs),
                created_at: Some(s.created_at),
                last_used_at: Some(s.last_used_at),
            });
        }
    }

    // Orphaned channels with close requested (skip those already added from local)
    if let Ok(creds) = WalletCredentials::load() {
        if creds.has_wallet() {
            if let Ok(wallet_addr) = creds.wallet_address().parse() {
                let channels = find_all_channels_for_payer(config, wallet_addr, None).await;
                for ch in &channels {
                    if ch.close_requested_at == 0 {
                        continue;
                    }
                    if local_ids.contains(&ch.channel_id.to_lowercase()) {
                        continue;
                    }
                    let (symbol, decimals) = resolve_token_meta(&ch.network, &ch.token);
                    let dep = ch.deposit;
                    let spent = ch.settled;
                    let rem = dep.saturating_sub(spent);
                    // Compute remaining from on-chain grace
                    let grace =
                        resolve_grace_period(config, &ch.network, &ch.escrow_contract).await;
                    let ready_at = ch.close_requested_at + grace;
                    let remaining_secs = ready_at.saturating_sub(now);
                    views.push(ChannelView {
                        channel_id: ch.channel_id.clone(),
                        network: ch.network.clone(),
                        origin: Some(String::new()),
                        symbol,
                        deposit: format_u256_with_decimals(U256::from(dep), decimals),
                        spent: format_u256_with_decimals(U256::from(spent), decimals),
                        remaining: format_u256_with_decimals(U256::from(rem), decimals),
                        status: if remaining_secs == 0 {
                            "finalizable".into()
                        } else {
                            "closing".into()
                        },
                        remaining_secs: Some(remaining_secs),
                        created_at: None,
                        last_used_at: None,
                    });
                }
            }
        }
    }

    render_channel_list(
        &views,
        output_format,
        "No sessions pending finalization.",
        "session(s) pending",
    )
}
// (duplicate removed)
