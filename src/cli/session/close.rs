use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};

use super::super::OutputFormat;
use crate::config::Config;
use crate::payment::session::store as session_store;
use crate::payment::session::{
    close_channel_by_id, close_discovered_channel, close_session_from_record,
    find_all_channels_for_payer, CloseOutcome,
};
use crate::wallet::credentials::WalletCredentials;
use crate::wallet::signer::{load_wallet_signer, WalletSigner};

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
#[allow(clippy::too_many_arguments)]
pub async fn close_sessions(
    config: &Config,
    url: Option<String>,
    all: bool,
    orphaned: bool,
    closed: bool,
    output_format: OutputFormat,
    show_output: bool,
    network: Option<&str>,
) -> Result<()> {
    if closed {
        return finalize_closed_channels(config, output_format, show_output, network).await;
    }
    if orphaned {
        return close_orphaned_channels(config, output_format, show_output, network).await;
    }
    if all {
        return close_all_sessions(config, output_format, show_output, network).await;
    }

    if let Some(ref target) = url {
        // If the target looks like a channel ID (0x-prefixed hex), close on-chain directly
        if target.starts_with("0x") && target.len() == 66 {
            return close_by_channel_id(config, target, output_format, show_output, network).await;
        }

        // Otherwise treat as a URL — close the local session
        return close_by_url(config, target, output_format, show_output).await;
    }

    anyhow::bail!(
        "Specify a URL, channel ID (0x...), or use --all/--orphaned/--closed to close sessions"
    );
}

/// Close all local sessions and on-chain orphaned channels.
async fn close_all_sessions(
    config: &Config,
    output_format: OutputFormat,
    show_output: bool,
    network: Option<&str>,
) -> Result<()> {
    let mut summary = CloseSummary::new();

    // Phase 1: close local sessions
    let sessions = session_store::list_sessions()?;
    for session in &sessions {
        let key = session_store::session_key(&session.origin);
        match close_session_from_record(session, config).await {
            Ok(CloseOutcome::Closed { tx_url }) => {
                if let Err(e) = session_store::delete_session(&key) {
                    if show_output {
                        eprintln!("  Failed to remove local session: {e}");
                    }
                }
                let _ = session_store::delete_pending_close(&session.channel_id);
                if show_output {
                    eprintln!("Closed {}", session.origin);
                    print_tx_url(&tx_url);
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
    let local_channel_ids: HashSet<&str> = sessions.iter().map(|s| s.channel_id.as_str()).collect();

    if let Ok(creds) = WalletCredentials::load() {
        if creds.has_wallet() {
            if let Ok(wallet_addr) = creds.wallet_address().parse() {
                let channels = find_all_channels_for_payer(config, wallet_addr, network).await;
                let orphaned: Vec<_> = channels
                    .iter()
                    .filter(|ch| !local_channel_ids.contains(ch.channel_id.as_str()))
                    .collect();

                if show_output && !orphaned.is_empty() {
                    eprintln!("Found {} orphaned channel(s)", orphaned.len());
                }

                for ch in &orphaned {
                    match close_discovered_channel(ch, config).await {
                        Ok(CloseOutcome::Closed { tx_url }) => {
                            let _ = session_store::delete_pending_close(&ch.channel_id);
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
    _show_output: bool,
    network: Option<&str>,
) -> Result<()> {
    match close_channel_by_id(config, target, network, None).await {
        Ok(CloseOutcome::Closed { tx_url }) => {
            let _ = session_store::delete_pending_close(target);
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
            // "not found on any network" means the channel is already
            // fully closed on-chain. Clean up stale local records.
            let err_msg = e.to_string();
            if err_msg.contains("not found on any network") {
                let _ = session_store::delete_pending_close(target);
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
                anyhow::bail!("{e}");
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
) -> Result<()> {
    let key = session_store::session_key(target);
    let session = session_store::load_session(&key)?;

    if let Some(record) = session {
        match close_session_from_record(&record, config).await {
            Ok(CloseOutcome::Closed { tx_url }) => {
                if let Err(e) = session_store::delete_session(&key) {
                    if show_output {
                        eprintln!("  Failed to remove local session: {e}");
                    }
                }
                let _ = session_store::delete_pending_close(&record.channel_id);
                if output_format.is_structured() {
                    println!(
                        "{}",
                        serde_json::json!({"closed": 1, "pending": 0, "failed": 0, "results": [{"origin": target, "channel_id": record.channel_id, "status": "closed"}]})
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
                        serde_json::json!({"closed": 0, "pending": 1, "failed": 0, "results": [{"origin": target, "channel_id": record.channel_id, "status": "pending", "remaining_secs": remaining_secs}]})
                    );
                } else {
                    println!(
                        "Session for {target}: close requested — {} remaining.",
                        format_duration(remaining_secs)
                    );
                }
            }
            Err(e) => {
                if output_format.is_structured() {
                    println!(
                        "{}",
                        serde_json::json!({"closed": 0, "pending": 0, "failed": 1, "results": [{"origin": target, "channel_id": record.channel_id, "status": "error", "error": e.to_string()}]})
                    );
                } else {
                    anyhow::bail!("{e}");
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
    network: Option<&str>,
) -> Result<()> {
    let no_wallet_msg = crate::error::no_wallet_message();
    let creds = WalletCredentials::load().context(no_wallet_msg.clone())?;
    anyhow::ensure!(creds.has_wallet(), "{no_wallet_msg}");
    let wallet_addr = creds
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
        match close_discovered_channel(ch, config).await {
            Ok(CloseOutcome::Closed { tx_url }) => {
                let _ = session_store::delete_pending_close(&ch.channel_id);
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
    network: Option<&str>,
) -> Result<()> {
    let all_pending = session_store::list_all_pending_closes()?;
    let pending: Vec<_> = if let Some(net) = network {
        all_pending
            .into_iter()
            .filter(|p| p.network == net)
            .collect()
    } else {
        all_pending
    };

    if pending.is_empty() {
        let summary = CloseSummary::new();
        summary.print(
            output_format,
            "No channels pending finalization.",
            "finalized",
        )?;
        return Ok(());
    }

    let mut summary = CloseSummary::new();

    // Cache wallet signers per network to avoid redundant disk I/O
    let mut signer_cache: HashMap<String, WalletSigner> = HashMap::new();

    for record in &pending {
        // Load signer once per network
        if !signer_cache.contains_key(&record.network) {
            match load_wallet_signer(&record.network) {
                Ok(w) => {
                    signer_cache.insert(record.network.clone(), w);
                }
                Err(e) => {
                    if show_output {
                        eprintln!("Failed to load wallet for {}", record.network);
                        eprintln!("  {e}");
                    }
                    summary.record_failed(serde_json::json!({
                        "channel_id": record.channel_id,
                        "status": "error",
                        "error": e.to_string(),
                    }));
                    continue;
                }
            }
        }
        let wallet = signer_cache.get(&record.network);

        match close_channel_by_id(config, &record.channel_id, Some(&record.network), wallet).await {
            Ok(CloseOutcome::Closed { tx_url }) => {
                if let Err(e) = session_store::delete_pending_close(&record.channel_id) {
                    tracing::warn!(%e, "failed to delete pending close record");
                }
                if let Err(e) = session_store::delete_session_by_channel_id(&record.channel_id) {
                    tracing::warn!(%e, "failed to delete session record");
                }
                if show_output {
                    eprintln!("Finalized {}", record.channel_id);
                    print_tx_url(&tx_url);
                }
                summary.record_closed(serde_json::json!({
                    "channel_id": record.channel_id,
                    "status": "closed",
                }));
            }
            Ok(CloseOutcome::Pending { remaining_secs }) => {
                if show_output {
                    eprintln!(
                        "Pending {} — {} remaining",
                        record.channel_id,
                        format_duration(remaining_secs)
                    );
                }
                summary.record_pending(serde_json::json!({
                    "channel_id": record.channel_id,
                    "status": "pending",
                    "remaining_secs": remaining_secs,
                }));
            }
            Err(e) => {
                let err_msg = e.to_string();
                if err_msg.contains("not found on any network") {
                    let _ = session_store::delete_pending_close(&record.channel_id);
                    let _ = session_store::delete_session_by_channel_id(&record.channel_id);
                    if show_output {
                        eprintln!("Finalized {} (already settled)", record.channel_id);
                    }
                    summary.record_closed(serde_json::json!({
                        "channel_id": record.channel_id,
                        "status": "closed",
                    }));
                } else {
                    if show_output {
                        eprintln!("Failed to finalize {}", record.channel_id);
                        eprintln!("  {e}");
                    }
                    summary.record_failed(serde_json::json!({
                        "channel_id": record.channel_id,
                        "status": "error",
                        "error": err_msg,
                    }));
                }
            }
        }
    }

    summary.print(
        output_format,
        "No sessions pending finalization.",
        "finalized",
    )?;
    Ok(())
}
