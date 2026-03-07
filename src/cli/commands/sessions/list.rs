use std::collections::{HashMap, HashSet};

use anyhow::Result;

use super::render::{render_channel_list, ChannelView};
use super::{session_store, SessionStatus};
use crate::cli::args::SessionStateArg;
use crate::cli::Context;
use crate::payment::session::channel::find_all_channels_for_payer;

/// List payment sessions.
///
/// By default lists local active sessions. With `--state all`, shows a unified
/// view of active, orphaned, and closing channels. With `--state orphaned`,
/// scans on-chain for channels without a local session. With `--state finalizable`,
/// shows channels pending finalization (requestClose submitted, awaiting grace period).
pub(super) async fn list_sessions(ctx: &Context, states: Vec<SessionStateArg>) -> Result<()> {
    let config = &ctx.config;
    let output_format = ctx.output_format;
    let network = ctx.network;
    let keys = &ctx.keys;

    // Expand `All` and apply default
    let selected: Vec<SessionStateArg> = if states.is_empty() {
        vec![SessionStateArg::Active]
    } else if states.iter().any(|s| matches!(s, SessionStateArg::All)) {
        vec![
            SessionStateArg::Active,
            SessionStateArg::Closing,
            SessionStateArg::Finalizable,
            SessionStateArg::Orphaned,
        ]
    } else {
        states
            .into_iter()
            .filter(|s| !matches!(s, SessionStateArg::All))
            .collect()
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
        let (status, _) = s.status_at(session_store::now_secs());
        let v = ChannelView::from(*s);
        let matches = match status {
            SessionStatus::Active => selected.contains(&SessionStateArg::Active),
            SessionStatus::Closing => selected.contains(&SessionStateArg::Closing),
            SessionStatus::Finalizable => selected.contains(&SessionStateArg::Finalizable),
            _ => false,
        };
        if matches {
            views.push(v);
        }
    }

    // Orphaned / on-chain closings if requested
    let need_orphaned = selected.contains(&SessionStateArg::Orphaned)
        || selected.contains(&SessionStateArg::Closing)
        || selected.contains(&SessionStateArg::Finalizable);

    if let Some(wallet_addr) = need_orphaned
        .then(|| keys.wallet_address_parsed())
        .flatten()
    {
        let channels = find_all_channels_for_payer(config, wallet_addr, network).await;

        // Avoid duplicates by skipping any with a local session
        let local_ids: HashSet<String> = filtered_local
            .iter()
            .map(|s| s.channel_id.to_lowercase())
            .collect();

        // Cache grace per escrow to reduce RPC chatter
        let mut grace_cache: HashMap<String, u64> = HashMap::new();

        for ch in &channels {
            if local_ids.contains(&ch.channel_id.to_lowercase()) {
                continue;
            }
            let grace = match grace_cache.get(&ch.escrow_contract) {
                Some(&g) => g,
                None => {
                    let g =
                        super::resolve_grace_period(config, ch.network, &ch.escrow_contract).await;
                    grace_cache.insert(ch.escrow_contract.clone(), g);
                    g
                }
            };

            let mut v = ChannelView::from_on_chain(
                &ch.channel_id,
                network,
                ch.deposit,
                ch.settled,
                ch.close_requested_at,
                grace,
            );
            // Show the "Channel" line in text output (origin presence triggers it)
            v.origin = Some(String::new());

            let include = match v.status {
                SessionStatus::Orphaned => selected.contains(&SessionStateArg::Orphaned),
                SessionStatus::Closing => selected.contains(&SessionStateArg::Closing),
                SessionStatus::Finalizable => selected.contains(&SessionStateArg::Finalizable),
                _ => false,
            };
            if !include {
                continue;
            }

            views.push(v);
        }
    }

    // Empty message by primary selection
    let empty_msg = if selected.len() == 1 && selected[0] == SessionStateArg::Active {
        "No active sessions."
    } else if selected
        .iter()
        .all(|s| matches!(s, SessionStateArg::Closing | SessionStateArg::Finalizable))
    {
        "No sessions pending finalization."
    } else if selected.len() == 1 && selected[0] == SessionStateArg::Orphaned {
        "No orphaned sessions found."
    } else {
        "No sessions found."
    };

    render_channel_list(&views, output_format, empty_msg, "session(s) total")
}

#[cfg(test)]
mod tests {
    use super::super::DEFAULT_GRACE_PERIOD_SECS;
    use super::*;

    fn make_record(
        state: SessionStatus,
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
            challenge_echo: "{}".into(),
            state,
            close_requested_at: if state == SessionStatus::Closing {
                grace_ready_at.saturating_sub(DEFAULT_GRACE_PERIOD_SECS)
            } else {
                0
            },
            grace_ready_at,
            created_at: last_used_at.saturating_sub(60),
            last_used_at,
        }
    }

    #[test]
    fn test_view_from_session_active() {
        let now = session_store::now_secs();
        let rec = make_record(SessionStatus::Active, 0, now);
        let view = ChannelView::from(&rec);
        assert_eq!(view.status, SessionStatus::Active);
        assert!(view.remaining_secs.is_none());
    }

    #[test]
    fn test_view_from_session_closing_and_finalizable() {
        let now = session_store::now_secs();
        // Closing with time remaining
        let rec = make_record(SessionStatus::Closing, now + 120, now);
        let view = ChannelView::from(&rec);
        assert_eq!(view.status, SessionStatus::Closing);
        assert_eq!(view.remaining_secs, Some(120));

        // Finalizable (ready_at <= now)
        let rec2 = make_record(SessionStatus::Closing, now, now);
        let view2 = ChannelView::from(&rec2);
        assert_eq!(view2.status, SessionStatus::Finalizable);
        assert_eq!(view2.remaining_secs, Some(0));
    }
}
