//! Session persistence helper for request-time session flows.

use tempo_common::error::{PaymentError, TempoError};

use tempo_common::payment::session::{
    load_channel, now_secs, save_channel, ChannelRecord, ChannelStatus,
};

use super::{ChannelContext, ChannelState};

/// Persist or update the session record to disk.
pub(super) fn persist_session(
    ctx: &ChannelContext<'_>,
    state: &ChannelState,
) -> Result<(), TempoError> {
    let now = now_secs();

    let echo_json = serde_json::to_string(ctx.echo).map_err(|source| {
        PaymentError::ChannelPersistenceSource {
            operation: "serialize challenge echo",
            source: Box::new(source),
        }
    })?;

    let existing = load_channel(&format!("{:#x}", state.channel_id)).map_err(|source| {
        PaymentError::ChannelPersistenceSource {
            operation: "load channel",
            source: Box::new(source),
        }
    })?;

    let record = if let Some(mut rec) = existing {
        // Update existing record
        rec.set_cumulative_amount(state.cumulative_amount);
        rec.deposit = ctx.deposit;
        rec.challenge_echo = echo_json;
        rec.touch();
        rec
    } else {
        ChannelRecord {
            version: 1,
            origin: ctx.origin.to_string(),
            request_url: ctx.url.to_string(),
            chain_id: state.chain_id,
            escrow_contract: state.escrow_contract,
            token: format!("{:#x}", ctx.token),
            payee: format!("{:#x}", ctx.payee),
            payer: format!("{:#x}", ctx.payer),
            authorized_signer: ctx.signer.address(),
            salt: ctx.salt.clone(),
            channel_id: state.channel_id,
            deposit: ctx.deposit,
            cumulative_amount: state.cumulative_amount,
            challenge_echo: echo_json,
            state: ChannelStatus::Active,
            close_requested_at: 0,
            grace_ready_at: 0,
            created_at: now,
            last_used_at: now,
        }
    };

    save_channel(&record).map_err(|source| PaymentError::ChannelPersistenceSource {
        operation: "save channel",
        source: Box::new(source),
    })?;

    if ctx.http.log_enabled() {
        let cumulative_display =
            tempo_common::cli::format::format_token_amount(state.cumulative_amount, ctx.network_id);
        eprintln!("Channel persisted (cumulative: {cumulative_display})");
    }

    Ok(())
}
