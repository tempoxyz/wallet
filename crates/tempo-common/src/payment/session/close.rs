//! Channel close operations.
//!
//! Handles closing payment channels via cooperative server close,
//! payer-initiated on-chain close (requestClose → withdraw), and
//! direct channel-by-ID close.

use alloy::primitives::{Address, Bytes, TxKind, B256, U256};
use alloy::sol_types::SolCall;
use anyhow::{Context, Result};
use tempo_primitives::transaction::Call;

use mpp::protocol::core::extract_tx_hash;
use mpp::protocol::methods::tempo::session::SessionCredentialPayload;
use mpp::protocol::methods::tempo::sign_voucher;
use mpp::{parse_receipt, ChallengeEcho};

use super::channel::{get_channel_on_chain, read_grace_period, IEscrow};
use super::store as session_store;
use super::store::SessionStatus;
use super::tx::submit_tempo_tx;
use super::DEFAULT_GRACE_PERIOD_SECS;
use crate::analytics::{Analytics, Event};
use crate::config::Config;
use crate::error::PaymentError;

/// Outcome of an on-chain close attempt.
pub enum CloseOutcome {
    /// Channel fully closed (withdrawn or cooperatively settled).
    Closed {
        tx_url: Option<String>,
        /// Formatted settlement amount (e.g., "0.002 USDC"), if available.
        amount_display: Option<String>,
    },
    /// `requestClose()` submitted or already pending; waiting for grace period.
    Pending { remaining_secs: u64 },
}

use crate::fmt::format_token_amount;
use crate::keys::{Keystore, Signer};
use crate::network::NetworkId;

/// Close a session from a persisted record.
///
/// Used by `tempo-wallet sessions close` to send a close credential to the server.
/// Tries cooperative (server-side) close first, then falls back to on-chain close.
pub async fn close_session_from_record(
    record: &session_store::SessionRecord,
    config: &Config,
    analytics: Option<&Analytics>,
    keys: &Keystore,
) -> Result<CloseOutcome> {
    let echo: ChallengeEcho = serde_json::from_str(&record.challenge_echo)
        .context("Failed to parse persisted challenge echo")?;

    let network_id = record.network_id();
    let wallet = keys.signer(network_id)?;

    let channel_id: B256 = record.channel_id_b256()?;

    let escrow_contract: Address = record
        .escrow_contract
        .parse()
        .context("Invalid escrow_contract in session record")?;

    let cumulative_amount: u128 = record.cumulative_amount_u128()?;

    // Try cooperative close via the server first
    match try_server_close(
        record,
        &echo,
        &wallet.signer,
        channel_id,
        escrow_contract,
        record.chain_id,
        cumulative_amount,
    )
    .await
    {
        Ok(tx_url) => {
            if let Some(a) = analytics {
                a.track(
                    Event::CoopCloseSuccess,
                    crate::analytics::CoopClosePayload {
                        network: network_id.as_str().to_string(),
                        channel_id: record.channel_id.clone(),
                    },
                );
            }
            let amount_display = record
                .cumulative_amount_u128()
                .ok()
                .map(|amt| format_token_amount(amt, network_id));
            return Ok(CloseOutcome::Closed {
                tx_url,
                amount_display,
            });
        }
        Err(coop_err) => {
            if let Some(a) = analytics {
                a.track(
                    Event::CoopCloseFailure,
                    crate::analytics::CoopClosePayload {
                        network: network_id.as_str().to_string(),
                        channel_id: record.channel_id.clone(),
                    },
                );
            }
            tracing::info!("Cooperative close failed: {coop_err:#}")
        }
    }

    let fee_token: Address = record
        .currency
        .parse()
        .context("Invalid currency address in session record")?;

    // Fallback: payer-initiated close (requestClose → withdraw)
    let outcome = close_on_chain(
        config,
        &wallet,
        channel_id,
        escrow_contract,
        record.chain_id,
        fee_token,
    )
    .await?;

    match outcome {
        CloseOutcome::Closed { tx_url, .. } => {
            let amount_display = record
                .cumulative_amount_u128()
                .ok()
                .map(|amt| format_token_amount(amt, network_id));
            Ok(CloseOutcome::Closed {
                tx_url,
                amount_display,
            })
        }
        other => Ok(other),
    }
}

/// Attempt a cooperative (server-side) close of a session without on-chain fallback.
///
/// Used for best-effort cleanup when reusing a session fails — the result is
/// typically discarded because the caller will open a new channel regardless.
pub async fn try_cooperative_close_from_record(
    record: &session_store::SessionRecord,
    keys: &Keystore,
) -> Result<()> {
    let echo: ChallengeEcho = serde_json::from_str(&record.challenge_echo)
        .context("Failed to parse persisted challenge echo")?;

    let network_id = record.network_id();
    let wallet = keys.signer(network_id)?;

    let channel_id: B256 = record.channel_id_b256()?;

    let escrow_contract: Address = record
        .escrow_contract
        .parse()
        .context("Invalid escrow_contract in session record")?;

    let cumulative_amount: u128 = record.cumulative_amount_u128()?;

    try_server_close(
        record,
        &echo,
        &wallet.signer,
        channel_id,
        escrow_contract,
        record.chain_id,
        cumulative_amount,
    )
    .await
    .map(|_| ())
}

/// Try cooperative close via the server.
///
/// Returns the settlement transaction URL on success (if available).
async fn try_server_close(
    record: &session_store::SessionRecord,
    echo: &ChallengeEcho,
    signer: &alloy::signers::local::PrivateKeySigner,
    channel_id: B256,
    escrow_contract: Address,
    chain_id: u64,
    cumulative_amount: u128,
) -> Result<Option<String>> {
    let close_url = if record.request_url.is_empty() {
        &record.origin
    } else {
        &record.request_url
    };

    // Single-shot coop-close with the persisted cumulative (fetch fresh echo first)
    let client = reqwest::Client::new();
    let fresh_echo = match client.post(close_url).send().await {
        Ok(resp) if resp.status().as_u16() == 402 => resp
            .headers()
            .get("www-authenticate")
            .and_then(|v| v.to_str().ok())
            .and_then(|wa| mpp::parse_www_authenticate(wa).ok())
            .map(|ch| ch.to_echo()),
        _ => None,
    };
    let echo = fresh_echo.as_ref().unwrap_or(echo);

    let network_id = record.network_id();
    let spent_fmt = format_token_amount(cumulative_amount, network_id);
    let deposit_u = record.deposit_u128().unwrap_or(0);
    let deposit_fmt = format_token_amount(deposit_u, network_id);
    tracing::info!(
        spent = %spent_fmt,
        deposit = %deposit_fmt,
        channel = %format_args!("{:#x}", channel_id),
        url = close_url,
        "coop close"
    );
    let sig = sign_voucher(
        signer,
        channel_id,
        cumulative_amount,
        escrow_contract,
        chain_id,
    )
    .await
    .context("Failed to sign close voucher")?;
    let payload = SessionCredentialPayload::Close {
        channel_id: format!("{:#x}", channel_id),
        cumulative_amount: cumulative_amount.to_string(),
        signature: format!("0x{}", hex::encode(sig)),
    };
    let credential =
        mpp::PaymentCredential::with_source(echo.clone(), record.payer.to_string(), payload);
    let auth =
        mpp::format_authorization(&credential).context("Failed to format close credential")?;
    let response = client
        .post(close_url)
        .header("Authorization", &auth)
        .send()
        .await
        .context("Channel close request failed")?;

    // Interpret response and optionally retry once with required cumulative
    let status = response.status();
    if status == reqwest::StatusCode::GONE {
        return Ok(None);
    }
    if status.is_client_error() || status.is_server_error() {
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| String::from("<no body>"));
        let reason = crate::payment::error::extract_json_error(&body)
            .unwrap_or_else(|| body.chars().take(200).collect());
        return Err(PaymentError::PaymentRejected {
            reason,
            status_code: status.as_u16(),
        }
        .into());
    }

    let tx_url = response
        .headers()
        .get("payment-receipt")
        .and_then(|v| v.to_str().ok())
        .and_then(|receipt_str| {
            let receipt = parse_receipt(receipt_str).ok()?;
            let tx_ref = extract_tx_hash(receipt_str).unwrap_or(receipt.reference);
            Some(record.network_id().tx_url(&tx_ref))
        });

    Ok(tx_url)
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
async fn close_on_chain(
    config: &Config,
    wallet: &Signer,
    channel_id: B256,
    escrow_contract: Address,
    chain_id: u64,
    fee_token: Address,
) -> Result<CloseOutcome> {
    let network_id = NetworkId::require_chain_id(chain_id)?;
    let rpc_url = config.rpc_url(network_id);
    let provider = alloy::providers::RootProvider::new_http(rpc_url.clone());
    let tempo_provider =
        alloy::providers::RootProvider::<mpp::client::TempoNetwork>::new_http(rpc_url);

    // Check current channel state to determine which step we're on
    let on_chain = get_channel_on_chain(&provider, escrow_contract, channel_id)
        .await?
        .ok_or(PaymentError::InvalidChallenge(
            "Channel no longer exists on-chain".to_string(),
        ))?;

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
            &channel_id_hex,
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
            &channel_id_hex,
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
        &channel_id_hex,
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
    channel: &super::channel::DiscoveredChannel,
    config: &Config,
    keys: &Keystore,
) -> Result<CloseOutcome> {
    let network_id = channel.network;
    let wallet = keys.signer(network_id)?;

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
        network_id.chain_id(),
        fee_token,
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
) -> Result<CloseOutcome> {
    let channel_id: B256 = channel_id_hex
        .parse()
        .context("Invalid channel ID (expected 0x-prefixed bytes32 hex)")?;

    let rpc_url = config.rpc_url(network);
    let provider = alloy::providers::RootProvider::new_http(rpc_url);

    let escrow: Address = network
        .escrow_contract()
        .parse()
        .context("Invalid escrow contract address")?;

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
        on_chain.token,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::post, Router};
    use std::sync::{Arc, Mutex};
    use tokio::task::JoinHandle;

    async fn spawn_test_server() -> (String, Arc<Mutex<(usize, usize)>>, JoinHandle<()>) {
        let counters = Arc::new(Mutex::new((0usize, 0usize)));
        let counters_clone = counters.clone();
        let app = Router::new().route(
            "/",
            post(move |req: axum::http::Request<axum::body::Body>| {
                let counters = counters_clone.clone();
                async move {
                    let has_auth = req.headers().get("authorization").is_some();
                    let mut c = counters.lock().unwrap();
                    if has_auth {
                        c.1 += 1;
                        axum::http::Response::builder()
                            .status(200)
                            .body(axum::body::Body::empty())
                            .unwrap()
                    } else {
                        c.0 += 1;
                        axum::http::Response::builder()
                            .status(402)
                            .header(
                                "www-authenticate",
                                r#"Payment id="abc", realm="test", method="tempo", intent="session", request="e30""#,
                            )
                            .body(axum::body::Body::empty())
                            .unwrap()
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (
            format!("http://{}:{}/", addr.ip(), addr.port()),
            counters,
            handle,
        )
    }

    #[tokio::test]
    async fn test_try_server_close_prefetch_and_single_attempt() {
        let (base, counters, _handle) = spawn_test_server().await;

        // Minimal synthetic record
        let record = session_store::SessionRecord {
            version: 1,
            origin: base.clone(),
            request_url: base.clone(),
            network_name: "tempo".into(),
            chain_id: 4217,
            escrow_contract: "0x0000000000000000000000000000000000000001".into(),
            currency: "0x0000000000000000000000000000000000000001".into(),
            recipient: "0x0000000000000000000000000000000000000002".into(),
            payer: "did:pkh:eip155:4217:0x0000000000000000000000000000000000000003".into(),
            authorized_signer: "0x0000000000000000000000000000000000000003".into(),
            salt: "0x00".into(),
            channel_id: "0x01".into(),
            deposit: "1000".into(),
            tick_cost: "1".into(),
            cumulative_amount: "2".into(),
            challenge_echo: serde_json::to_string(&mpp::ChallengeEcho {
                id: "abc".into(),
                realm: "test".into(),
                method: mpp::protocol::core::MethodName::from("tempo"),
                intent: mpp::protocol::core::IntentName::from("session"),
                request: "e30".into(), // base64url of {}
                expires: None,
                digest: None,
                opaque: None,
            })
            .unwrap(),
            state: SessionStatus::Active,
            close_requested_at: 0,
            grace_ready_at: 0,
            created_at: 0,
            last_used_at: 0,
        };

        let echo: ChallengeEcho = serde_json::from_str(&record.challenge_echo).unwrap();
        let signer = alloy::signers::local::PrivateKeySigner::from_bytes(
            &"0x0707070707070707070707070707070707070707070707070707070707070707"
                .parse()
                .unwrap(),
        )
        .unwrap();
        let channel_id: B256 = "0x0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20"
            .parse()
            .unwrap();
        let escrow: Address = "0x0000000000000000000000000000000000000001"
            .parse()
            .unwrap();

        let _ = try_server_close(&record, &echo, &signer, channel_id, escrow, 4217, 2).await;

        let (prefetch, authorized) = *counters.lock().unwrap();
        assert_eq!(prefetch, 1, "should prefetch fresh echo with 402");
        assert_eq!(
            authorized, 1,
            "should send exactly one authorized close request"
        );
    }
}
