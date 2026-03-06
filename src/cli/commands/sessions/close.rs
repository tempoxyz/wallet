use std::collections::HashSet;

use alloy::primitives::Address;
use anyhow::{Context, Result};

use crate::analytics::Analytics;
use crate::cli::OutputFormat;
use crate::config::Config;
use crate::error::TempoWalletError;
use crate::keys::Keystore;
use crate::network::NetworkId;
use crate::payment::session::channel::find_all_channels_for_payer;
use crate::payment::session::close::{
    close_channel_by_id, close_discovered_channel, close_session_from_record,
};
use crate::payment::session::store as session_store;
use crate::payment::session::store::SessionStatus;
use crate::payment::session::CloseOutcome;
use crate::payment::session::DEFAULT_GRACE_PERIOD_SECS;

use super::render::{format_duration, CloseSummary};

/// Print the settlement tx URL indented under a close message.
fn print_tx_url(tx_url: &Option<String>) {
    if let Some(url) = tx_url {
        eprintln!("  {url}");
    }
}

/// Close a session by URL or close all sessions.
///
/// When `--all` is used, this first closes local sessions, then scans on-chain
/// for any orphaned channels belonging to the current wallet and closes those too.
pub(super) async fn close_sessions(
    ctx: &crate::cli::Context,
    url: Option<String>,
    all: bool,
    orphaned: bool,
    finalize: bool,
) -> Result<()> {
    let config = &ctx.config;
    let output_format = ctx.output_format;
    let show_output = ctx.cli.verbosity().show_output;
    let network = ctx.network;
    let analytics = ctx.analytics.as_ref();
    let keys = &ctx.keys;

    if finalize {
        return finalize_closed_channels(config, output_format, show_output, network, keys).await;
    }
    if orphaned {
        return close_orphaned_channels(config, output_format, show_output, network, keys).await;
    }
    if all {
        return close_all_sessions(config, output_format, show_output, network, analytics, keys)
            .await;
    }

    if let Some(ref target) = url {
        // If the target looks like a channel ID (0x-prefixed hex), close on-chain directly
        if target.starts_with("0x") && target.len() == 66 {
            return close_by_channel_id(config, target, output_format, network, keys).await;
        }

        // Otherwise treat as a URL — close the local session
        return close_by_url(config, target, output_format, show_output, analytics, keys).await;
    }

    anyhow::bail!(TempoWalletError::InvalidUrl(
        "Specify a URL, channel ID (0x...), or use --all/--orphaned/--finalize to close sessions"
            .to_string()
    ));
}

/// Close all local sessions and on-chain orphaned channels.
async fn close_all_sessions(
    config: &Config,
    output_format: OutputFormat,
    show_output: bool,
    network: NetworkId,
    analytics: Option<&Analytics>,
    keys: &Keystore,
) -> Result<()> {
    let mut summary = CloseSummary::new();

    // Phase 1: close local sessions (scoped to current network)
    let all_sessions = session_store::list_sessions()?;
    let net = network.as_str();
    let sessions: Vec<_> = all_sessions
        .iter()
        .filter(|s| s.network_name == net)
        .collect();
    for session in &sessions {
        let key = session_store::session_key(&session.origin);
        match close_session_from_record(session, config, analytics, keys).await {
            Ok(CloseOutcome::Closed {
                tx_url,
                amount_display,
            }) => {
                if let Err(e) = session_store::delete_session(&key) {
                    if show_output {
                        eprintln!("  Failed to remove local session: {e}");
                    }
                }
                if show_output {
                    eprintln!("Closed {}", session.origin);
                    print_tx_url(&tx_url);
                    if let Some(url) = &tx_url {
                        if let Some(ref amt) = amount_display {
                            eprintln!("Paid {amt} · {url}");
                        } else {
                            eprintln!("Paid settlement · {url}");
                        }
                    }
                }
                summary.record_closed(serde_json::json!({
                    "origin": session.origin,
                    "channel_id": session.channel_id,
                    "status": "closed",
                }));
            }
            Ok(CloseOutcome::Pending { remaining_secs }) => {
                if show_output {
                    eprintln!(
                        "Pending {} — {} remaining",
                        session.origin,
                        format_duration(remaining_secs)
                    );
                }
                summary.record_pending(serde_json::json!({
                    "origin": session.origin,
                    "channel_id": session.channel_id,
                    "status": "pending",
                    "remaining_secs": remaining_secs,
                }));
            }
            Err(e) => {
                if show_output {
                    eprintln!("Failed to close {}", session.origin);
                    eprintln!("  {e:#}");
                }
                summary.record_failed(serde_json::json!({
                    "origin": session.origin,
                    "channel_id": session.channel_id,
                    "status": "error",
                    "error": format!("{e:#}"),
                }));
            }
        }
    }

    // Phase 2: scan on-chain for orphaned channels
    let local_channel_ids: HashSet<&str> =
        all_sessions.iter().map(|s| s.channel_id.as_str()).collect();

    if let Some(wallet_addr) = keys
        .has_wallet()
        .then(|| keys.wallet_address().parse::<Address>().ok())
        .flatten()
    {
        let channels = find_all_channels_for_payer(config, wallet_addr, network).await;
        let orphaned: Vec<_> = channels
            .iter()
            .filter(|ch| !local_channel_ids.contains(ch.channel_id.as_str()))
            .collect();

        if show_output && !orphaned.is_empty() {
            eprintln!("Found {} orphaned channel(s)", orphaned.len());
        }

        for ch in &orphaned {
            match close_discovered_channel(ch, config, keys).await {
                Ok(CloseOutcome::Closed {
                    tx_url,
                    amount_display,
                }) => {
                    if show_output {
                        eprintln!("Closed {}", ch.channel_id);
                        print_tx_url(&tx_url);
                        if let Some(url) = &tx_url {
                            if let Some(ref amt) = amount_display {
                                eprintln!("Paid {amt} · {url}");
                            } else {
                                eprintln!("Paid settlement · {url}");
                            }
                        }
                    }
                    summary.record_closed(serde_json::json!({
                        "channel_id": ch.channel_id,
                        "status": "closed",
                    }));
                }
                Ok(CloseOutcome::Pending { remaining_secs }) => {
                    if show_output {
                        eprintln!(
                            "Pending {} — {} remaining",
                            ch.channel_id,
                            format_duration(remaining_secs)
                        );
                    }
                    summary.record_pending(serde_json::json!({
                        "channel_id": ch.channel_id,
                        "status": "pending",
                        "remaining_secs": remaining_secs,
                    }));
                }
                Err(e) => {
                    if show_output {
                        eprintln!("Failed to close {}", ch.channel_id);
                        eprintln!("  {e}");
                    }
                    summary.record_failed(serde_json::json!({
                        "channel_id": ch.channel_id,
                        "status": "error",
                        "error": e.to_string()
                    }));
                }
            }
        }
    }

    summary.print(output_format, "No active sessions to close.", "closed")?;
    Ok(())
}

/// Close a single channel by its on-chain ID (0x...).
async fn close_by_channel_id(
    config: &Config,
    target: &str,
    output_format: OutputFormat,
    network: NetworkId,
    keys: &Keystore,
) -> Result<()> {
    match close_channel_by_id(config, target, network, None, keys).await {
        Ok(CloseOutcome::Closed { tx_url, .. }) => {
            let _ = session_store::delete_session_by_channel_id(target);
            if output_format.is_structured() {
                println!(
                    "{}",
                    serde_json::json!({"closed": 1, "pending": 0, "failed": 0, "results": [{"channel_id": target, "status": "closed"}]})
                );
            } else {
                println!("Closed {target}");
                if let Some(url) = &tx_url {
                    println!("  {url}");
                }
            }
        }
        Ok(CloseOutcome::Pending { remaining_secs }) => {
            if output_format.is_structured() {
                println!(
                    "{}",
                    serde_json::json!({"closed": 0, "pending": 1, "failed": 0, "results": [{"channel_id": target, "status": "pending", "remaining_secs": remaining_secs}]})
                );
            } else {
                println!(
                    "Channel {target}: close requested — {} remaining.",
                    format_duration(remaining_secs)
                );
            }
        }
        Err(e) => {
            // "not found on" means the channel is already
            // fully closed on-chain. Clean up stale local records.
            let err_msg = e.to_string();
            if err_msg.contains("not found on") {
                let _ = session_store::delete_session_by_channel_id(target);
                if output_format.is_structured() {
                    println!(
                        "{}",
                        serde_json::json!({"closed": 1, "pending": 0, "failed": 0, "results": [{"channel_id": target, "status": "closed"}]})
                    );
                } else {
                    println!("Channel {target} already closed.");
                }
            } else if output_format.is_structured() {
                println!(
                    "{}",
                    serde_json::json!({"closed": 0, "pending": 0, "failed": 1, "results": [{"channel_id": target, "status": "error", "error": err_msg}]})
                );
            } else {
                return Err(e);
            }
        }
    }
    Ok(())
}

/// Close a session by URL (local session lookup).
async fn close_by_url(
    config: &Config,
    target: &str,
    output_format: OutputFormat,
    show_output: bool,
    analytics: Option<&Analytics>,
    keys: &Keystore,
) -> Result<()> {
    let key = session_store::session_key(target);
    let session = session_store::load_session(&key)?;

    if let Some(record) = session {
        match close_session_from_record(&record, config, analytics, keys).await {
            Ok(CloseOutcome::Closed {
                tx_url,
                amount_display,
            }) => {
                if let Err(e) = session_store::delete_session(&key) {
                    if show_output {
                        eprintln!("  Failed to remove local session: {e}");
                    }
                }
                if output_format.is_structured() {
                    println!(
                        "{}",
                        serde_json::json!({"closed": 1, "pending": 0, "failed": 0, "results": [{"origin": record.origin, "channel_id": record.channel_id, "status": "closed"}]})
                    );
                } else {
                    println!("Closed {}", record.origin);
                    if let Some(url) = &tx_url {
                        if let Some(ref amt) = amount_display {
                            println!("Paid {amt} · {url}");
                        } else {
                            println!("Paid settlement · {url}");
                        }
                    }
                }
            }
            Ok(CloseOutcome::Pending { remaining_secs }) => {
                if output_format.is_structured() {
                    println!(
                        "{}",
                        serde_json::json!({"closed": 0, "pending": 1, "failed": 0, "results": [{"origin": record.origin, "channel_id": record.channel_id, "status": "pending", "remaining_secs": remaining_secs}]})
                    );
                } else {
                    println!(
                        "Session for {}: close requested — {} remaining.",
                        record.origin,
                        format_duration(remaining_secs)
                    );
                }
            }
            Err(e) => {
                if output_format.is_structured() {
                    println!(
                        "{}",
                        serde_json::json!({"closed": 0, "pending": 0, "failed": 1, "results": [{"origin": record.origin, "channel_id": record.channel_id, "status": "error", "error": e.to_string()}]})
                    );
                } else {
                    return Err(e);
                }
            }
        }
    } else if output_format.is_structured() {
        println!(
            "{}",
            serde_json::json!({"closed": 0, "pending": 0, "failed": 1, "results": [{"origin": target, "status": "error", "error": "no active session"}]})
        );
    } else {
        println!("No active session for {target}");
    }

    Ok(())
}

/// Close only orphaned on-chain channels (channels with no local session record).
async fn close_orphaned_channels(
    config: &Config,
    output_format: OutputFormat,
    show_output: bool,
    network: NetworkId,
    keys: &Keystore,
) -> Result<()> {
    if !keys.has_wallet() {
        anyhow::bail!(TempoWalletError::ConfigMissing(
            "No wallet configured. Log in with 'tempo-wallet login'.".to_string()
        ));
    }
    let wallet_addr = keys
        .wallet_address()
        .parse()
        .context("Invalid wallet address")?;

    let local_sessions = session_store::list_sessions()?;
    let local_ids: HashSet<String> = local_sessions
        .iter()
        .map(|s| s.channel_id.to_lowercase())
        .collect();

    let channels = find_all_channels_for_payer(config, wallet_addr, network).await;
    let orphaned: Vec<_> = channels
        .iter()
        .filter(|ch| !local_ids.contains(&ch.channel_id.to_lowercase()))
        .collect();

    if orphaned.is_empty() {
        let summary = CloseSummary::new();
        summary.print(output_format, "No orphaned channels found.", "closed")?;
        return Ok(());
    }

    if show_output {
        eprintln!("Found {} orphaned channel(s)", orphaned.len());
    }

    let mut summary = CloseSummary::new();

    for ch in &orphaned {
        match close_discovered_channel(ch, config, keys).await {
            Ok(CloseOutcome::Closed { tx_url, .. }) => {
                let _ = session_store::delete_session_by_channel_id(&ch.channel_id);
                if show_output {
                    eprintln!("Closed {}", ch.channel_id);
                    print_tx_url(&tx_url);
                }
                summary.record_closed(serde_json::json!({
                    "channel_id": ch.channel_id,
                    "status": "closed",
                }));
            }
            Ok(CloseOutcome::Pending { remaining_secs }) => {
                if show_output {
                    eprintln!(
                        "Pending {} — {} remaining",
                        ch.channel_id,
                        format_duration(remaining_secs)
                    );
                }
                summary.record_pending(serde_json::json!({
                    "channel_id": ch.channel_id,
                    "status": "pending",
                    "remaining_secs": remaining_secs,
                }));
            }
            Err(e) => {
                if show_output {
                    eprintln!("Failed to close {}", ch.channel_id);
                    eprintln!("  {e}");
                }
                summary.record_failed(serde_json::json!({
                    "channel_id": ch.channel_id,
                    "status": "error",
                    "error": e.to_string()
                }));
            }
        }
    }

    summary.print(output_format, "No orphaned sessions found.", "closed")?;
    Ok(())
}

/// Finalize channels that have had requestClose() submitted and whose grace period has elapsed.
async fn finalize_closed_channels(
    config: &Config,
    output_format: OutputFormat,
    show_output: bool,
    network: NetworkId,
    keys: &Keystore,
) -> Result<()> {
    let now = session_store::now_secs();
    let mut summary = CloseSummary::new();

    // Load wallet signer once (all channels share the same network)
    let wallet = match keys.signer(network) {
        Ok(w) => Some(w),
        Err(e) => {
            if show_output {
                eprintln!("Failed to load wallet for {network}");
                eprintln!("  {e}");
            }
            None
        }
    };

    // 1) Local sessions ready to finalize
    for s in session_store::list_sessions()? {
        if s.network_name != network.as_str() {
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
        match close_channel_by_id(config, &s.channel_id, network, Some(wallet), keys).await {
            Ok(CloseOutcome::Closed { tx_url, .. }) => {
                if let Err(e) = session_store::delete_session_by_channel_id(&s.channel_id) {
                    tracing::warn!(%e, "failed to delete session record");
                }
                if show_output {
                    eprintln!("Finalized {}", s.channel_id);
                    print_tx_url(&tx_url);
                }
                summary.record_closed(serde_json::json!({
                    "channel_id": s.channel_id,
                    "status": "closed",
                }));
            }
            Ok(CloseOutcome::Pending { remaining_secs }) => {
                if show_output {
                    eprintln!(
                        "Pending {} — {} remaining",
                        s.channel_id,
                        format_duration(remaining_secs)
                    );
                }
                summary.record_pending(serde_json::json!({
                    "channel_id": s.channel_id,
                    "status": "pending",
                    "remaining_secs": remaining_secs,
                }));
            }
            Err(e) => {
                let err_msg = e.to_string();
                if err_msg.contains("not found on") {
                    let _ = session_store::delete_session_by_channel_id(&s.channel_id);
                    if show_output {
                        eprintln!("Finalized {} (already settled)", s.channel_id);
                    }
                    summary.record_closed(serde_json::json!({
                        "channel_id": s.channel_id,
                        "status": "closed",
                    }));
                } else {
                    if show_output {
                        eprintln!("Failed to finalize {}", s.channel_id);
                        eprintln!("  {e}");
                    }
                    summary.record_failed(serde_json::json!({
                        "channel_id": s.channel_id,
                        "status": "error",
                        "error": err_msg,
                    }));
                }
            }
        }
    }

    // 2) Orphaned channels ready to finalize
    if let Some(wallet_addr) = keys
        .has_wallet()
        .then(|| keys.wallet_address().parse::<Address>().ok())
        .flatten()
    {
        let channels = find_all_channels_for_payer(config, wallet_addr, network).await;
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
            let grace = crate::payment::session::channel::read_grace_period(
                &alloy::providers::RootProvider::<alloy::network::Ethereum>::new_http(
                    config.rpc_url(network),
                ),
                ch.escrow_contract.parse().ok().unwrap_or_default(),
            )
            .await
            .unwrap_or(DEFAULT_GRACE_PERIOD_SECS);
            let ready_at = ch.close_requested_at + grace;
            if now < ready_at {
                continue;
            }
            match close_channel_by_id(config, &ch.channel_id, network, Some(wallet), keys).await {
                Ok(CloseOutcome::Closed { tx_url, .. }) => {
                    if show_output {
                        eprintln!("Finalized {}", ch.channel_id);
                        print_tx_url(&tx_url);
                    }
                    summary.record_closed(serde_json::json!({
                        "channel_id": ch.channel_id,
                        "status": "closed",
                    }));
                }
                Ok(CloseOutcome::Pending { remaining_secs }) => {
                    if show_output {
                        eprintln!(
                            "Pending {} — {} remaining",
                            ch.channel_id,
                            format_duration(remaining_secs)
                        );
                    }
                    summary.record_pending(serde_json::json!({
                        "channel_id": ch.channel_id,
                        "status": "pending",
                        "remaining_secs": remaining_secs,
                    }));
                }
                Err(e) => {
                    let err_msg = e.to_string();
                    if show_output {
                        eprintln!("Failed to finalize {}", ch.channel_id);
                        eprintln!("  {e}");
                    }
                    summary.record_failed(serde_json::json!({
                        "channel_id": ch.channel_id,
                        "status": "error",
                        "error": err_msg,
                    }));
                }
            }
        }
    }

    summary.print(
        output_format,
        "No channels pending finalization.",
        "finalized",
    )?;
    Ok(())
}
