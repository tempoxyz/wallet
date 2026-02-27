//! Channel close operations.
//!
//! Handles closing payment channels via cooperative server close,
//! payer-initiated on-chain close (requestClose → withdraw), and
//! direct channel-by-ID close.

use alloy::primitives::{Address, Bytes, TxKind, B256, U256};
use alloy::sol;
use alloy::sol_types::SolCall;
use anyhow::{Context, Result};
use tempo_primitives::transaction::Call;

use mpp::protocol::core::extract_tx_hash;
use mpp::protocol::methods::tempo::session::SessionCredentialPayload;
use mpp::protocol::methods::tempo::sign_voucher;
use mpp::{parse_receipt, ChallengeEcho};

use super::channel::{get_channel_on_chain, read_grace_period};
use super::store as session_store;
use super::tx::submit_tempo_tx;
use super::CloseOutcome;
use crate::config::Config;
use crate::network::Network;
use crate::wallet::signer::{load_wallet_signer, WalletSigner};

sol! {
    interface IEscrow {
        function requestClose(bytes32 channelId) external;
        function withdraw(bytes32 channelId) external;
    }
}

/// Close a session from a persisted record.
///
/// Used by `presto session close` to send a close credential to the server.
/// Tries cooperative (server-side) close first, then falls back to on-chain close.
pub async fn close_session_from_record(
    record: &session_store::SessionRecord,
    config: &Config,
    nonce_offset: u64,
) -> Result<CloseOutcome> {
    let echo: ChallengeEcho = serde_json::from_str(&record.challenge_echo)
        .context("Failed to parse persisted challenge echo")?;

    let wallet = load_wallet_signer(&record.network_name)?;

    let channel_id: B256 = record.channel_id_b256()?;

    let escrow_contract: Address = record
        .escrow_contract
        .parse()
        .context("Invalid escrow_contract in session record")?;

    let cumulative_amount: u128 = record.cumulative_amount_u128()?;

    let sig = sign_voucher(
        &wallet.signer,
        channel_id,
        cumulative_amount,
        escrow_contract,
        record.chain_id,
    )
    .await
    .context("Failed to sign close voucher")?;

    // Try cooperative close via the server first
    if try_server_close(record, &echo, channel_id, cumulative_amount, &sig)
        .await
        .is_ok()
    {
        return Ok(CloseOutcome::Closed);
    }

    let fee_token: Address = record
        .currency
        .parse()
        .context("Invalid currency address in session record")?;

    // Fallback: payer-initiated close (requestClose → withdraw)
    close_on_chain(
        config,
        &wallet,
        channel_id,
        escrow_contract,
        record.chain_id,
        fee_token,
        nonce_offset,
    )
    .await
}

/// Try cooperative close via the server.
async fn try_server_close(
    record: &session_store::SessionRecord,
    echo: &ChallengeEcho,
    channel_id: B256,
    cumulative_amount: u128,
    sig: &[u8],
) -> Result<()> {
    let close_payload = SessionCredentialPayload::Close {
        channel_id: format!("{}", channel_id),
        cumulative_amount: cumulative_amount.to_string(),
        signature: format!("0x{}", hex::encode(sig)),
    };

    let credential =
        mpp::PaymentCredential::with_source(echo.clone(), record.did.clone(), close_payload);

    let auth =
        mpp::format_authorization(&credential).context("Failed to format close credential")?;

    let close_url = if record.request_url.is_empty() {
        &record.origin
    } else {
        &record.request_url
    };

    let response = reqwest::Client::new()
        .post(close_url)
        .header("Authorization", &auth)
        .send()
        .await
        .context("Channel close request failed")?;

    let status = response.status();

    // HTTP 410 Gone means the channel is already finalized on-chain.
    // Treat this as a successful close — the local record just needs cleanup.
    if status == reqwest::StatusCode::GONE {
        return Ok(());
    }

    if status.is_client_error() || status.is_server_error() {
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| String::from("<no body>"));
        let reason = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| {
                v.get("error")
                    .or_else(|| v.get("message"))
                    .or_else(|| v.get("detail"))
                    .and_then(|s| s.as_str().map(String::from))
            })
            .unwrap_or_else(|| body.chars().take(200).collect());
        anyhow::bail!(
            "Channel close rejected: HTTP {} — {}",
            status.as_u16(),
            reason
        );
    }

    if let Some(receipt_str) = response.headers().get("payment-receipt") {
        if let Ok(receipt_str) = receipt_str.to_str() {
            if let Ok(receipt) = parse_receipt(receipt_str) {
                let tx_ref = extract_tx_hash(receipt_str).unwrap_or(receipt.reference);
                let explorer = record
                    .network_name
                    .parse::<Network>()
                    .ok()
                    .and_then(|n| n.info().explorer);
                if let Some(exp) = explorer.as_ref() {
                    eprintln!("Channel settled: {}", exp.tx_url(&tx_ref));
                } else {
                    eprintln!("Channel settled: {}", tx_ref);
                }
            }
        }
    } else {
        eprintln!("Channel close sent (no receipt)");
    }

    Ok(())
}

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
    wallet: &WalletSigner,
    channel_id: B256,
    escrow_contract: Address,
    chain_id: u64,
    fee_token: Address,
    nonce_offset: u64,
) -> Result<CloseOutcome> {
    let network = Network::require_chain_id(chain_id)?;
    let network_name = network.as_str();
    let network_info = config.resolve_network(network_name)?;
    let rpc_url = Network::parse_rpc_url(&network_info.rpc_url)?;
    let provider = alloy::providers::RootProvider::new_http(rpc_url.clone());
    let tempo_provider =
        alloy::providers::RootProvider::<mpp::client::TempoNetwork>::new_http(rpc_url);

    // Check current channel state to determine which step we're on
    let on_chain = get_channel_on_chain(&provider, escrow_contract, channel_id, B256::ZERO)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Channel no longer exists on-chain"))?;

    let from = wallet.from;
    let channel_id_hex = format!("{:#x}", channel_id);

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

        let tx_hash = submit_tempo_tx(
            &tempo_provider,
            wallet,
            chain_id,
            fee_token,
            from,
            calls,
            nonce_offset,
        )
        .await?;

        let explorer = network.info().explorer;
        let tx_url = explorer
            .as_ref()
            .map(|exp| exp.tx_url(&tx_hash))
            .unwrap_or(tx_hash);
        tracing::info!("requestClose TX: {}", tx_url);

        let grace_secs = read_grace_period(&provider, escrow_contract)
            .await
            .unwrap_or(900);
        let now = session_store::now_secs();
        let ready_at = now + grace_secs;

        if let Err(e) = session_store::save_pending_close(&channel_id_hex, network_name, ready_at) {
            tracing::warn!(%e, "failed to persist pending close for automatic finalization");
        }

        return Ok(CloseOutcome::Pending {
            remaining_secs: grace_secs,
        });
    }

    // closeRequestedAt is non-zero — check if grace period has elapsed
    let grace_period = read_grace_period(&provider, escrow_contract)
        .await
        .unwrap_or(900);
    let now = session_store::now_secs();
    let ready_at = on_chain.close_requested_at as u64 + grace_period;
    if now < ready_at {
        let remaining = ready_at - now;

        // Ensure pending close is persisted so `session list` can show the countdown
        if let Err(e) = session_store::save_pending_close(&channel_id_hex, network_name, ready_at) {
            tracing::warn!(%e, "failed to persist pending close");
        }

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

    let tx_hash = submit_tempo_tx(
        &tempo_provider,
        wallet,
        chain_id,
        fee_token,
        from,
        calls,
        nonce_offset,
    )
    .await?;

    let explorer = network.info().explorer;
    let tx_url = explorer
        .as_ref()
        .map(|exp| exp.tx_url(&tx_hash))
        .unwrap_or(tx_hash);
    tracing::info!("withdraw TX: {}", tx_url);

    Ok(CloseOutcome::Closed)
}

/// Close a discovered on-chain channel directly, without a server.
///
/// Uses the payer-initiated path (`requestClose` → `withdraw`) which works
/// regardless of whether the current key matches the channel's
/// `authorizedSigner`. This allows closing orphaned channels after key
/// rotation or expiry.
pub async fn close_discovered_channel(
    channel: &super::channel::DiscoveredChannel,
    config: &Config,
    nonce_offset: u64,
) -> Result<CloseOutcome> {
    let network: Network = channel
        .network
        .parse()
        .map_err(|_| anyhow::anyhow!("Unknown network: {}", channel.network))?;

    let wallet = load_wallet_signer(network.as_str())?;

    let channel_id: B256 = channel.channel_id.parse().context("Invalid channel_id")?;
    let escrow_contract: Address = channel
        .escrow_contract
        .parse()
        .context("Invalid escrow_contract")?;
    let fee_token: Address = channel.token.parse().context("Invalid token address")?;

    close_on_chain(
        config,
        &wallet,
        channel_id,
        escrow_contract,
        network.chain_id(),
        fee_token,
        nonce_offset,
    )
    .await
}

/// Close a channel by its on-chain ID, scanning all networks to find it.
///
/// Uses the payer-initiated path (`requestClose` → `withdraw`) which works
/// regardless of the channel's authorized signer. This allows closing
/// orphaned channels after key rotation or expiry.
pub async fn close_channel_by_id(
    config: &Config,
    channel_id_hex: &str,
    network_filter: Option<&str>,
    wallet_override: Option<&WalletSigner>,
) -> Result<CloseOutcome> {
    let channel_id: B256 = channel_id_hex
        .parse()
        .context("Invalid channel ID (expected 0x-prefixed bytes32 hex)")?;

    let networks: Vec<Network> = if let Some(name) = network_filter {
        name.parse::<Network>().ok().into_iter().collect()
    } else {
        Network::all().to_vec()
    };

    let mut had_rpc_errors = false;

    for network in &networks {
        let network_info = match config.resolve_network(network.as_str()) {
            Ok(info) => info,
            Err(_) => continue,
        };
        let rpc_url: url::Url = match network_info.rpc_url.parse() {
            Ok(u) => u,
            Err(_) => continue,
        };
        let provider = alloy::providers::RootProvider::new_http(rpc_url);

        let escrow: Address = match network.escrow_contract().parse() {
            Ok(a) => a,
            Err(_) => continue,
        };

        let on_chain = match get_channel_on_chain(&provider, escrow, channel_id, B256::ZERO).await {
            Ok(Some(ch)) => ch,
            Ok(None) => continue,
            Err(e) => {
                tracing::debug!(network = network.as_str(), %e, "failed to query channel");
                had_rpc_errors = true;
                continue;
            }
        };

        let owned_wallet;
        let wallet = match wallet_override {
            Some(w) => w,
            None => {
                owned_wallet = load_wallet_signer(network.as_str())?;
                &owned_wallet
            }
        };

        return close_on_chain(
            config,
            wallet,
            channel_id,
            escrow,
            network.chain_id(),
            on_chain.token,
            0,
        )
        .await;
    }

    if had_rpc_errors {
        anyhow::bail!(
            "Channel {} could not be verified — RPC errors prevented checking all networks",
            channel_id_hex
        )
    } else {
        anyhow::bail!("Channel {} not found on any network", channel_id_hex)
    }
}
