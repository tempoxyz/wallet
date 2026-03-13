//! On-chain channel close (payer-initiated `requestClose` → `withdraw`).

use alloy::primitives::{Address, Bytes, TxKind, B256, U256};
use alloy::sol_types::SolCall;
use tempo_primitives::transaction::Call;

use super::super::channel::{get_channel_on_chain, read_grace_period, IEscrow};
use super::super::store as session_store;
use super::super::store::SessionStatus;
use super::super::tx::submit_tempo_tx;
use super::super::DEFAULT_GRACE_PERIOD_SECS;
use super::CloseOutcome;
use crate::config::Config;
use crate::error::{InputError, PaymentError, TempoError};
use crate::keys::{Keystore, Signer};
use crate::network::NetworkId;

type SessionResult<T> = Result<T, TempoError>;

/// Submit requestClose() or withdraw() directly on-chain as a Tempo type-0x76 transaction.
///
/// The escrow contract's payer-initiated close is a two-step process:
/// 1. `requestClose(channelId)` — starts a 15-minute grace period
/// 2. `withdraw(channelId)` — after the grace period, refunds deposit minus settled
///
/// This path works regardless of the authorized signer, since only the payer
/// wallet is required. No voucher signature is needed.
///
/// This function checks the channel's `closeRequestedAt` timestamp:
/// - If 0: submits `requestClose()` and returns `Pending`
/// - If non-zero and grace period elapsed: submits `withdraw()` and returns `Closed`
/// - If non-zero but grace period not elapsed: returns `Pending`
pub(super) async fn close_on_chain(
    config: &Config,
    wallet: &Signer,
    channel_id: B256,
    escrow_contract: Address,
    chain_id: u64,
    fee_token: Address,
) -> SessionResult<CloseOutcome> {
    let network_id = NetworkId::require_chain_id(chain_id)?;
    let rpc_url = config.rpc_url(network_id);
    let provider = alloy::providers::RootProvider::new_http(rpc_url.clone());
    let tempo_provider =
        alloy::providers::RootProvider::<mpp::client::TempoNetwork>::new_http(rpc_url);

    // Check current channel state to determine which step we're on
    let on_chain = match get_channel_on_chain(&provider, escrow_contract, channel_id).await? {
        Some(channel) => channel,
        None => {
            return Ok(CloseOutcome::Closed {
                tx_url: None,
                amount_display: None,
            })
        }
    };

    let from = wallet.from;

    // If closeRequestedAt is 0, we need to call requestClose() first
    if on_chain.close_requested_at == 0 {
        let request_close_data = Bytes::from(
            IEscrow::requestCloseCall {
                channelId: channel_id,
            }
            .abi_encode(),
        );

        let calls = vec![Call {
            to: TxKind::Call(escrow_contract),
            value: U256::ZERO,
            input: request_close_data,
        }];

        let tx_hash =
            submit_tempo_tx(&tempo_provider, wallet, chain_id, fee_token, from, calls).await?;

        let tx_url = network_id.tx_url(&tx_hash);
        tracing::info!("requestClose TX: {}", tx_url);

        let grace_secs = read_grace_period(&provider, escrow_contract)
            .await
            .unwrap_or(DEFAULT_GRACE_PERIOD_SECS);
        let now = session_store::now_secs();
        let ready_at = now + grace_secs;

        // Update local session state if present
        let _ = session_store::update_session_close_state_by_channel_id(
            channel_id,
            SessionStatus::Closing,
            now,
            ready_at,
        );

        return Ok(CloseOutcome::Pending {
            remaining_secs: grace_secs,
        });
    }

    // closeRequestedAt is non-zero — check if grace period has elapsed
    let grace_period = read_grace_period(&provider, escrow_contract)
        .await
        .unwrap_or(DEFAULT_GRACE_PERIOD_SECS);
    let now = session_store::now_secs();
    let ready_at = on_chain.close_requested_at + grace_period;
    if now < ready_at {
        let remaining = ready_at - now;

        // Ensure pending close is persisted so `session list` can show the countdown
        // Update local session state if present
        let _ = session_store::update_session_close_state_by_channel_id(
            channel_id,
            SessionStatus::Closing,
            on_chain.close_requested_at,
            ready_at,
        );

        return Ok(CloseOutcome::Pending {
            remaining_secs: remaining,
        });
    }

    // Grace period elapsed — submit withdraw() to reclaim deposit
    let withdraw_data = Bytes::from(
        IEscrow::withdrawCall {
            channelId: channel_id,
        }
        .abi_encode(),
    );

    let calls = vec![Call {
        to: TxKind::Call(escrow_contract),
        value: U256::ZERO,
        input: withdraw_data,
    }];

    let tx_hash =
        submit_tempo_tx(&tempo_provider, wallet, chain_id, fee_token, from, calls).await?;

    let tx_url = network_id.tx_url(&tx_hash);
    tracing::info!("withdraw TX: {}", tx_url);

    // Best-effort local cleanup is handled by callers, but mark state finalizable->finalized if present
    let _ = session_store::update_session_close_state_by_channel_id(
        channel_id,
        SessionStatus::Finalizable,
        on_chain.close_requested_at,
        now,
    );

    Ok(CloseOutcome::Closed {
        tx_url: Some(tx_url),
        amount_display: None,
    })
}

/// Close a discovered on-chain channel directly, without a server.
///
/// Uses the payer-initiated path (`requestClose` → `withdraw`) which works
/// regardless of whether the current key matches the channel's
/// `authorizedSigner`. This allows closing orphaned channels after key
/// rotation or expiry.
pub async fn close_discovered_channel(
    channel: &super::super::channel::DiscoveredChannel,
    config: &Config,
    keys: &Keystore,
) -> SessionResult<CloseOutcome> {
    let network_id = channel.network;
    let wallet = keys.signer(network_id)?;

    close_on_chain(
        config,
        &wallet,
        channel.channel_id,
        channel.escrow_contract,
        network_id.chain_id(),
        channel.currency,
    )
    .await
}

/// Close a channel by its on-chain ID.
///
/// Uses the payer-initiated path (`requestClose` → `withdraw`) which works
/// regardless of the channel's authorized signer. This allows closing
/// orphaned channels after key rotation or expiry.
pub async fn close_channel_by_id(
    config: &Config,
    channel_id_hex: &str,
    network: NetworkId,
    wallet_override: Option<&Signer>,
    keys: &Keystore,
) -> SessionResult<CloseOutcome> {
    let channel_id: B256 =
        channel_id_hex
            .parse()
            .map_err(|_| InputError::InvalidChannelIdValue {
                value: channel_id_hex.to_string(),
            })?;

    let rpc_url = config.rpc_url(network);
    let provider = alloy::providers::RootProvider::new_http(rpc_url);

    let escrow = network.escrow_contract();

    let on_chain = get_channel_on_chain(&provider, escrow, channel_id)
        .await?
        .ok_or_else(|| PaymentError::ChannelNotFound {
            channel_id: channel_id_hex.to_string(),
            network: network.as_str().to_string(),
        })?;

    let owned_wallet;
    let wallet = match wallet_override {
        Some(w) => w,
        None => {
            owned_wallet = keys.signer(network)?;
            &owned_wallet
        }
    };

    close_on_chain(
        config,
        wallet,
        channel_id,
        escrow,
        network.chain_id(),
        on_chain.currency,
    )
    .await
}
