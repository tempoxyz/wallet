use alloy::primitives::{Address, B256, U256};
use anyhow::{Context, Result};

use super::render::{render_channel_list, render_channel_text, ChannelView};
use crate::cli::OutputFormat;
use crate::config::Config;
use crate::network::resolve_token_meta;
use crate::payment::session::channel::resolve_scan_networks;
use crate::payment::session::read_grace_period;
use crate::payment::session::store as session_store;
use crate::util::format_u256_with_decimals;

/// Show details for a local session by URL/origin or for a channel by ID.
pub async fn show_session_info(
    config: &Config,
    output_format: OutputFormat,
    target: &str,
    network_filter: Option<&str>,
) -> Result<()> {
    if target.starts_with("0x") && target.len() == 66 {
        return show_channel_info(config, output_format, target, network_filter).await;
    }

    // Treat as URL/origin; normalize to origin key
    let key = session_store::session_key(target);
    if let Some(rec) = session_store::load_session(&key)? {
        let (symbol, decimals) = resolve_token_meta(&rec.network_name, &rec.currency);
        let spent_u = rec.cumulative_amount_u128().unwrap_or(0);
        let dep_u = rec.deposit_u128().unwrap_or(0);
        let remaining_u = dep_u.saturating_sub(spent_u);
        let now = session_store::now_secs();
        let (status, remaining_secs) = match rec.state.as_str() {
            "closing" => {
                let rem = rec.grace_ready_at.saturating_sub(now);
                if rem == 0 && rec.grace_ready_at > 0 {
                    ("finalizable".to_string(), Some(0))
                } else {
                    ("closing".to_string(), Some(rem))
                }
            }
            _ => ("active".to_string(), None),
        };

        let view = ChannelView {
            channel_id: rec.channel_id,
            network: rec.network_name,
            origin: Some(rec.origin),
            symbol,
            deposit: format_u256_with_decimals(U256::from(dep_u), decimals),
            spent: format_u256_with_decimals(U256::from(spent_u), decimals),
            remaining: format_u256_with_decimals(U256::from(remaining_u), decimals),
            status,
            remaining_secs,
            created_at: Some(rec.created_at),
            last_used_at: Some(rec.last_used_at),
        };
        match output_format {
            OutputFormat::Text => {
                render_channel_text(&view);
            }
            _ => {
                render_channel_list(&[view], output_format, "", "session(s)")?;
            }
        }
    } else {
        // No local record — give a helpful message
        match output_format {
            OutputFormat::Json | OutputFormat::Toon => {
                println!(
                    "{}",
                    output_format.serialize(&serde_json::json!({
                        "sessions": [],
                        "total": 0,
                        "message": "no local session for origin"
                    }))?
                );
            }
            OutputFormat::Text => {
                println!("No local session for {}", target);
                println!(
                    "Hint: use ' tempo-walletsessions list --orphaned' to view on-chain channels for your wallet."
                );
            }
        }
    }

    Ok(())
}

async fn show_channel_info(
    config: &Config,
    output_format: OutputFormat,
    channel_id_hex: &str,
    network_filter: Option<&str>,
) -> Result<()> {
    // Prefer local session if available
    let sessions = session_store::list_sessions()?;
    if let Some(rec) = sessions
        .into_iter()
        .find(|s| s.channel_id.eq_ignore_ascii_case(channel_id_hex))
    {
        let (symbol, decimals) = resolve_token_meta(&rec.network_name, &rec.currency);
        let spent_u = rec.cumulative_amount_u128().unwrap_or(0);
        let dep_u = rec.deposit_u128().unwrap_or(0);
        let remaining_u = dep_u.saturating_sub(spent_u);
        let now = session_store::now_secs();
        let (status, remaining_secs) = match rec.state.as_str() {
            "closing" => {
                let rem = rec.grace_ready_at.saturating_sub(now);
                if rem == 0 && rec.grace_ready_at > 0 {
                    ("finalizable".to_string(), Some(0))
                } else {
                    ("closing".to_string(), Some(rem))
                }
            }
            _ => ("active".to_string(), None),
        };

        let view = ChannelView {
            channel_id: rec.channel_id,
            network: rec.network_name,
            origin: Some(rec.origin),
            symbol,
            deposit: format_u256_with_decimals(U256::from(dep_u), decimals),
            spent: format_u256_with_decimals(U256::from(spent_u), decimals),
            remaining: format_u256_with_decimals(U256::from(remaining_u), decimals),
            status,
            remaining_secs,
            created_at: Some(rec.created_at),
            last_used_at: Some(rec.last_used_at),
        };
        return render_channel_list(&[view], output_format, "", "session(s)");
    }

    // Fallback: scan networks to locate channel on-chain
    let channel_id: B256 = channel_id_hex
        .parse()
        .context("Invalid channel ID (expected 0x-prefixed bytes32 hex)")?;
    let networks = resolve_scan_networks(network_filter);

    for network in &networks {
        let net_info = match config.resolve_network(network.as_str()) {
            Ok(i) => i,
            Err(_) => continue,
        };
        let rpc_url = crate::network::Network::parse_rpc_url(&net_info.rpc_url)?;
        let provider =
            alloy::providers::RootProvider::<alloy::network::Ethereum>::new_http(rpc_url);
        let escrow: Address = match network.escrow_contract().parse() {
            Ok(a) => a,
            Err(_) => continue,
        };
        let on_chain = match crate::payment::session::channel::get_channel_on_chain(
            &provider, escrow, channel_id,
        )
        .await
        {
            Ok(Some(ch)) => ch,
            Ok(None) => continue,
            Err(_) => continue,
        };

        let token_hex = format!("{:#x}", on_chain.token);
        let (symbol, decimals) = resolve_token_meta(network.as_str(), &token_hex);
        let dep_u = on_chain.deposit;
        let spent_u = on_chain.settled;
        let remaining_u = dep_u.saturating_sub(spent_u);
        let status;
        let mut remaining_secs = None;
        if on_chain.close_requested_at > 0 {
            let grace = read_grace_period(&provider, escrow).await.unwrap_or(900);
            let now = session_store::now_secs();
            let ready_at = on_chain.close_requested_at + grace;
            let rem = ready_at.saturating_sub(now);
            status = if rem == 0 { "finalizable" } else { "closing" };
            remaining_secs = Some(rem);
        } else {
            status = "orphaned";
        }

        let view = ChannelView {
            channel_id: format!("{:#x}", channel_id),
            network: network.as_str().to_string(),
            origin: None,
            symbol,
            deposit: format_u256_with_decimals(U256::from(dep_u), decimals),
            spent: format_u256_with_decimals(U256::from(spent_u), decimals),
            remaining: format_u256_with_decimals(U256::from(remaining_u), decimals),
            status: status.to_string(),
            remaining_secs,
            created_at: None,
            last_used_at: None,
        };
        return match output_format {
            OutputFormat::Text => {
                render_channel_text(&view);
                Ok(())
            }
            _ => render_channel_list(&[view], output_format, "", "session(s)"),
        };
    }

    // Not found anywhere
    match output_format {
        OutputFormat::Json | OutputFormat::Toon => {
            println!(
                "{}",
                output_format.serialize(&serde_json::json!({
                    "sessions": [],
                    "total": 0,
                    "message": "channel not found on any network"
                }))?
            );
        }
        OutputFormat::Text => println!("Channel {channel_id_hex} not found on any network"),
    }
    Ok(())
}
