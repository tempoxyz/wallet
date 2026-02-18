//! Session management commands.

use anyhow::Result;

use crate::payment::session::close_session_from_record;
use crate::payment::session_store;

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
        let cumulative: f64 = session.cumulative_amount_u128().unwrap_or(0) as f64 / 1e6;
        let deposit: f64 = session.deposit_u128().unwrap_or(0) as f64 / 1e6;

        println!("  Origin:     {}", session.origin);
        println!("  Network:    {}", session.network_name);
        println!("  Channel:    {}", session.channel_id);
        println!("  Spent:      {cumulative:.6} / {deposit:.6} pathUSD");
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
        for session in &sessions {
            let key = session_store::session_key(&session.origin);
            eprintln!("Closing session for {}...", session.origin);
            let _ = close_session_from_record(session).await;
            let _ = session_store::delete_session(&key);
            closed += 1;
        }

        println!("Closed {closed} session(s).");
        return Ok(());
    }

    if let Some(ref url) = url {
        let key = session_store::session_key(url);
        let session = session_store::load_session(&key)?;

        if let Some(record) = session {
            eprintln!("Closing session for {url}...");
            let _ = close_session_from_record(&record).await;
            let _ = session_store::delete_session(&key);
            println!("Session closed.");
        } else {
            println!("No active session for {url}");
        }
        return Ok(());
    }

    anyhow::bail!("Specify a URL or use --all to close all sessions");
}
