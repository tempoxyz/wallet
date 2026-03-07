use anyhow::{Context as _, Result};

use crate::analytics::Event;
use crate::cli::OutputFormat;
use crate::payment::session::channel::{get_channel_on_chain, query_channel_state, read_grace_period};
use crate::payment::session::store as session_store;
use crate::payment::session::store::SessionStatus;
use crate::payment::session::DEFAULT_GRACE_PERIOD_SECS;

/// Reconcile local session records with on-chain state.
///
/// Without an origin, removes stale local records for settled channels.
/// With `--origin`, re-syncs close timing for a specific session.
pub(super) async fn sync_sessions(
    ctx: &crate::cli::Context,
    origin: Option<&str>,
) -> Result<()> {
    let config = &ctx.config;
    let output_format = ctx.output_format;
    let show_output = ctx.cli.verbosity().show_output;
    let analytics = ctx.analytics.as_ref();

    if let Some(origin_input) = origin {
        return recover_origin(config, output_format, origin_input, analytics).await;
    }

    let sessions = session_store::list_sessions()?;

    if sessions.is_empty() {
        if output_format.is_structured() {
            println!(
                "{}",
                output_format.serialize(&serde_json::json!({
                    "synced": 0,
                    "removed": 0,
                }))?
            );
        } else {
            println!("No sessions to sync.");
        }
        return Ok(());
    }

    let mut removed = 0;

    for session in &sessions {
        let network_id = session.network_id();
        let state = query_channel_state(config, &session.channel_id, network_id).await;

        let is_gone = match state {
            Ok(None) => true,     // Channel settled or doesn't exist
            Ok(Some(_)) => false, // Channel still open
            Err(e) => {
                // RPC error — skip, don't delete (may be transient)
                if show_output {
                    eprintln!(
                        "  Skipping {} ({}): {e}",
                        session.origin, session.channel_id
                    );
                }
                false
            }
        };

        if is_gone {
            if show_output {
                eprintln!("  Removed stale session: {}", session.origin);
            }
            let key = session_store::session_key(&session.origin);
            let _ = session_store::delete_session(&key);
            removed += 1;
        }
    }

    let total = sessions.len();
    if output_format.is_structured() {
        println!(
            "{}",
            output_format.serialize(&serde_json::json!({
                "synced": total,
                "removed": removed,
            }))?
        );
    } else if removed > 0 {
        println!("Synced {total} session(s), removed {removed} stale.");
    } else {
        println!("All {total} session(s) are up to date.");
    }

    Ok(())
}

/// Re-sync a single session's close state from on-chain for a given origin.
async fn recover_origin(
    config: &crate::config::Config,
    output_format: OutputFormat,
    origin_input: &str,
    analytics: Option<&crate::analytics::Analytics>,
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
                    "Use 'tempo-wallet sessions list --state orphaned' to view on-chain channels and 'tempo-wallet sessions close --orphaned' to close them."
                );
            }
        }
        return Ok(());
    };

    // Query on-chain state for this channel on its recorded network
    let network_id = rec.network_id();
    let rpc_url = config.rpc_url(network_id);
    let provider = alloy::providers::RootProvider::<alloy::network::Ethereum>::new_http(rpc_url);
    let escrow: alloy::primitives::Address = rec
        .escrow_contract
        .parse()
        .context("invalid escrow address in local record")?;

    let on_chain = match get_channel_on_chain(&provider, escrow, rec.channel_id_b256()?).await {
        Ok(Some(ch)) => ch,
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
            return Ok(());
        }
        Err(e) => {
            match output_format {
                OutputFormat::Json | OutputFormat::Toon => println!(
                    "{}",
                    output_format.serialize(&serde_json::json!({
                        "recovered": false,
                        "error": e.to_string(),
                    }))?
                ),
                OutputFormat::Text => return Err(e),
            }
            return Ok(());
        }
    };

    if on_chain.close_requested_at > 0 {
        let grace = read_grace_period(&provider, escrow)
            .await
            .unwrap_or(DEFAULT_GRACE_PERIOD_SECS);
        let ready_at = on_chain.close_requested_at + grace;
        let status = if ready_at <= session_store::now_secs() {
            SessionStatus::Finalizable
        } else {
            SessionStatus::Closing
        };
        let _ = session_store::update_session_close_state_by_channel_id(
            &rec.channel_id,
            status,
            on_chain.close_requested_at,
            ready_at,
        );
        if let Some(a) = analytics {
            a.track_event(Event::SessionRecovered);
        }
        match output_format {
            OutputFormat::Json | OutputFormat::Toon => println!(
                "{}",
                output_format.serialize(&serde_json::json!({
                    "recovered": true,
                    "status": status.as_str(),
                    "remaining_secs": ready_at.saturating_sub(session_store::now_secs()),
                }))?
            ),
            OutputFormat::Text => println!(
                "Recovered state: {} ({}s remaining)",
                status.as_str(),
                ready_at.saturating_sub(session_store::now_secs())
            ),
        }
    } else {
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

    Ok(())
}
