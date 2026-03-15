//! Session persistence helper for request-time session flows.

use tempo_common::error::{PaymentError, TempoError};

use tempo_common::payment::session::{
    load_session, now_secs, save_session, session_key, SessionRecord, SessionStatus,
};

use super::{SessionContext, SessionState};

/// Persist or update the session record to disk.
pub(super) fn persist_session(
    ctx: &SessionContext<'_>,
    state: &SessionState,
) -> Result<(), TempoError> {
    let now = now_secs();

    let echo_json = serde_json::to_string(ctx.echo).map_err(|source| {
        PaymentError::SessionPersistenceSource {
            operation: "serialize challenge echo",
            source: Box::new(source),
        }
    })?;

    let session_key = session_key(ctx.url);
    let existing = load_session(&session_key)
        .map_err(|source| PaymentError::SessionPersistenceSource {
            operation: "load session",
            source: Box::new(source),
        })?
        .filter(|r| r.channel_id == state.channel_id);

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
            chain_id: state.chain_id,
            escrow_contract: state.escrow_contract,
            currency: format!("{:#x}", ctx.currency),
            recipient: format!("{:#x}", ctx.recipient),
            payer: ctx.did.to_string(),
            authorized_signer: ctx.signer.address(),
            salt: ctx.salt.clone(),
            channel_id: state.channel_id,
            deposit: ctx.deposit,
            tick_cost: ctx.tick_cost,
            cumulative_amount: state.cumulative_amount,
            challenge_echo: echo_json,
            state: SessionStatus::Active,
            close_requested_at: 0,
            grace_ready_at: 0,
            created_at: now,
            last_used_at: now,
        }
    };

    save_session(&record).map_err(|source| PaymentError::SessionPersistenceSource {
        operation: "save session",
        source: Box::new(source),
    })?;

    if ctx.http.log_enabled() {
        let cumulative_display =
            tempo_common::cli::format::format_token_amount(state.cumulative_amount, ctx.network_id);
        eprintln!("Session persisted (cumulative: {cumulative_display})");
    }

    Ok(())
}
