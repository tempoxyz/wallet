use std::collections::HashSet;

use anyhow::Result;

use super::{session_store, SessionStatus};
use crate::cli::Context;
use crate::cli::OutputFormat;
use crate::error::TempoWalletError;
use crate::payment::session::channel::find_all_channels_for_payer;
use crate::payment::session::close::{
    close_channel_by_id, close_discovered_channel, close_session_from_record,
};
use crate::payment::session::CloseOutcome;
use crate::util::format_duration;

/// Close a session by URL or close all sessions.
///
/// When `--all` is used, this first closes local sessions, then scans on-chain
/// for any orphaned channels belonging to the current wallet and closes those too.
pub(super) async fn close_sessions(
    ctx: &Context,
    url: Option<String>,
    all: bool,
    orphaned: bool,
    finalize: bool,
) -> Result<()> {
    if finalize {
        return finalize_closed_channels(ctx).await;
    }
    if orphaned {
        return close_orphaned_channels(ctx).await;
    }
    if all {
        return close_all_sessions(ctx).await;
    }

    if let Some(ref target) = url {
        // If the target looks like a channel ID (0x-prefixed hex), close on-chain directly
        if super::is_channel_id(target) {
            return close_by_channel_id(ctx, target).await;
        }

        // Otherwise treat as a URL — close the local session
        return close_by_url(ctx, target).await;
    }

    anyhow::bail!(TempoWalletError::InvalidUrl(
        "Specify a URL, channel ID (0x...), or use --all/--orphaned/--finalize to close sessions"
            .to_string()
    ));
}

/// Close all local sessions and on-chain orphaned channels.
async fn close_all_sessions(ctx: &Context) -> Result<()> {
    let show_output = ctx.cli.verbosity().show_output;
    let analytics = ctx.analytics.as_ref();
    let mut summary = CloseSummary::new();

    // Phase 1: close local sessions (scoped to current network)
    let all_sessions = session_store::list_sessions()?;
    let net = ctx.network.as_str();
    let sessions: Vec<_> = all_sessions
        .iter()
        .filter(|s| s.network_name == net)
        .collect();
    for session in &sessions {
        let key = session_store::session_key(&session.origin);
        let result = close_session_from_record(session, &ctx.config, analytics, &ctx.keys).await;
        if matches!(result, Ok(CloseOutcome::Closed { .. })) {
            if let Err(e) = session_store::delete_session(&key) {
                if show_output {
                    eprintln!("  Failed to remove local session: {e}");
                }
            }
        }
        summary.record_outcome(
            result,
            &session.origin,
            &session.channel_id,
            show_output,
            serde_json::json!({"origin": session.origin}),
        );
    }

    // Phase 2: scan on-chain for orphaned channels
    close_orphaned_into_summary(ctx, &all_sessions, &mut summary).await;

    summary.print(ctx.output_format, "No active sessions to close.", "closed")?;
    Ok(())
}

/// Close a single channel by its on-chain ID (0x...).
async fn close_by_channel_id(ctx: &Context, target: &str) -> Result<()> {
    let mut summary = CloseSummary::new();
    let result = close_channel_by_id(&ctx.config, target, ctx.network, None, &ctx.keys).await;
    summary.record_finalize_outcome(result, target, true);
    summary.print(ctx.output_format, "No channel to close.", "closed")
}

/// Close a session by URL (local session lookup).
async fn close_by_url(ctx: &Context, target: &str) -> Result<()> {
    let show_output = ctx.cli.verbosity().show_output;
    let output_format = ctx.output_format;
    let analytics = ctx.analytics.as_ref();

    let key = session_store::session_key(target);
    let session = session_store::load_session(&key)?;
    let mut summary = CloseSummary::new();

    if let Some(record) = session {
        let result = close_session_from_record(&record, &ctx.config, analytics, &ctx.keys).await;
        if matches!(result, Ok(CloseOutcome::Closed { .. })) {
            if let Err(e) = session_store::delete_session(&key) {
                if show_output {
                    eprintln!("  Failed to remove local session: {e}");
                }
            }
        }
        summary.record_outcome(
            result,
            &record.origin,
            &record.channel_id,
            show_output,
            serde_json::json!({"origin": record.origin}),
        );
    } else if output_format.is_structured() {
        summary.record_failed(serde_json::json!({
            "origin": target,
            "status": "error",
            "error": "no active session",
        }));
    } else {
        println!("No active session for {target}");
        return Ok(());
    }

    summary.print(output_format, "No active session.", "closed")
}

/// Scan on-chain for orphaned channels and close them into the given summary.
///
/// Shared by `close_all_sessions` (Phase 2) and `close_orphaned_channels`.
async fn close_orphaned_into_summary(
    ctx: &Context,
    local_sessions: &[session_store::SessionRecord],
    summary: &mut CloseSummary,
) {
    let show_output = ctx.cli.verbosity().show_output;

    let Some(wallet_addr) = ctx.keys.wallet_address_parsed() else {
        return;
    };

    let local_ids: HashSet<String> = local_sessions
        .iter()
        .map(|s| s.channel_id.to_lowercase())
        .collect();

    let channels = find_all_channels_for_payer(&ctx.config, wallet_addr, ctx.network).await;
    let orphaned: Vec<_> = channels
        .iter()
        .filter(|ch| !local_ids.contains(&ch.channel_id.to_lowercase()))
        .collect();

    if show_output && !orphaned.is_empty() {
        eprintln!("Found {} orphaned channel(s)", orphaned.len());
    }

    for ch in &orphaned {
        let result = close_discovered_channel(ch, &ctx.config, &ctx.keys).await;
        if matches!(result, Ok(CloseOutcome::Closed { .. })) {
            let _ = session_store::delete_session_by_channel_id(&ch.channel_id);
        }
        summary.record_outcome(
            result,
            &ch.channel_id,
            &ch.channel_id,
            show_output,
            serde_json::json!({}),
        );
    }
}

/// Close only orphaned on-chain channels (channels with no local session record).
async fn close_orphaned_channels(ctx: &Context) -> Result<()> {
    if !ctx.keys.has_wallet() {
        anyhow::bail!(TempoWalletError::ConfigMissing(
            "No wallet configured. Log in with 'tempo-wallet login'.".to_string()
        ));
    }

    let local_sessions = session_store::list_sessions()?;
    let mut summary = CloseSummary::new();

    close_orphaned_into_summary(ctx, &local_sessions, &mut summary).await;

    summary.print(ctx.output_format, "No orphaned channels found.", "closed")?;
    Ok(())
}

/// Finalize channels that have had requestClose() submitted and whose grace period has elapsed.
async fn finalize_closed_channels(ctx: &Context) -> Result<()> {
    let show_output = ctx.cli.verbosity().show_output;
    let now = session_store::now_secs();
    let mut summary = CloseSummary::new();

    // Load wallet signer once (all channels share the same network)
    let wallet = match ctx.keys.signer(ctx.network) {
        Ok(w) => Some(w),
        Err(e) => {
            if show_output {
                eprintln!("Failed to load wallet for {}", ctx.network);
                eprintln!("  {e}");
            }
            None
        }
    };

    // 1) Local sessions ready to finalize
    for s in session_store::list_sessions()? {
        if s.network_name != ctx.network.as_str() {
            continue;
        }
        if !(s.state == SessionStatus::Closing && now >= s.grace_ready_at) {
            continue;
        }
        let Some(ref wallet) = wallet else {
            summary.record_failed(serde_json::json!({
                "channel_id": s.channel_id,
                "status": "error",
                "error": "no wallet available",
            }));
            continue;
        };
        let result = close_channel_by_id(
            &ctx.config,
            &s.channel_id,
            ctx.network,
            Some(wallet),
            &ctx.keys,
        )
        .await;
        summary.record_finalize_outcome(result, &s.channel_id, show_output);
    }

    // 2) Orphaned channels ready to finalize
    if let Some(wallet_addr) = ctx.keys.wallet_address_parsed() {
        let channels = find_all_channels_for_payer(&ctx.config, wallet_addr, ctx.network).await;
        for ch in &channels {
            if ch.close_requested_at == 0 {
                continue;
            }
            let Some(ref wallet) = wallet else {
                summary.record_failed(serde_json::json!({
                    "channel_id": ch.channel_id,
                    "status": "error",
                    "error": "no wallet available",
                }));
                continue;
            };
            // Check grace readiness from on-chain constant
            let grace =
                super::resolve_grace_period(&ctx.config, ctx.network, &ch.escrow_contract).await;
            let ready_at = ch.close_requested_at + grace;
            if now < ready_at {
                continue;
            }
            let result = close_channel_by_id(
                &ctx.config,
                &ch.channel_id,
                ctx.network,
                Some(wallet),
                &ctx.keys,
            )
            .await;
            summary.record_finalize_outcome(result, &ch.channel_id, show_output);
        }
    }

    summary.print(
        ctx.output_format,
        "No channels pending finalization.",
        "finalized",
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// CloseSummary — batch close result tracking and output
// ---------------------------------------------------------------------------

/// Tracks the result of batch close operations for consistent output.
struct CloseSummary {
    closed: u32,
    pending: u32,
    failed: u32,
    results: Vec<serde_json::Value>,
}

impl CloseSummary {
    fn new() -> Self {
        Self {
            closed: 0,
            pending: 0,
            failed: 0,
            results: Vec::new(),
        }
    }

    /// Record a `CloseOutcome`, logging progress to stderr.
    ///
    /// `label` is the display name for the channel (origin URL or channel ID).
    /// `extra_json` is merged into the JSON result object (e.g. `"origin"` field).
    fn record_outcome(
        &mut self,
        result: Result<CloseOutcome>,
        label: &str,
        channel_id: &str,
        show_output: bool,
        extra_json: serde_json::Value,
    ) {
        match result {
            Ok(CloseOutcome::Closed {
                tx_url,
                amount_display,
            }) => {
                if show_output {
                    eprintln!("Closed {label}");
                    if let Some(url) = &tx_url {
                        if let Some(ref amt) = amount_display {
                            eprintln!("Paid {amt} · {url}");
                        } else {
                            eprintln!("Paid settlement · {url}");
                        }
                    }
                }
                let mut json = extra_json;
                json["channel_id"] = serde_json::json!(channel_id);
                json["status"] = serde_json::json!("closed");
                self.record_closed(json);
            }
            Ok(CloseOutcome::Pending { remaining_secs }) => {
                if show_output {
                    eprintln!(
                        "Pending {label} — {} remaining",
                        format_duration(remaining_secs)
                    );
                }
                let mut json = extra_json;
                json["channel_id"] = serde_json::json!(channel_id);
                json["status"] = serde_json::json!("pending");
                json["remaining_secs"] = serde_json::json!(remaining_secs);
                self.record_pending(json);
            }
            Err(e) => {
                if show_output {
                    eprintln!("Failed to close {label}");
                    eprintln!("  {e:#}");
                }
                let mut json = extra_json;
                json["channel_id"] = serde_json::json!(channel_id);
                json["status"] = serde_json::json!("error");
                json["error"] = serde_json::json!(format!("{e:#}"));
                self.record_failed(json);
            }
        }
    }

    /// Record a finalize outcome, treating `ChannelNotFound` as a successful close.
    fn record_finalize_outcome(
        &mut self,
        result: Result<CloseOutcome>,
        channel_id: &str,
        show_output: bool,
    ) {
        match result {
            Err(e)
                if e.downcast_ref::<TempoWalletError>()
                    .is_some_and(|te| matches!(te, TempoWalletError::ChannelNotFound { .. })) =>
            {
                let _ = session_store::delete_session_by_channel_id(channel_id);
                if show_output {
                    eprintln!("Finalized {channel_id} (already settled)");
                }
                self.record_closed(serde_json::json!({
                    "channel_id": channel_id,
                    "status": "closed",
                }));
            }
            other => {
                if matches!(other, Ok(CloseOutcome::Closed { .. })) {
                    let _ = session_store::delete_session_by_channel_id(channel_id);
                }
                self.record_outcome(
                    other,
                    channel_id,
                    channel_id,
                    show_output,
                    serde_json::json!({}),
                );
            }
        }
    }

    fn record_closed(&mut self, result: serde_json::Value) {
        self.closed += 1;
        self.results.push(result);
    }

    fn record_pending(&mut self, result: serde_json::Value) {
        self.pending += 1;
        self.results.push(result);
    }

    fn record_failed(&mut self, result: serde_json::Value) {
        self.failed += 1;
        self.results.push(result);
    }

    fn print(
        &self,
        output_format: OutputFormat,
        empty_msg: &str,
        closed_label: &str,
    ) -> Result<()> {
        if output_format.is_structured() {
            println!(
                "{}",
                output_format.serialize(&serde_json::json!({
                    "closed": self.closed,
                    "pending": self.pending,
                    "failed": self.failed,
                    "results": self.results
                }))?
            );
        } else {
            let total = self.closed + self.pending + self.failed;
            if total == 0 {
                println!("{empty_msg}");
            } else {
                let mut parts = Vec::new();
                if self.closed > 0 {
                    parts.push(format!("{} {closed_label}", self.closed));
                }
                if self.pending > 0 {
                    parts.push(format!("{} pending", self.pending));
                }
                if self.failed > 0 {
                    parts.push(format!("{} failed", self.failed));
                }
                println!("{}", parts.join(", "));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::OutputFormat;

    #[test]
    fn test_close_summary_empty_text() {
        let summary = CloseSummary::new();
        summary
            .print(OutputFormat::Text, "No sessions to close.", "closed")
            .unwrap();
    }

    #[test]
    fn test_close_summary_empty_json() {
        let summary = CloseSummary::new();
        summary
            .print(OutputFormat::Json, "No sessions to close.", "closed")
            .unwrap();
    }

    #[test]
    fn test_close_summary_counts() {
        let mut summary = CloseSummary::new();
        summary.record_closed(serde_json::json!({"channel_id": "0x1", "status": "closed"}));
        summary.record_closed(serde_json::json!({"channel_id": "0x2", "status": "closed"}));
        summary.record_pending(serde_json::json!({"channel_id": "0x3", "status": "pending"}));
        summary.record_failed(serde_json::json!({"channel_id": "0x4", "status": "error"}));

        assert_eq!(summary.closed, 2);
        assert_eq!(summary.pending, 1);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.results.len(), 4);
    }

    #[test]
    fn test_close_summary_json_output_no_panic() {
        let mut summary = CloseSummary::new();
        summary.record_closed(serde_json::json!({"channel_id": "0x1", "status": "closed"}));
        summary.record_pending(
            serde_json::json!({"channel_id": "0x2", "status": "pending", "remaining_secs": 60}),
        );
        summary.record_failed(
            serde_json::json!({"channel_id": "0x3", "status": "error", "error": "timeout"}),
        );
        summary
            .print(OutputFormat::Json, "No sessions.", "closed")
            .unwrap();
    }

    #[test]
    fn test_close_summary_text_output_no_panic() {
        let mut summary = CloseSummary::new();
        summary.record_closed(serde_json::json!({"status": "closed"}));
        summary
            .print(OutputFormat::Text, "No sessions.", "closed")
            .unwrap();
    }
}
