//! Session management commands.

use anyhow::Result;

use crate::network::Network;
use crate::payment::session::close_session_from_record;
use crate::payment::session_store;
use crate::util::format_u256_with_decimals;

/// List all active payment sessions.
pub fn list_sessions() -> Result<()> {
    let sessions = session_store::list_sessions()?;

    if sessions.is_empty() {
        println!("No active sessions.");
        return Ok(());
    }

    println!("Active payment sessions:\n");
    for session in &sessions {
        let expired = if session.is_expired() {
            " (expired)"
        } else {
            ""
        };
        let token_config = session
            .network_name
            .parse::<Network>()
            .ok()
            .and_then(|n| n.token_config_by_address(&session.currency));
        let symbol = token_config.map(|t| t.symbol).unwrap_or("tokens");
        let decimals = token_config.map(|t| t.decimals).unwrap_or(6);

        let cumulative = format_u256_with_decimals(
            alloy::primitives::U256::from(session.cumulative_amount_u128().unwrap_or(0)),
            decimals,
        );
        let deposit = format_u256_with_decimals(
            alloy::primitives::U256::from(session.deposit_u128().unwrap_or(0)),
            decimals,
        );

        println!("  Origin:     {}", session.origin);
        println!("  Network:    {}", session.network_name);
        println!("  Channel:    {}", session.channel_id);
        println!("  Spent:      {cumulative} / {deposit} {symbol}");
        println!(
            "  Status:     {}{}",
            if session.is_expired() {
                "expired"
            } else {
                "active"
            },
            expired
        );
        println!();
    }

    println!("{} session(s) total.", sessions.len());
    Ok(())
}

/// Close a session by URL or close all sessions.
pub async fn close_sessions(url: Option<String>, all: bool) -> Result<()> {
    if all {
        let sessions = session_store::list_sessions()?;
        if sessions.is_empty() {
            println!("No active sessions to close.");
            return Ok(());
        }

        let mut closed = 0;
        let mut failed = 0;
        for session in &sessions {
            let key = session_store::session_key(&session.origin);
            let _lock = session_store::lock_session(&key)?;
            eprintln!("Closing session for {}...", session.origin);
            if let Err(e) = close_session_from_record(session).await {
                eprintln!("  Failed to close: {e}");
                eprintln!("  Keeping local record for retry.");
                failed += 1;
            } else {
                closed += 1;
                if let Err(e) = session_store::delete_session(&key) {
                    eprintln!("  Failed to remove local session: {e}");
                }
            }
        }

        if failed > 0 {
            println!("Closed {closed} session(s), {failed} failed.");
        } else {
            println!("Closed {closed} session(s).");
        }
        return Ok(());
    }

    if let Some(ref url) = url {
        let key = session_store::session_key(url);
        let _lock = session_store::lock_session(&key)?;
        let session = session_store::load_session(&key)?;

        if let Some(record) = session {
            eprintln!("Closing session for {url}...");
            match close_session_from_record(&record).await {
                Ok(()) => {
                    if let Err(e) = session_store::delete_session(&key) {
                        eprintln!("Failed to remove local session: {e}");
                    } else {
                        println!("Session closed.");
                    }
                }
                Err(e) => {
                    eprintln!("Failed to close session: {e}");
                    eprintln!("Keeping local record for retry.");
                }
            }
        } else {
            println!("No active session for {url}");
        }
        return Ok(());
    }

    anyhow::bail!("Specify a URL or use --all to close all sessions");
}
