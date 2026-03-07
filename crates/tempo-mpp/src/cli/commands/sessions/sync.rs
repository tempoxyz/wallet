use alloy::primitives::Address;
use anyhow::{Context as _, Result};

use super::{session_store, SessionStatus};
use crate::cli::output;
use crate::cli::Context;
use tempo_common::analytics::Event;
use tempo_common::payment::session::channel::{get_channel_on_chain, query_channel_state};

#[derive(serde::Serialize)]
struct SyncSessionsResponse {
    synced: usize,
    removed: usize,
}

#[derive(serde::Serialize)]
struct SyncOriginResponse {
    recovered: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    remaining_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

impl SyncOriginResponse {
    fn recovered(status: &'static str, remaining_secs: u64) -> Self {
        Self {
            recovered: true,
            status: Some(status),
            remaining_secs: Some(remaining_secs),
            message: None,
        }
    }

    fn not_recovered(message: impl Into<String>) -> Self {
        Self {
            recovered: false,
            status: None,
            remaining_secs: None,
            message: Some(message.into()),
        }
    }
}

/// Reconcile local session records with on-chain state.
///
/// Without an origin, removes stale local records for settled channels.
/// With `--origin`, re-syncs close timing for a specific session.
pub(super) async fn sync_sessions(ctx: &Context, origin: Option<&str>) -> Result<()> {
    let output_format = ctx.output_format;
    let show_output = ctx.verbosity.show_output;

    if let Some(origin_input) = origin {
        return sync_origin(ctx, origin_input).await;
    }

    let sessions = session_store::list_sessions()?;

    if sessions.is_empty() {
        output::emit_by_format(
            output_format,
            &SyncSessionsResponse {
                synced: 0,
                removed: 0,
            },
            || {
                println!("No sessions to sync.");
                Ok(())
            },
        )?;
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
    output::emit_by_format(
        output_format,
        &SyncSessionsResponse {
            synced: total,
            removed,
        },
        || {
            if removed > 0 {
                println!("Synced {total} session(s), removed {removed} stale.");
            } else {
                println!("All {total} session(s) are up to date.");
            }
            Ok(())
        },
    )?;

    Ok(())
}

/// Re-sync a single session's close state from on-chain for a given origin.
async fn sync_origin(ctx: &Context, origin_input: &str) -> Result<()> {
    let config = &ctx.config;
    let output_format = ctx.output_format;
    let key = session_store::session_key(origin_input);
    let Some(rec) = session_store::load_session(&key)? else {
        output::emit_by_format(
            output_format,
            &SyncOriginResponse::not_recovered("no local session for origin; cannot recover"),
            || {
                println!("No local session for {origin_input}");
                println!(
                    "Use 'tempo-wallet sessions list --state orphaned' to view on-chain channels and 'tempo-wallet sessions close --orphaned' to close them."
                );
                Ok(())
            },
        )?;
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
            output::emit_by_format(
                output_format,
                &SyncOriginResponse::not_recovered(
                    "channel already settled — removed local record",
                ),
                || {
                    println!(
                        "Channel settled on-chain — removed local record for {}",
                        rec.origin
                    );
                    Ok(())
                },
            )?;
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
        ctx.track_event(Event::SessionRecovered);
        let remaining_secs = ready_at.saturating_sub(session_store::now_secs());
        output::emit_by_format(
            output_format,
            &SyncOriginResponse::recovered(status.as_str(), remaining_secs),
            || {
                println!(
                    "Recovered state: {} ({}s remaining)",
                    status.as_str(),
                    remaining_secs
                );
                Ok(())
            },
        )?;
    } else {
        // No pending close; nothing to recover
        output::emit_by_format(
            output_format,
            &SyncOriginResponse::not_recovered("no pending close to recover"),
            || {
                println!("No pending close to recover for {}", rec.origin);
                Ok(())
            },
        )?;
    }

    Ok(())
}
