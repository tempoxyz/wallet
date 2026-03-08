use alloy::primitives::{Address, B256};
use anyhow::{Context as _, Result};

use super::render::{render_channel_list, ChannelView};
use super::session_store;
use crate::cli::Context;
use tempo_common::output;
use tempo_common::payment::session::channel::get_channel_on_chain;

#[derive(serde::Serialize)]
struct SessionInfoResponse {
    sessions: Vec<SessionInfoItem>,
    total: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

#[derive(serde::Serialize)]
struct SessionInfoItem;

impl SessionInfoResponse {
    fn with_message(message: impl Into<String>) -> Self {
        Self {
            sessions: Vec::new(),
            total: 0,
            message: Some(message.into()),
        }
    }
}

/// Show details for a local session by URL/origin or for a channel by ID.
pub(super) async fn show_session_info(ctx: &Context, target: &str) -> Result<()> {
    let output_format = ctx.output_format;

    if super::is_channel_id(target) {
        return show_channel_info(ctx, target).await;
    }

    // Treat as URL/origin; normalize to origin key
    let key = session_store::session_key(target);
    if let Some(rec) = session_store::load_session(&key)? {
        let view = ChannelView::from(&rec);
        render_channel_list(&[view], output_format, "", "")?;
    } else {
        // No local record — give a helpful message
        output::emit_by_format(
            output_format,
            &SessionInfoResponse::with_message("no local session for origin"),
            || {
                println!("No local session for {}", target);
                println!(
                    "Hint: use 'tempo-wallet sessions list --state orphaned' to view on-chain channels for your wallet."
                );
                Ok(())
            },
        )?;
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
    let provider = super::make_provider(config, network);
    let escrow: Address = network
        .escrow_contract()
        .parse()
        .context("Invalid escrow contract address")?;
    let on_chain = match get_channel_on_chain(&provider, escrow, channel_id).await {
        Ok(Some(ch)) => ch,
        Ok(None) => {
            output::emit_by_format(
                output_format,
                &SessionInfoResponse::with_message(format!("channel not found on {}", network)),
                || {
                    println!("Channel {channel_id_hex} not found on {network}");
                    Ok(())
                },
            )?;
            return Ok(());
        }
        Err(e) => {
            anyhow::bail!("Failed to query channel on {network}: {e}")
        }
    };

    let grace = super::resolve_grace_period(config, network, network.escrow_contract()).await;

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
