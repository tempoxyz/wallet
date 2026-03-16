use std::collections::HashSet;

use super::{session, ChannelStatus};
use tempo_common::{
    cli::{context::Context, format::format_duration, output, output::OutputFormat},
    error::{ConfigError, InputError, PaymentError, TempoError},
    payment::session::{
        close_channel_by_id, close_channel_from_record, close_channel_from_record_cooperative,
        close_discovered_channel, find_all_channels_for_payer, CloseOutcome,
    },
};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CloseSelection<'a> {
    Finalize,
    Orphaned,
    All,
    Target(&'a str),
    Missing,
}

const fn determine_close_selection(
    target: Option<&str>,
    all: bool,
    orphaned: bool,
    finalize: bool,
) -> CloseSelection<'_> {
    if finalize {
        return CloseSelection::Finalize;
    }
    if orphaned {
        return CloseSelection::Orphaned;
    }
    if all {
        return CloseSelection::All;
    }
    if let Some(target) = target {
        return CloseSelection::Target(target);
    }
    CloseSelection::Missing
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
    cooperative: bool,
    dry_run: bool,
) -> Result<(), TempoError> {
    if cooperative && (all || orphaned || finalize) {
        return Err(InputError::InvalidSessionCloseCooperativeCombination.into());
    }

    if dry_run && url.is_none() && !all && !orphaned && !finalize {
        return Err(InputError::MissingSessionCloseTarget.into());
    }

    if !ctx.keys.has_wallet() {
        return Err(ConfigError::Missing(
            "No wallet configured. Log in with 'tempo wallet login'.".to_string(),
        )
        .into());
    }

    // CLI flag semantics: `--cooperative` means cooperative-only (no fallback).
    let cooperative_only = cooperative;

    let selection = determine_close_selection(url.as_deref(), all, orphaned, finalize);

    if dry_run {
        return dry_run_close(ctx, selection).await;
    }

    match selection {
        CloseSelection::Finalize => finalize_closed_channels(ctx).await,
        CloseSelection::Orphaned => close_orphaned_channels(ctx).await,
        CloseSelection::All => close_all_sessions(ctx, cooperative_only).await,
        CloseSelection::Target(target) => {
            if target.starts_with("0x") {
                super::util::validate_channel_id(target)?;
                close_by_channel_id(ctx, target, cooperative_only).await
            } else {
                close_by_url(ctx, target, cooperative_only).await
            }
        }
        CloseSelection::Missing => Err(InputError::MissingSessionCloseTarget.into()),
    }
}

async fn dry_run_close(ctx: &Context, selection: CloseSelection<'_>) -> Result<(), TempoError> {
    #[derive(serde::Serialize)]
    struct DryRunResponse {
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
    let all_sessions = session::list_channels()?;
    let local_sessions: Vec<_> = all_sessions
        .iter()
        .filter(|s| s.network_id() == ctx.network)
        .collect();
    let now = session::now_secs();

    match selection {
        CloseSelection::All => {
            for s in &local_sessions {
                targets.push(DryRunTarget {
                    channel_id: s.channel_id_hex(),
                    origin: Some(s.origin.clone()),
                    state: Some(format!("{:?}", s.state)),
                });
            }

            let local_ids: HashSet<_> = local_sessions.iter().map(|s| s.channel_id).collect();
            if let Some(wallet_addr) = ctx.keys.wallet_address_parsed() {
                let channels =
                    find_all_channels_for_payer(&ctx.config, wallet_addr, ctx.network).await;
                for ch in &channels {
                    if local_ids.contains(&ch.channel_id) {
                        continue;
                    }
                    let state = if ch.close_requested_at == 0 {
                        ChannelStatus::Orphaned
                    } else {
                        let grace = super::util::resolve_grace_period(
                            &ctx.config,
                            ctx.network,
                            ch.escrow_contract,
                        )
                        .await;
                        let ready_at = ch.close_requested_at.saturating_add(grace);
                        if ready_at <= now {
                            ChannelStatus::Finalizable
                        } else {
                            ChannelStatus::Closing
                        }
                    };

                    targets.push(DryRunTarget {
                        channel_id: format!("{:#x}", ch.channel_id),
                        origin: None,
                        state: Some(format!("{state:?}")),
                    });
                }
            }
        }
        CloseSelection::Finalize => {
            for s in &local_sessions {
                let (status, _) = s.status_at(now);
                if matches!(status, ChannelStatus::Finalizable) {
                    targets.push(DryRunTarget {
                        channel_id: s.channel_id_hex(),
                        origin: Some(s.origin.clone()),
                        state: Some("Finalizable".to_string()),
                    });
                }
            }

            let local_ids: HashSet<_> = local_sessions.iter().map(|s| s.channel_id).collect();
            if let Some(wallet_addr) = ctx.keys.wallet_address_parsed() {
                let channels =
                    find_all_channels_for_payer(&ctx.config, wallet_addr, ctx.network).await;
                for ch in &channels {
                    if local_ids.contains(&ch.channel_id) {
                        continue;
                    }
                    if ch.close_requested_at == 0 {
                        continue;
                    }
                    let grace = super::util::resolve_grace_period(
                        &ctx.config,
                        ctx.network,
                        ch.escrow_contract,
                    )
                    .await;
                    let ready_at = ch.close_requested_at.saturating_add(grace);
                    if ready_at > now {
                        continue;
                    }

                    targets.push(DryRunTarget {
                        channel_id: format!("{:#x}", ch.channel_id),
                        origin: None,
                        state: Some("Finalizable".to_string()),
                    });
                }
            }
        }
        CloseSelection::Orphaned => {
            let local_ids: HashSet<_> = local_sessions.iter().map(|s| s.channel_id).collect();
            if let Some(wallet_addr) = ctx.keys.wallet_address_parsed() {
                let channels =
                    find_all_channels_for_payer(&ctx.config, wallet_addr, ctx.network).await;
                for ch in &channels {
                    if local_ids.contains(&ch.channel_id) {
                        continue;
                    }
                    let state = if ch.close_requested_at == 0 {
                        ChannelStatus::Orphaned
                    } else {
                        let grace = super::util::resolve_grace_period(
                            &ctx.config,
                            ctx.network,
                            ch.escrow_contract,
                        )
                        .await;
                        let ready_at = ch.close_requested_at.saturating_add(grace);
                        if ready_at <= now {
                            ChannelStatus::Finalizable
                        } else {
                            ChannelStatus::Closing
                        }
                    };

                    targets.push(DryRunTarget {
                        channel_id: format!("{:#x}", ch.channel_id),
                        origin: None,
                        state: Some(format!("{state:?}")),
                    });
                }
            }
        }
        CloseSelection::Target(target) => {
            if super::util::is_channel_id(target) {
                targets.push(DryRunTarget {
                    channel_id: target.to_string(),
                    origin: None,
                    state: None,
                });
            } else {
                let origin = normalize_origin(target);
                let records: Vec<_> = session::load_channels_by_origin(&origin)?
                    .into_iter()
                    .filter(|record| record.network_id() == ctx.network)
                    .collect();
                if records.is_empty() {
                    targets.push(DryRunTarget {
                        channel_id: String::new(),
                        origin: Some(target.to_string()),
                        state: Some("not found".to_string()),
                    });
                } else {
                    for rec in records {
                        targets.push(DryRunTarget {
                            channel_id: rec.channel_id_hex(),
                            origin: Some(rec.origin.clone()),
                            state: Some(format!("{:?}", rec.state)),
                        });
                    }
                }
            }
        }
        CloseSelection::Missing => {}
    }

    let response = DryRunResponse { targets };

    output::emit_by_format(ctx.output_format, &response, || {
        eprintln!(
            "[DRY RUN] Would close {} session(s)",
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
    })?;

    Ok(())
}

async fn close_local_record(
    record: &session::ChannelRecord,
    ctx: &Context,
    analytics: Option<&tempo_common::analytics::Analytics>,
    cooperative_only: bool,
) -> Result<CloseOutcome, TempoError> {
    if cooperative_only {
        return close_channel_from_record_cooperative(record, analytics, &ctx.keys).await;
    }
    // Default mode: cooperative-first, with on-chain fallback if cooperative close fails.
    close_channel_from_record(record, &ctx.config, analytics, &ctx.keys).await
}

/// Close all local sessions and on-chain orphaned channels.
async fn close_all_sessions(ctx: &Context, cooperative_only: bool) -> Result<(), TempoError> {
    let show_output = ctx.verbosity.show_output;
    let analytics = ctx.analytics.as_ref();
    let mut summary = CloseSummary::new();

    // Phase 1: close local sessions (scoped to current network)
    let all_sessions = session::list_channels()?;
    let sessions: Vec<_> = all_sessions
        .iter()
        .filter(|s| s.network_id() == ctx.network)
        .collect();
    for session in &sessions {
        let result = close_local_record(session, ctx, analytics, cooperative_only).await;
        if matches!(result, Ok(CloseOutcome::Closed { .. })) {
            if let Err(e) = session::delete_channel(&session.channel_id_hex()) {
                if show_output {
                    eprintln!("  Failed to remove local session: {e}");
                }
            }
        }
        let channel_id = session.channel_id_hex();
        summary.record_outcome(
            result,
            Some(&session.origin),
            &session.origin,
            &channel_id,
            show_output,
        );
    }

    // Phase 2: scan on-chain for orphaned channels
    if !cooperative_only {
        close_orphaned_into_summary(ctx, &all_sessions, &mut summary).await;
    }

    summary.print(ctx.output_format, "No active sessions to close.", "closed")?;
    Ok(())
}

/// Close a single channel by its on-chain ID (0x...).
///
/// If a local session record exists for this channel, routes through
/// `close_channel_from_record` which tries cooperative close first.
/// Falls back to on-chain-only close when no local record is found.
async fn close_by_channel_id(
    ctx: &Context,
    target: &str,
    cooperative_only: bool,
) -> Result<(), TempoError> {
    let channel_id = super::util::parse_channel_id(target)?;

    // Try local session record first — enables cooperative close
    if let Ok(Some(record)) = session::load_channel(&format!("{channel_id:#x}")) {
        if record.network_id() == ctx.network {
            let show_output = ctx.verbosity.show_output;
            let analytics = ctx.analytics.as_ref();
            let mut summary = CloseSummary::new();

            let result = close_local_record(&record, ctx, analytics, cooperative_only).await;
            if matches!(result, Ok(CloseOutcome::Closed { .. })) {
                if let Err(e) = session::delete_channel(&record.channel_id_hex()) {
                    if show_output {
                        eprintln!("  Failed to remove local session: {e}");
                    }
                }
            }
            let cid = record.channel_id_hex();
            summary.record_outcome(
                result,
                Some(&record.origin),
                &record.origin,
                &cid,
                show_output,
            );
            return summary.print(ctx.output_format, "No channel to close.", "closed");
        }
    }

    if cooperative_only {
        return Err(InputError::SessionCloseCooperativeRequiresLocalRecord.into());
    }

    // No local record (orphaned channel) — on-chain close only
    let mut summary = CloseSummary::new();
    let result = close_channel_by_id(&ctx.config, target, ctx.network, None, &ctx.keys).await;
    let show_output = ctx.verbosity.show_output && !ctx.output_format.is_structured();
    summary.record_finalize_outcome(result, target, show_output);
    summary.print(ctx.output_format, "No channel to close.", "closed")
}

/// Close a session by URL (local session lookup).
async fn close_by_url(
    ctx: &Context,
    target: &str,
    cooperative_only: bool,
) -> Result<(), TempoError> {
    let show_output = ctx.verbosity.show_output;
    let output_format = ctx.output_format;
    let analytics = ctx.analytics.as_ref();

    let origin = normalize_origin(target);
    let sessions: Vec<_> = session::load_channels_by_origin(&origin)?
        .into_iter()
        .filter(|record| record.network_id() == ctx.network)
        .collect();
    let mut summary = CloseSummary::new();

    if !sessions.is_empty() {
        for record in sessions {
            let result = close_local_record(&record, ctx, analytics, cooperative_only).await;
            if matches!(result, Ok(CloseOutcome::Closed { .. })) {
                if let Err(e) = session::delete_channel(&record.channel_id_hex()) {
                    if show_output {
                        eprintln!("  Failed to remove local session: {e}");
                    }
                }
            }
            let channel_id = record.channel_id_hex();
            summary.record_outcome(
                result,
                Some(&record.origin),
                &record.origin,
                &channel_id,
                show_output,
            );
        }
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
    local_sessions: &[session::ChannelRecord],
    summary: &mut CloseSummary,
) {
    let show_output = ctx.verbosity.show_output;

    let Some(wallet_addr) = ctx.keys.wallet_address_parsed() else {
        return;
    };

    let local_ids: HashSet<_> = local_sessions.iter().map(|s| s.channel_id).collect();

    let channels = find_all_channels_for_payer(&ctx.config, wallet_addr, ctx.network).await;
    let orphaned: Vec<_> = channels
        .iter()
        .filter(|ch| !local_ids.contains(&ch.channel_id))
        .collect();

    if show_output && !orphaned.is_empty() {
        eprintln!("Found {} orphaned channel(s)", orphaned.len());
    }

    for ch in &orphaned {
        let channel_id_hex = format!("{:#x}", ch.channel_id);
        let result = close_discovered_channel(ch, &ctx.config, &ctx.keys).await;
        if matches!(result, Ok(CloseOutcome::Closed { .. })) {
            let _ = session::delete_channel(&format!("{:#x}", ch.channel_id));
        }
        summary.record_outcome(result, None, &channel_id_hex, &channel_id_hex, show_output);
    }
}

/// Close only orphaned on-chain channels (channels with no local session record).
async fn close_orphaned_channels(ctx: &Context) -> Result<(), TempoError> {
    if !ctx.keys.has_wallet() {
        return Err(ConfigError::Missing(
            "No wallet configured. Log in with 'tempo wallet login'.".to_string(),
        )
        .into());
    }

    let local_sessions = session::list_channels()?;
    let mut summary = CloseSummary::new();

    close_orphaned_into_summary(ctx, &local_sessions, &mut summary).await;

    summary.print(ctx.output_format, "No orphaned channels found.", "closed")?;
    Ok(())
}

/// Finalize channels that have had `requestClose()` submitted and whose grace period has elapsed.
async fn finalize_closed_channels(ctx: &Context) -> Result<(), TempoError> {
    let show_output = ctx.verbosity.show_output;
    let now = session::now_secs();
    let mut summary = CloseSummary::new();
    let mut attempted = HashSet::new();

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
    for s in session::list_channels()? {
        if s.network_id() != ctx.network {
            continue;
        }
        if !(s.state == ChannelStatus::Closing && now >= s.grace_ready_at) {
            continue;
        }
        attempted.insert(s.channel_id);
        let channel_id = s.channel_id_hex();
        let Some(ref wallet) = wallet else {
            summary.record_failed(CloseResult::failed(
                &channel_id,
                None,
                "no wallet available",
            ));
            continue;
        };
        let result = close_channel_by_id(
            &ctx.config,
            &channel_id,
            ctx.network,
            Some(wallet),
            &ctx.keys,
        )
        .await;
        summary.record_finalize_outcome(result, &channel_id, show_output);
    }

    // 2) Orphaned channels ready to finalize
    if let Some(wallet_addr) = ctx.keys.wallet_address_parsed() {
        let channels = find_all_channels_for_payer(&ctx.config, wallet_addr, ctx.network).await;
        for ch in &channels {
            if attempted.contains(&ch.channel_id) {
                continue;
            }
            if ch.close_requested_at == 0 {
                continue;
            }
            attempted.insert(ch.channel_id);
            let channel_id_hex = format!("{:#x}", ch.channel_id);
            let Some(ref wallet) = wallet else {
                summary.record_failed(CloseResult::failed(
                    &channel_id_hex,
                    None,
                    "no wallet available",
                ));
                continue;
            };
            // Check grace readiness from on-chain constant
            let grace =
                super::util::resolve_grace_period(&ctx.config, ctx.network, ch.escrow_contract)
                    .await;
            let ready_at = ch.close_requested_at + grace;
            if now < ready_at {
                continue;
            }
            let result = close_channel_by_id(
                &ctx.config,
                &channel_id_hex,
                ctx.network,
                Some(wallet),
                &ctx.keys,
            )
            .await;
            summary.record_finalize_outcome(result, &channel_id_hex, show_output);
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

type CloseOpResult = std::result::Result<CloseOutcome, TempoError>;

impl CloseSummary {
    const fn new() -> Self {
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
        result: CloseOpResult,
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
        result: CloseOpResult,
        channel_id: &str,
        show_output: bool,
    ) {
        match result {
            Err(TempoError::Payment(PaymentError::ChannelNotFound { .. })) => {
                maybe_delete_session_by_channel_id(channel_id);
                if show_output {
                    eprintln!("Finalized {channel_id} (already settled)");
                }
                self.record_closed(CloseResult::closed(channel_id, None));
            }
            other => {
                if matches!(other, Ok(CloseOutcome::Closed { .. })) {
                    maybe_delete_session_by_channel_id(channel_id);
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
    ) -> Result<(), TempoError> {
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
        })?;

        Ok(())
    }
}

fn maybe_delete_session_by_channel_id(channel_id: &str) {
    match super::util::parse_channel_id(channel_id) {
        Ok(parsed) => {
            let _ = session::delete_channel(&format!("{parsed:#x}"));
        }
        Err(err) => {
            tracing::warn!(
                channel_id,
                error = %err,
                "Skipping local session deletion for malformed channel ID"
            );
        }
    }
}

fn normalize_origin(target: &str) -> String {
    url::Url::parse(target)
        .map_or_else(|_| target.to_string(), |u| u.origin().ascii_serialization())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempo_common::cli::output::OutputFormat;

    #[test]
    fn test_determine_close_selection_precedence() {
        assert_eq!(
            determine_close_selection(Some("https://x"), true, true, true),
            CloseSelection::Finalize
        );
        assert_eq!(
            determine_close_selection(Some("https://x"), true, true, false),
            CloseSelection::Orphaned
        );
        assert_eq!(
            determine_close_selection(Some("https://x"), true, false, false),
            CloseSelection::All
        );
        assert_eq!(
            determine_close_selection(Some("https://x"), false, false, false),
            CloseSelection::Target("https://x")
        );
        assert_eq!(
            determine_close_selection(None, false, false, false),
            CloseSelection::Missing
        );
    }

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
