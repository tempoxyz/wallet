//! Voucher credential construction for session payments.

use alloy::{
    primitives::{Address, B256},
    sol_types::{eip712_domain, SolStruct},
};

use mpp::{
    protocol::methods::tempo::{
        session::SessionCredentialPayload,
        voucher::{Voucher, DOMAIN_NAME, DOMAIN_VERSION},
    },
    ChallengeEcho,
};

use super::ChannelState;
use tempo_common::error::TempoError;

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
        channel_id: format!("{channel_id:#x}"),
        transaction,
        authorized_signer: Some(format!("{authorized_signer:#x}")),
        cumulative_amount: cumulative_amount.to_string(),
        signature: format!("0x{}", hex::encode(voucher_sig)),
    }
}

/// Compute the EIP-712 signing hash for a voucher.
pub(crate) fn voucher_signing_hash(
    channel_id: B256,
    cumulative_amount: u128,
    escrow_contract: Address,
    chain_id: u64,
) -> B256 {
    let domain = eip712_domain! {
        name: DOMAIN_NAME,
        version: DOMAIN_VERSION,
        chain_id: chain_id,
        verifying_contract: escrow_contract,
    };
    let voucher = Voucher {
        channelId: channel_id,
        cumulativeAmount: cumulative_amount,
    };
    voucher.eip712_signing_hash(&domain)
}

/// Build a voucher credential for an existing session.
pub(super) async fn build_voucher_credential(
    signer: &tempo_common::keys::Signer,
    echo: &ChallengeEcho,
    did: &str,
    state: &ChannelState,
) -> Result<mpp::PaymentCredential, TempoError> {
    let hash = voucher_signing_hash(
        state.channel_id,
        state.cumulative_amount,
        state.escrow_contract,
        state.chain_id,
    );
    let sig = signer.sign_voucher_hash(hash)?;

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

/// Build a `SessionCredentialPayload::TopUp` from a signed top-up transaction.
pub(super) fn build_top_up_payload(
    channel_id: B256,
    transaction: String,
    additional_deposit: u128,
) -> SessionCredentialPayload {
    SessionCredentialPayload::TopUp {
        payload_type: "transaction".to_string(),
        channel_id: format!("{channel_id:#x}"),
        transaction,
        additional_deposit: additional_deposit.to_string(),
    }
}
