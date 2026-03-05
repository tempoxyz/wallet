use anyhow::{Context, Result};

use crate::analytics::Analytics;
use crate::cli::OutputFormat;
use crate::config::Config;
use crate::payment::session::store as session_store;
use crate::payment::session::{query_channel_state, read_grace_period};

/// Re-sync a local session's state from on-chain for a given origin.
///
/// This updates the `state`, `close_requested_at`, and `grace_ready_at` fields
/// if the session exists locally and the on-chain channel indicates a pending
/// close. It does not recreate missing sessions (origin→channel mapping is not on-chain).
pub(super) async fn recover_session(
    config: &Config,
    output_format: OutputFormat,
    origin_input: &str,
    analytics: Option<&Analytics>,
) -> Result<()> {
    let key = session_store::session_key(origin_input);
    let Some(rec) = session_store::load_session(&key)? else {
        match output_format {
            OutputFormat::Json | OutputFormat::Toon => {
                println!(
                    "{}",
                    output_format.serialize(&serde_json::json!({
                        "recovered": false,
                        "message": "no local session for origin; cannot recover",
                    }))?
                );
            }
            OutputFormat::Text => {
                println!("No local session for {origin_input}");
                println!(
                    "Use ' tempo-walletsessions list --orphaned' to view on-chain channels and ' tempo-walletsessions close --orphaned' to close them."
                );
            }
        }
        return Ok(());
    };

    // Query on-chain state for this channel on its recorded network
    let network_id = rec.network_id();

    match query_channel_state(config, &rec.channel_id, network_id).await {
        Ok(Some((_token, _dep, _settled))) => {
            // Read grace period and compute readiness if close has been requested
            let rpc_url = config.rpc_url(network_id);
            let provider =
                alloy::providers::RootProvider::<alloy::network::Ethereum>::new_http(rpc_url);
            let escrow: alloy::primitives::Address = rec
                .escrow_contract
                .parse()
                .context("invalid escrow address in local record")?;

            // We don't have closeRequestedAt from query_channel_state; re-query full channel
            let ch = crate::payment::session::channel::get_channel_on_chain(
                &provider,
                escrow,
                rec.channel_id_b256()?,
            )
            .await
            .ok()
            .flatten();

            if let Some(on_chain) = ch {
                if on_chain.close_requested_at > 0 {
                    let grace = read_grace_period(&provider, escrow).await.unwrap_or(900);
                    let ready_at = on_chain.close_requested_at + grace;
                    let _ = session_store::update_session_close_state_by_channel_id(
                        &rec.channel_id,
                        "closing",
                        on_chain.close_requested_at,
                        ready_at,
                    );
                    if let Some(a) = analytics {
                        a.track(
                            crate::analytics::Event::SessionRecovered,
                            crate::analytics::EmptyPayload,
                        );
                    }
                    match output_format {
                        OutputFormat::Json | OutputFormat::Toon => println!(
                            "{}",
                            output_format.serialize(&serde_json::json!({
                                "recovered": true,
                                "status": if ready_at <= session_store::now_secs() {"finalizable"} else {"closing"},
                                "remaining_secs": ready_at.saturating_sub(session_store::now_secs()),
                            }))?
                        ),
                        OutputFormat::Text => println!(
                            "Recovered state: {} ({}s remaining)",
                            if ready_at <= session_store::now_secs() {"finalizable"} else {"closing"},
                            ready_at.saturating_sub(session_store::now_secs())
                        ),
                    }
                    return Ok(());
                }
            }
            // No pending close; nothing to recover
            match output_format {
                OutputFormat::Json | OutputFormat::Toon => println!(
                    "{}",
                    output_format.serialize(&serde_json::json!({
                        "recovered": false,
                        "message": "no pending close to recover",
                    }))?
                ),
                OutputFormat::Text => println!("No pending close to recover for {}", rec.origin),
            }
        }
        Ok(None) => {
            // Channel settled — clean up local record
            let _ = session_store::delete_session(&key);
            match output_format {
                OutputFormat::Json | OutputFormat::Toon => println!(
                    "{}",
                    output_format.serialize(&serde_json::json!({
                        "recovered": false,
                        "message": "channel already settled — removed local record",
                    }))?
                ),
                OutputFormat::Text => println!(
                    "Channel settled on-chain — removed local record for {}",
                    rec.origin
                ),
            }
        }
        Err(e) => match output_format {
            OutputFormat::Json | OutputFormat::Toon => println!(
                "{}",
                output_format.serialize(&serde_json::json!({
                    "recovered": false,
                    "error": e.to_string(),
                }))?
            ),
            OutputFormat::Text => anyhow::bail!("{e}"),
        },
    }

    Ok(())
}
