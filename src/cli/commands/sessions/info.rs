use alloy::primitives::{Address, B256};
use anyhow::{Context as _, Result};

use super::display::{render_channel_list, ChannelView};
use super::{session_store, DEFAULT_GRACE_PERIOD_SECS};
use crate::cli::Context;
use crate::payment::session::channel::{get_channel_on_chain, read_grace_period};

/// Show details for a local session by URL/origin or for a channel by ID.
pub(super) async fn show_session_info(ctx: &Context, target: &str) -> Result<()> {
    let output_format = ctx.output_format;

    if target.starts_with("0x") && target.len() == 66 {
        return show_channel_info(ctx, target).await;
    }

    // Treat as URL/origin; normalize to origin key
    let key = session_store::session_key(target);
    if let Some(rec) = session_store::load_session(&key)? {
        let view = ChannelView::from(&rec);
        render_channel_list(&[view], output_format, "", "")?;
    } else {
        // No local record — give a helpful message
        if output_format.is_structured() {
            println!(
                "{}",
                output_format.serialize(&serde_json::json!({
                    "sessions": [],
                    "total": 0,
                    "message": "no local session for origin"
                }))?
            );
        } else {
            println!("No local session for {}", target);
            println!(
                "Hint: use 'tempo-wallet sessions list --state orphaned' to view on-chain channels for your wallet."
            );
        }
    }

    Ok(())
}

async fn show_channel_info(ctx: &Context, channel_id_hex: &str) -> Result<()> {
    let config = &ctx.config;
    let output_format = ctx.output_format;
    let network = ctx.network;

    // Prefer local session if available
    let sessions = session_store::list_sessions()?;
    if let Some(rec) = sessions
        .into_iter()
        .find(|s| s.channel_id.eq_ignore_ascii_case(channel_id_hex))
    {
        let view = ChannelView::from(&rec);
        return render_channel_list(&[view], output_format, "", "");
    }

    // Fallback: query single network to locate channel on-chain
    let channel_id: B256 = channel_id_hex
        .parse()
        .context("Invalid channel ID (expected 0x-prefixed bytes32 hex)")?;
    let rpc_url = config.rpc_url(network);
    let provider = alloy::providers::RootProvider::<alloy::network::Ethereum>::new_http(rpc_url);
    let escrow: Address = network
        .escrow_contract()
        .parse()
        .context("Invalid escrow contract address")?;
    let on_chain = match get_channel_on_chain(&provider, escrow, channel_id).await {
        Ok(Some(ch)) => ch,
        Ok(None) => {
            if output_format.is_structured() {
                println!(
                    "{}",
                    output_format.serialize(&serde_json::json!({
                        "sessions": [],
                        "total": 0,
                        "message": format!("channel not found on {}", network)
                    }))?
                );
            } else {
                println!("Channel {channel_id_hex} not found on {network}")
            }
            return Ok(());
        }
        Err(e) => {
            anyhow::bail!("Failed to query channel on {network}: {e}")
        }
    };

    let grace = read_grace_period(&provider, escrow)
        .await
        .unwrap_or(DEFAULT_GRACE_PERIOD_SECS);

    let view = ChannelView::from_on_chain(
        &format!("{:#x}", channel_id),
        network,
        on_chain.deposit,
        on_chain.settled,
        on_chain.close_requested_at,
        grace,
    );
    render_channel_list(&[view], output_format, "", "")
}
