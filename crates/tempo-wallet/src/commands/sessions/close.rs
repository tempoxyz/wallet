use std::collections::HashSet;

use anyhow::Result;

use super::{session_store, SessionStatus};
use tempo_common::cli::context::Context;
use tempo_common::cli::format::format_duration;
use tempo_common::cli::output;
use tempo_common::cli::output::OutputFormat;
use tempo_common::error::{ConfigError, InputError, PaymentError, TempoError};
use tempo_common::payment::session::channel::find_all_channels_for_payer;
use tempo_common::payment::session::close::{
    close_channel_by_id, close_discovered_channel, close_session_from_record,
};
use tempo_common::payment::session::CloseOutcome;

#[derive(serde::Serialize)]
struct CloseSummaryResponse {
    closed: u32,
    pending: u32,
    failed: u32,
    results: Vec<CloseResult>,
}

#[derive(Clone, serde::Serialize)]
struct CloseResult {
    channel_id: String,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    origin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    remaining_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl CloseResult {
    fn closed(channel_id: &str, origin: Option<&str>) -> Self {
        Self {
            channel_id: channel_id.to_string(),
            status: "closed",
            origin: origin.map(ToString::to_string),
            remaining_secs: None,
            error: None,
        }
    }

    fn pending(channel_id: &str, origin: Option<&str>, remaining_secs: u64) -> Self {
        Self {
            channel_id: channel_id.to_string(),
            status: "pending",
            origin: origin.map(ToString::to_string),
            remaining_secs: Some(remaining_secs),
            error: None,
        }
    }

    fn failed(channel_id: &str, origin: Option<&str>, error: impl Into<String>) -> Self {
        Self {
            channel_id: channel_id.to_string(),
            status: "error",
            origin: origin.map(ToString::to_string),
            remaining_secs: None,
            error: Some(error.into()),
        }
    }
}

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
    dry_run: bool,
) -> Result<()> {
    if dry_run {
        return dry_run_close(ctx, url.as_deref(), all, orphaned, finalize).await;
    }
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
        if target.starts_with("0x") {
            super::validate_channel_id(target)?;
            return close_by_channel_id(ctx, target).await;
        }
        return close_by_url(ctx, target).await;
    }

    anyhow::bail!(InputError::InvalidUrl(
        "Specify a URL, channel ID (0x...), or use --all/--orphaned/--finalize to close sessions"
            .to_string()
    ));
}

async fn dry_run_close(
    ctx: &Context,
    url: Option<&str>,
    all: bool,
    orphaned: bool,
    finalize: bool,
) -> Result<()> {
    let net = ctx.network.as_str();

    #[derive(serde::Serialize)]
    struct DryRunResponse {
        mode: &'static str,
        targets: Vec<DryRunTarget>,
    }

    #[derive(serde::Serialize)]
    struct DryRunTarget {
        channel_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        origin: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        state: Option<String>,
    }

    let mut targets = Vec::new();

    let mode = if finalize {
        "finalize"
    } else if orphaned {
        "orphaned"
    } else if all {
        "all"
    } else {
        "single"
    };

    if all || (!orphaned && !finalize && url.is_none()) {
        let sessions = session_store::list_sessions()?;
        for s in &sessions {
            if s.network_name == net {
                targets.push(DryRunTarget {
                    channel_id: s.channel_id.clone(),
                    origin: Some(s.origin.clone()),
                    state: Some(format!("{:?}", s.state)),
                });
            }
        }
    }

    if let Some(target) = url {
        if super::is_channel_id(target) {
            targets.push(DryRunTarget {
                channel_id: target.to_string(),
                origin: None,
                state: None,
            });
        } else {
            let key = session_store::session_key(target);
            if let Some(rec) = session_store::load_session(&key)? {
                targets.push(DryRunTarget {
                    channel_id: rec.channel_id.clone(),
                    origin: Some(rec.origin.clone()),
                    state: Some(format!("{:?}", rec.state)),
                });
            } else {
                targets.push(DryRunTarget {
                    channel_id: String::new(),
                    origin: Some(target.to_string()),
                    state: Some("not found".to_string()),
                });
            }
        }
    }

    let response = DryRunResponse { mode, targets };

    output::emit_by_format(ctx.output_format, &response, || {
        eprintln!(
            "[DRY RUN] Would close {} session(s) (mode: {mode})",
            response.targets.len()
        );
        for t in &response.targets {
            if let Some(ref origin) = t.origin {
                eprintln!("  {} ({})", origin, t.channel_id);
            } else {
                eprintln!("  {}", t.channel_id);
            }
        }
        Ok(())
    })
}

/// Close all local sessions and on-chain orphaned channels.
async fn close_all_sessions(ctx: &Context) -> Result<()> {
    let show_output = ctx.verbosity.show_output;
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
            Some(&session.origin),
            &session.origin,
            &session.channel_id,
            show_output,
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
    let show_output = ctx.verbosity.show_output;
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
            Some(&record.origin),
            &record.origin,
            &record.channel_id,
            show_output,
        );
    } else {
        let emitted_text = output::run_text_only(output_format, || {
            println!("No active session for {target}");
            Ok(())
        })?;
        if emitted_text {
            return Ok(());
        }
        summary.record_failed(CloseResult::failed(
            target,
            Some(target),
            "no active session",
        ));
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
    let show_output = ctx.verbosity.show_output;

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
        summary.record_outcome(result, None, &ch.channel_id, &ch.channel_id, show_output);
    }
}

/// Close only orphaned on-chain channels (channels with no local session record).
async fn close_orphaned_channels(ctx: &Context) -> Result<()> {
    if !ctx.keys.has_wallet() {
        anyhow::bail!(ConfigError::Missing(
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
    let show_output = ctx.verbosity.show_output;
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
            summary.record_failed(CloseResult::failed(
                &s.channel_id,
                None,
                "no wallet available",
            ));
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
                summary.record_failed(CloseResult::failed(
                    &ch.channel_id,
                    None,
                    "no wallet available",
                ));
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
    results: Vec<CloseResult>,
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
    fn record_outcome(
        &mut self,
        result: Result<CloseOutcome>,
        origin: Option<&str>,
        label: &str,
        channel_id: &str,
        show_output: bool,
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
                self.record_closed(CloseResult::closed(channel_id, origin));
            }
            Ok(CloseOutcome::Pending { remaining_secs }) => {
                if show_output {
                    eprintln!(
                        "Pending {label} — {} remaining",
                        format_duration(remaining_secs)
                    );
                }
                self.record_pending(CloseResult::pending(channel_id, origin, remaining_secs));
            }
            Err(e) => {
                if show_output {
                    eprintln!("Failed to close {label}");
                    eprintln!("  {e:#}");
                }
                self.record_failed(CloseResult::failed(channel_id, origin, format!("{e:#}")));
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
                if e.downcast_ref::<TempoError>().is_some_and(|te| {
                    matches!(
                        te,
                        TempoError::Payment(PaymentError::ChannelNotFound { .. })
                    )
                }) =>
            {
                let _ = session_store::delete_session_by_channel_id(channel_id);
                if show_output {
                    eprintln!("Finalized {channel_id} (already settled)");
                }
                self.record_closed(CloseResult::closed(channel_id, None));
            }
            other => {
                if matches!(other, Ok(CloseOutcome::Closed { .. })) {
                    let _ = session_store::delete_session_by_channel_id(channel_id);
                }
                self.record_outcome(other, None, channel_id, channel_id, show_output);
            }
        }
    }

    fn record_closed(&mut self, result: CloseResult) {
        self.closed += 1;
        self.results.push(result);
    }

    fn record_pending(&mut self, result: CloseResult) {
        self.pending += 1;
        self.results.push(result);
    }

    fn record_failed(&mut self, result: CloseResult) {
        self.failed += 1;
        self.results.push(result);
    }

    fn print(
        &self,
        output_format: OutputFormat,
        empty_msg: &str,
        closed_label: &str,
    ) -> Result<()> {
        let structured_payload = CloseSummaryResponse {
            closed: self.closed,
            pending: self.pending,
            failed: self.failed,
            results: self.results.clone(),
        };
        output::emit_by_format(output_format, &structured_payload, || {
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
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempo_common::cli::output::OutputFormat;

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
        summary.record_closed(CloseResult::closed("0x1", None));
        summary.record_closed(CloseResult::closed("0x2", None));
        summary.record_pending(CloseResult::pending("0x3", None, 60));
        summary.record_failed(CloseResult::failed("0x4", None, "timeout"));

        assert_eq!(summary.closed, 2);
        assert_eq!(summary.pending, 1);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.results.len(), 4);
    }

    #[test]
    fn test_close_summary_json_output_no_panic() {
        let mut summary = CloseSummary::new();
        summary.record_closed(CloseResult::closed("0x1", None));
        summary.record_pending(CloseResult::pending("0x2", None, 60));
        summary.record_failed(CloseResult::failed("0x3", None, "timeout"));
        summary
            .print(OutputFormat::Json, "No sessions.", "closed")
            .unwrap();
    }

    #[test]
    fn test_close_summary_text_output_no_panic() {
        let mut summary = CloseSummary::new();
        summary.record_closed(CloseResult::closed("0x1", None));
        summary
            .print(OutputFormat::Text, "No sessions.", "closed")
            .unwrap();
    }
}
