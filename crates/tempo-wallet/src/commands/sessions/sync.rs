use alloy::primitives::Address;

use super::{session_store, ChannelStatus};
use tempo_common::{analytics::Event, cli::context::Context, error::TempoError};

const SESSION_RECOVERED: Event = Event::new("session recovered");
use tempo_common::{
    cli::output,
    payment::session::{get_channel_on_chain, query_channel_state},
};

#[derive(serde::Serialize)]
struct SyncOriginResult {
    channel_id: String,
    recovered: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    remaining_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

#[derive(serde::Serialize)]
struct SyncOriginResponse {
    recovered: bool,
    processed: u32,
    recovered_count: u32,
    removed_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    remaining_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    results: Vec<SyncOriginResult>,
}

/// Reconcile local session records with on-chain state.
///
/// Without an origin, removes stale local records for settled channels.
/// With `--origin`, re-syncs close timing for a specific session.
pub(super) async fn sync_sessions(ctx: &Context, origin: Option<&str>) -> Result<(), TempoError> {
    let show_output = ctx.verbosity.show_output;

    if let Some(origin_input) = origin {
        return sync_origin(ctx, origin_input).await;
    }

    let sessions = session_store::list_channels()?;

    if !sessions.is_empty() {
        let mut removed = 0;

        for session in &sessions {
            let network_id = session.network_id();
            let channel_id = session.channel_id_hex();
            let state = query_channel_state(&ctx.config, &channel_id, network_id).await;

            let is_gone = match state {
                Ok(None) => true,
                Ok(Some(_)) => false,
                Err(e) => {
                    if show_output {
                        eprintln!("  Skipping {} ({}): {e}", session.origin, channel_id);
                    }
                    false
                }
            };

            if is_gone {
                if show_output {
                    eprintln!("  Removed stale session: {}", session.origin);
                }
                let _ = session_store::delete_channel(&session.channel_id_hex());
                removed += 1;
            }
        }

        if show_output && removed > 0 {
            eprintln!(
                "Synced {} session(s), removed {} stale.",
                sessions.len(),
                removed
            );
        }
    }

    super::list::list_channels(ctx, vec![]).await
}

/// Re-sync all matching sessions' close state from on-chain for a given origin.
async fn sync_origin(ctx: &Context, origin_input: &str) -> Result<(), TempoError> {
    let config = &ctx.config;
    let output_format = ctx.output_format;
    let origin = normalize_origin(origin_input);
    let records: Vec<_> = session_store::load_channels_by_origin(&origin)?
        .into_iter()
        .filter(|record| record.network_id() == ctx.network)
        .collect();
    if records.is_empty() {
        output::emit_by_format(
            output_format,
            &SyncOriginResponse {
                recovered: false,
                processed: 0,
                recovered_count: 0,
                removed_count: 0,
                status: None,
                remaining_secs: None,
                message: Some("no local session for origin; cannot recover".to_string()),
                results: Vec::new(),
            },
            || {
                eprintln!("No local session for {origin_input}");
                eprintln!(
                    "Use 'tempo wallet sessions list --state orphaned' to view on-chain channels and 'tempo wallet sessions close --orphaned' to close them."
                );
                Ok(())
            },
        )?;
        return Ok(());
    }

    let mut recovered_count = 0u32;
    let mut removed_count = 0u32;
    let mut results = Vec::new();

    for rec in records {
        // Query on-chain state for each matching channel on its recorded network
        let network_id = rec.network_id();
        let provider = super::util::make_provider(config, network_id);
        let escrow: Address = rec.escrow_contract;

        let on_chain = match get_channel_on_chain(&provider, escrow, rec.channel_id).await {
            Ok(Some(ch)) => ch,
            Ok(None) => {
                // Channel settled — clean up local record
                let _ = session_store::delete_channel(&rec.channel_id_hex());
                removed_count = removed_count.saturating_add(1);
                results.push(SyncOriginResult {
                    channel_id: rec.channel_id_hex(),
                    recovered: false,
                    status: None,
                    remaining_secs: None,
                    message: Some("channel already settled — removed local record".to_string()),
                });
                continue;
            }
            Err(e) => return Err(e),
        };

        if on_chain.close_requested_at > 0 {
            let grace = super::util::resolve_grace_period(config, network_id, escrow).await;
            let ready_at = on_chain.close_requested_at + grace;
            let status = if ready_at <= session_store::now_secs() {
                ChannelStatus::Finalizable
            } else {
                ChannelStatus::Closing
            };
            let _ = session_store::update_channel_close_state(
                &rec.channel_id_hex(),
                status,
                on_chain.close_requested_at,
                ready_at,
            );
            ctx.track_event(SESSION_RECOVERED);
            let remaining_secs = ready_at.saturating_sub(session_store::now_secs());
            recovered_count = recovered_count.saturating_add(1);
            results.push(SyncOriginResult {
                channel_id: rec.channel_id_hex(),
                recovered: true,
                status: Some(status.as_str()),
                remaining_secs: Some(remaining_secs),
                message: None,
            });
        } else {
            results.push(SyncOriginResult {
                channel_id: rec.channel_id_hex(),
                recovered: false,
                status: None,
                remaining_secs: None,
                message: Some("no pending close to recover".to_string()),
            });
        }
    }

    let processed = results.len() as u32;
    let response = if results.len() == 1 {
        let single = &results[0];
        SyncOriginResponse {
            recovered: single.recovered,
            processed,
            recovered_count,
            removed_count,
            status: single.status,
            remaining_secs: single.remaining_secs,
            message: single.message.clone(),
            results,
        }
    } else {
        SyncOriginResponse {
            recovered: recovered_count > 0,
            processed,
            recovered_count,
            removed_count,
            status: None,
            remaining_secs: None,
            message: None,
            results,
        }
    };

    output::emit_by_format(output_format, &response, || {
        if response.processed == 1 {
            if response.recovered {
                eprintln!(
                    "Recovered state: {} ({}s remaining)",
                    response.status.unwrap_or("unknown"),
                    response.remaining_secs.unwrap_or(0)
                );
            } else {
                eprintln!("No pending close to recover for {origin}");
            }
        } else {
            eprintln!(
                "Processed {} channel(s) for {}: recovered {}, removed {}",
                response.processed, origin, response.recovered_count, response.removed_count
            );
        }
        Ok(())
    })?;

    Ok(())
}

fn normalize_origin(target: &str) -> String {
    url::Url::parse(target)
        .map_or_else(|_| target.to_string(), |u| u.origin().ascii_serialization())
}
