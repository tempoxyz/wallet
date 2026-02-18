use anyhow::{Context, Result};

use crate::payment::session_store::{self, SessionRecord, SESSION_TTL_SECS};

use super::types::{SessionContext, SessionState};

/// Persist or update the session record to disk.
pub(super) fn persist_session(ctx: &SessionContext<'_>, state: &SessionState) -> Result<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let echo_json =
        serde_json::to_string(ctx.echo).context("Failed to serialize challenge echo")?;

    let session_key = session_store::session_key(ctx.url);
    let _lock = session_store::lock_session(&session_key)?;
    let existing = session_store::load_session(&session_key)?;

    let record = if let Some(mut rec) = existing {
        // Update existing record
        rec.set_cumulative_amount(state.cumulative_amount);
        rec.challenge_echo = echo_json;
        rec.touch();
        rec
    } else {
        SessionRecord {
            version: 1,
            origin: ctx.origin.to_string(),
            request_url: ctx.url.to_string(),
            network_name: ctx.network_name.to_string(),
            chain_id: state.chain_id,
            escrow_contract: format!("{:#x}", state.escrow_contract),
            currency: ctx.currency.clone(),
            recipient: ctx.recipient.clone(),
            payer: ctx.did.to_string(),
            authorized_signer: format!("{:#x}", ctx.signer.address()),
            salt: ctx.salt.clone(),
            channel_id: format!("{}", state.channel_id),
            deposit: ctx.deposit.to_string(),
            tick_cost: ctx.tick_cost.to_string(),
            cumulative_amount: state.cumulative_amount.to_string(),
            did: ctx.did.to_string(),
            challenge_echo: echo_json,
            challenge_id: ctx.echo.id.clone(),
            created_at: now,
            last_used_at: now,
            expires_at: now + SESSION_TTL_SECS,
        }
    };

    session_store::save_session(&record)?;

    if ctx.request_ctx.cli.is_verbose() && ctx.request_ctx.cli.should_show_output() {
        let cumulative_f64 = state.cumulative_amount as f64 / 1e6;
        let symbol = ctx.token_symbol();
        eprintln!("Session persisted (cumulative: {cumulative_f64:.6} {symbol})");
    }

    Ok(())
}
