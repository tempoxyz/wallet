use std::collections::HashMap;

use alloy::primitives::utils::format_units;
use alloy::primitives::{Address, U256};
use anyhow::Result;

use crate::cli::OutputFormat;
use crate::config::Config;
use crate::keys::Keystore;
use crate::network::NetworkId;
use crate::payment::session::store as session_store;
use crate::payment::session::{find_all_channels_for_payer, read_grace_period};

use super::render::{render_channel_list, ChannelView};

// ---------------------------------------------------------------------------
// Utilities (list-only)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SessionState {
    Active,
    Closing,
    Finalizable,
    Orphaned,
}

/// Build a `ChannelView` from a local session record.
fn view_from_session(session: &session_store::SessionRecord) -> ChannelView {
    let t = session.network_id().token();
    let (symbol, decimals) = (t.symbol, t.decimals);

    let spent_u = session.cumulative_amount_u128().unwrap_or(0);
    let limit_u = session.deposit_u128().unwrap_or(0);
    let remaining_u = limit_u.saturating_sub(spent_u);

    let (status, remaining_secs) = session.status_at(session_store::now_secs());

    ChannelView {
        channel_id: session.channel_id.clone(),
        network: session.network_name.clone(),
        origin: Some(session.origin.clone()),
        symbol,
        deposit: format_units(U256::from(limit_u), decimals).expect("decimals <= 77"),
        spent: format_units(U256::from(spent_u), decimals).expect("decimals <= 77"),
        remaining: format_units(U256::from(remaining_u), decimals).expect("decimals <= 77"),
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

/// Resolve the grace period for an escrow contract, falling back to 900s.
async fn resolve_grace_period(config: &Config, network: NetworkId, escrow_hex: &str) -> u64 {
    let rpc_url = config.rpc_url(network);
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
pub(super) async fn list_sessions(
    config: &Config,
    output_format: OutputFormat,
    states: &[SessionState],
    network: NetworkId,
    keys: &Keystore,
) -> Result<()> {
    // Default to active when no state filter is provided
    let selected: Vec<SessionState> = if states.is_empty() {
        vec![SessionState::Active]
    } else {
        states.to_vec()
    };

    // Local sessions (active/closing/finalizable)
    let sessions = session_store::list_sessions()?;
    let filtered_local: Vec<_> = {
        let net = network.as_str();
        sessions.iter().filter(|s| s.network_name == net).collect()
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

    if let Some(wallet_addr) = need_orphaned
        .then(|| keys.wallet_address().parse::<Address>().ok())
        .flatten()
        .filter(|_| keys.has_wallet())
    {
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
            let t = network.token();
            let (symbol, decimals) = (t.symbol, t.decimals);
            let remaining_u = ch.deposit.saturating_sub(ch.settled);
            let (status, remaining_secs) = if ch.close_requested_at > 0 {
                let grace = match grace_cache.get(&ch.escrow_contract) {
                    Some(&g) => g,
                    None => {
                        let g = resolve_grace_period(config, ch.network, &ch.escrow_contract).await;
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
                network: ch.network.to_string(),
                origin: Some(String::new()),
                symbol,
                deposit: format_units(U256::from(ch.deposit), decimals).expect("decimals <= 77"),
                spent: format_units(U256::from(ch.settled), decimals).expect("decimals <= 77"),
                remaining: format_units(U256::from(remaining_u), decimals).expect("decimals <= 77"),
                status: status.to_string(),
                remaining_secs,
                created_at: None,
                last_used_at: None,
            });
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
