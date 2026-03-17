use alloy::primitives::Address;

use super::session;
use tempo_common::{
    analytics::Event,
    cli::context::Context,
    error::TempoError,
    payment::session::{get_channel_on_chain, query_channel_state},
};

const SESSION_RECOVERED: Event = Event::new("session recovered");

struct SyncSummary {
    processed: u32,
    recovered: u32,
    removed: u32,
}

/// Reconcile local session records with on-chain state.
///
/// Without an origin, removes stale local records for settled channels.
/// With `--origin`, re-syncs close timing for matching local sessions.
///
/// Output contract is aligned with `sessions list`: after reconciliation this
/// command always renders the current local session list.
pub(super) async fn sync_sessions(ctx: &Context, origin: Option<&str>) -> Result<(), TempoError> {
    let summary = if let Some(origin_input) = origin {
        sync_origin(ctx, origin_input).await?
    } else {
        sync_global(ctx).await?
    };

    if ctx.verbosity.show_output && !ctx.output_format.is_structured() {
        eprintln!(
            "Sync complete: processed {}, recovered {}, removed {}",
            summary.processed, summary.recovered, summary.removed
        );
    }

    super::list::list_channels(ctx, false, false).await
}

async fn sync_global(ctx: &Context) -> Result<SyncSummary, TempoError> {
    let sessions = session::list_channels()?;

    let mut processed = 0u32;
    let mut removed = 0u32;

    for session in &sessions {
        if session.network_id() != ctx.network {
            continue;
        }

        processed = processed.saturating_add(1);
        let channel_id = session.channel_id_hex();
        let state = query_channel_state(&ctx.config, &channel_id, session.network_id()).await;

        let is_gone = match state {
            Ok(None) => true,
            Ok(Some(_)) => false,
            Err(e) => {
                if ctx.verbosity.show_output {
                    eprintln!("  Skipping {} ({}): {e}", session.origin, channel_id);
                }
                false
            }
        };

        if is_gone {
            if ctx.verbosity.show_output {
                eprintln!("  Removed stale session: {}", session.origin);
            }
            let _ = session::delete_channel(&channel_id);
            removed = removed.saturating_add(1);
        }
    }

    Ok(SyncSummary {
        processed,
        recovered: 0,
        removed,
    })
}

async fn sync_origin(ctx: &Context, origin_input: &str) -> Result<SyncSummary, TempoError> {
    let origin = super::util::normalize_origin(origin_input);
    let records: Vec<_> = session::load_channels_by_origin(&origin)?
        .into_iter()
        .filter(|record| record.network_id() == ctx.network)
        .collect();
    let processed = records.len() as u32;

    if records.is_empty() {
        if ctx.verbosity.show_output && !ctx.output_format.is_structured() {
            eprintln!("No local session for {origin_input}");
            eprintln!(
                "Use 'tempo wallet sessions list --orphaned' to discover and persist orphaned channels."
            );
        }
        return Ok(SyncSummary {
            processed: 0,
            recovered: 0,
            removed: 0,
        });
    }

    let mut recovered = 0u32;
    let mut removed = 0u32;

    for rec in records {
        let provider = super::util::make_provider(&ctx.config, rec.network_id());
        let escrow: Address = rec.escrow_contract;

        let on_chain = match get_channel_on_chain(&provider, escrow, rec.channel_id).await {
            Ok(Some(ch)) => ch,
            Ok(None) => {
                let _ = session::delete_channel(&rec.channel_id_hex());
                removed = removed.saturating_add(1);
                continue;
            }
            Err(e) => return Err(e),
        };

        if on_chain.close_requested_at > 0 {
            let grace =
                super::util::resolve_grace_period(&ctx.config, rec.network_id(), escrow).await;
            let now = session::now_secs();
            let ready_at = super::util::grace_ready_at(on_chain.close_requested_at, grace);
            let status =
                super::util::status_from_close_timing(on_chain.close_requested_at, grace, now);
            let _ = session::update_channel_close_state(
                &rec.channel_id_hex(),
                status,
                on_chain.close_requested_at,
                ready_at,
            );
            recovered = recovered.saturating_add(1);
            ctx.track_event(SESSION_RECOVERED);
        }
    }

    Ok(SyncSummary {
        processed,
        recovered,
        removed,
    })
}
