use alloy::primitives::Address;
use anyhow::{Context as _, Result};

use super::{session_store, SessionStatus};
use crate::analytics::Event;
use crate::cli::Context;
use crate::payment::session::channel::{get_channel_on_chain, query_channel_state};

/// Reconcile local session records with on-chain state.
///
/// Without an origin, removes stale local records for settled channels.
/// With `--origin`, re-syncs close timing for a specific session.
pub(super) async fn sync_sessions(ctx: &Context, origin: Option<&str>) -> Result<()> {
    let output_format = ctx.output_format;
    let show_output = ctx.cli.verbosity().show_output;

    if let Some(origin_input) = origin {
        return sync_origin(ctx, origin_input).await;
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
        let state = query_channel_state(&ctx.config, &session.channel_id, network_id).await;

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
async fn sync_origin(ctx: &Context, origin_input: &str) -> Result<()> {
    let config = &ctx.config;
    let output_format = ctx.output_format;
    let key = session_store::session_key(origin_input);
    let Some(rec) = session_store::load_session(&key)? else {
        if output_format.is_structured() {
            println!(
                "{}",
                output_format.serialize(&serde_json::json!({
                    "recovered": false,
                    "message": "no local session for origin; cannot recover",
                }))?
            );
        } else {
            println!("No local session for {origin_input}");
            println!(
                "Use 'tempo-wallet sessions list --state orphaned' to view on-chain channels and 'tempo-wallet sessions close --orphaned' to close them."
            );
        }
        return Ok(());
    };

    // Query on-chain state for this channel on its recorded network
    let network_id = rec.network_id();
    let provider = super::make_provider(config, network_id);
    let escrow: Address = rec
        .escrow_contract
        .parse()
        .context("invalid escrow address in local record")?;

    let on_chain = match get_channel_on_chain(&provider, escrow, rec.channel_id_b256()?).await {
        Ok(Some(ch)) => ch,
        Ok(None) => {
            // Channel settled — clean up local record
            let _ = session_store::delete_session(&key);
            if output_format.is_structured() {
                println!(
                    "{}",
                    output_format.serialize(&serde_json::json!({
                        "recovered": false,
                        "message": "channel already settled — removed local record",
                    }))?
                );
            } else {
                println!(
                    "Channel settled on-chain — removed local record for {}",
                    rec.origin
                );
            }
            return Ok(());
        }
        Err(e) => return Err(e),
    };

    if on_chain.close_requested_at > 0 {
        let grace = super::resolve_grace_period(config, network_id, &rec.escrow_contract).await;
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
        if let Some(a) = ctx.analytics.as_ref() {
            a.track_event(Event::SessionRecovered);
        }
        if output_format.is_structured() {
            println!(
                "{}",
                output_format.serialize(&serde_json::json!({
                    "recovered": true,
                    "status": status.as_str(),
                    "remaining_secs": ready_at.saturating_sub(session_store::now_secs()),
                }))?
            );
        } else {
            println!(
                "Recovered state: {} ({}s remaining)",
                status.as_str(),
                ready_at.saturating_sub(session_store::now_secs())
            );
        }
    } else {
        // No pending close; nothing to recover
        if output_format.is_structured() {
            println!(
                "{}",
                output_format.serialize(&serde_json::json!({
                    "recovered": false,
                    "message": "no pending close to recover",
                }))?
            );
        } else {
            println!("No pending close to recover for {}", rec.origin);
        }
    }

    Ok(())
}
