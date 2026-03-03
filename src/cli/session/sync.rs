use anyhow::Result;

use super::super::OutputFormat;
use crate::config::Config;
use crate::payment::session::query_channel_state;
use crate::payment::session::store as session_store;

/// Reconcile local session records with on-chain state.
///
/// For each local session, queries the channel on-chain. If the channel is
/// settled or no longer exists, the local record is removed.
pub async fn sync_sessions(
    config: &Config,
    output_format: OutputFormat,
    show_output: bool,
) -> Result<()> {
    let sessions = session_store::list_sessions()?;

    if sessions.is_empty() {
        if output_format.is_structured() {
            println!(
                "{}",
                output_format.serialize(&serde_json::json!({
                    "synced": 0,
                    "removed": 0,
                }))?
            );
        } else {
            println!("No sessions to sync.");
        }
        return Ok(());
    }

    let mut removed = 0;

    for session in &sessions {
        let state = query_channel_state(config, &session.channel_id, &session.network_name).await;

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
            // pending_closes removed — no cleanup needed
            removed += 1;
        }
    }

    let total = sessions.len();
    if output_format.is_structured() {
        println!(
            "{}",
            output_format.serialize(&serde_json::json!({
                "synced": total,
                "removed": removed,
            }))?
        );
    } else if removed > 0 {
        println!("Synced {total} session(s), removed {removed} stale.");
    } else {
        println!("All {total} session(s) are up to date.");
    }

    Ok(())
}
