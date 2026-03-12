//! Cooperative (server-side) channel close.
//!
//! Sends a close credential to the server and lets it settle the channel
//! on-chain, avoiding the payer-initiated grace period.

use alloy::primitives::{Address, B256};
use anyhow::{Context, Result};

use mpp::protocol::methods::tempo::session::SessionCredentialPayload;
use mpp::protocol::methods::tempo::sign_voucher;
use mpp::{parse_receipt, ChallengeEcho};

use mpp::protocol::core::extract_tx_hash;

use super::super::store as session_store;
use crate::cli::format::format_token_amount;
use crate::error::PaymentError;

/// Attempt a cooperative (server-side) close of a session without on-chain fallback.
///
/// Used for best-effort cleanup when reusing a session fails — the result is
/// typically discarded because the caller will open a new channel regardless.
#[allow(dead_code)]
async fn try_cooperative_close_from_record(
    record: &session_store::SessionRecord,
    keys: &crate::keys::Keystore,
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

    let client = reqwest::Client::new();
    try_server_close(
        record,
        &echo,
        &wallet.signer,
        channel_id,
        escrow_contract,
        record.chain_id,
        cumulative_amount,
        &client,
    )
    .await
    .map(|_| ())
}

/// Try cooperative close via the server.
///
/// Returns the settlement transaction URL on success (if available).
#[allow(clippy::too_many_arguments)]
pub(super) async fn try_server_close(
    record: &session_store::SessionRecord,
    echo: &ChallengeEcho,
    signer: &alloy::signers::local::PrivateKeySigner,
    channel_id: B256,
    escrow_contract: Address,
    chain_id: u64,
    cumulative_amount: u128,
    client: &reqwest::Client,
) -> Result<Option<String>> {
    let close_url = if record.request_url.is_empty() {
        &record.origin
    } else {
        &record.request_url
    };

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
    if status.is_client_error() || status.is_server_error() {
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| String::from("<no body>"));
        let reason = crate::payment::classify::extract_json_error(&body)
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
            state: session_store::SessionStatus::Active,
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

        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let _ = try_server_close(
            &record, &echo, &signer, channel_id, escrow, 4217, 2, &client,
        )
        .await;

        let (prefetch, authorized) = *counters.lock().unwrap();
        assert_eq!(prefetch, 1, "should prefetch fresh echo with 402");
        assert_eq!(
            authorized, 1,
            "should send exactly one authorized close request"
        );
    }
}
