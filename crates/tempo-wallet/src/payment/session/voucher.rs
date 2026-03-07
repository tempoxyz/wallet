//! Voucher credential construction for session payments.

use alloy::primitives::{Address, B256};
use anyhow::{Context, Result};

use mpp::protocol::methods::tempo::session::SessionCredentialPayload;
use mpp::protocol::methods::tempo::sign_voucher;
use mpp::ChallengeEcho;

use super::state::SessionState;

/// Build a `SessionCredentialPayload::Open` with the given transaction bytes.
pub(super) fn build_open_payload(
    channel_id: B256,
    transaction: String,
    authorized_signer: Address,
    cumulative_amount: u128,
    voucher_sig: &[u8],
) -> SessionCredentialPayload {
    SessionCredentialPayload::Open {
        payload_type: "transaction".to_string(),
        channel_id: format!("{:#x}", channel_id),
        transaction,
        authorized_signer: Some(format!("{:#x}", authorized_signer)),
        cumulative_amount: cumulative_amount.to_string(),
        signature: format!("0x{}", hex::encode(voucher_sig)),
    }
}

/// Build a voucher credential for an existing session.
pub(super) async fn build_voucher_credential(
    signer: &alloy::signers::local::PrivateKeySigner,
    echo: &ChallengeEcho,
    did: &str,
    state: &SessionState,
) -> Result<mpp::PaymentCredential> {
    let sig = sign_voucher(
        signer,
        state.channel_id,
        state.cumulative_amount,
        state.escrow_contract,
        state.chain_id,
    )
    .await
    .context("Failed to sign voucher")?;

    let payload = SessionCredentialPayload::Voucher {
        channel_id: format!("{:#x}", state.channel_id),
        cumulative_amount: state.cumulative_amount.to_string(),
        signature: format!("0x{}", hex::encode(&sig)),
    };

    Ok(mpp::PaymentCredential::with_source(
        echo.clone(),
        did.to_string(),
        payload,
    ))
}
