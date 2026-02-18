//! Session-based payment handling for the CLI
//!
//! This module handles session payments (intent="session") using presto's
//! keychain-aware transaction building. Sessions open a payment channel
//! on-chain and then exchange off-chain vouchers for each request or SSE
//! token, settling on-chain when the session is closed.
//!
//! Sessions are persisted across CLI invocations via `session_store`. A
//! returning request to the same origin will reuse an existing channel
//! (skipping the on-chain open) and simply increment the cumulative
//! voucher amount.
//!
//! Unlike the mpp `TempoSessionProvider` (which only supports direct EOA
//! signing), this implementation uses presto's transaction builder to
//! support smart wallet / access key (keychain) signing mode.

mod channel;
mod persist;
mod sse;
mod types;
mod voucher;

pub use types::SessionResult;

use alloy::primitives::{Address, B256};
use anyhow::{Context, Result};
use std::str::FromStr;

use mpp::protocol::methods::tempo::session::{SessionCredentialPayload, TempoSessionExt};
use mpp::protocol::methods::tempo::{compute_channel_id, sign_voucher};
use mpp::{parse_receipt, parse_www_authenticate, ChallengeEcho};

use crate::config::Config;
use crate::http::request::RequestContext;
use crate::network::Network;
use crate::payment::mpp_ext::{
    extract_tx_hash, network_from_session_request, validate_session_challenge,
};
use crate::payment::providers::tempo::create_tempo_payment_from_calls;
use crate::payment::session_store;
use crate::wallet::signer::load_signer_with_priority;

use channel::{build_open_calls, extract_origin, send_session_request};
use persist::persist_session;
use types::{SessionContext, SessionState};

/// Handle an MPP session flow (402 with intent="session").
///
/// This manages the session lifecycle with persistence:
/// 1. Parse the session challenge from the initial 402 response
/// 2. Check for an existing persisted session for this origin
/// 3. If found and not expired, reuse it (skip channel open)
/// 4. If not found or expired, open a new channel on-chain
/// 5. Send the real request with a voucher
/// 6. Stream SSE events (or return buffered response)
/// 7. Persist/update the session (do NOT close the channel)
pub async fn handle_session_request(
    config: &Config,
    request_ctx: &RequestContext,
    url: &str,
    initial_response: &crate::http::HttpResponse,
) -> Result<SessionResult> {
    let www_auth = initial_response
        .get_header("www-authenticate")
        .ok_or_else(|| crate::error::PrestoError::MissingHeader("WWW-Authenticate".to_string()))?;

    let challenge =
        parse_www_authenticate(www_auth).context("Failed to parse WWW-Authenticate header")?;

    validate_session_challenge(&challenge)?;

    let session_req: mpp::SessionRequest = challenge
        .request
        .decode()
        .context("Failed to parse session request from challenge")?;

    let network = network_from_session_request(&session_req)?;
    let network_name = network.as_str();

    let tick_cost: u128 = session_req
        .amount
        .parse()
        .context("Invalid session amount")?;

    let escrow_str = session_req
        .escrow_contract()
        .context("Missing escrow contract in session challenge")?;
    let escrow_contract: Address = escrow_str
        .parse()
        .context("Invalid escrow contract address")?;

    let chain_id = session_req
        .chain_id()
        .context("Missing chain ID in session challenge")?;

    let currency: Address = session_req
        .currency
        .parse()
        .context("Invalid currency address")?;

    let recipient: Address = session_req
        .recipient
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Missing recipient in session challenge"))?
        .parse()
        .context("Invalid recipient address")?;

    if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
        eprintln!("Session challenge ID: {}", challenge.id);
        eprintln!("Payment method: {}", challenge.method);
        eprintln!("Network: {}", network_name);
        eprintln!(
            "Cost per {}: {} atomic units",
            session_req.unit_type, tick_cost
        );
    }

    // Load signer for channel operations
    let signer_ctx = load_signer_with_priority()
        .context("Failed to load wallet. Run 'presto login' to get started.")?;

    let key_address = signer_ctx.signer.address();
    let wallet_address = signer_ctx
        .wallet_address
        .as_ref()
        .map(|addr| Address::from_str(addr))
        .transpose()
        .context("Invalid wallet address")?;

    let from = wallet_address.unwrap_or(key_address);

    // Always refresh the challenge echo from the current 402 response
    let echo = challenge.to_echo();
    let origin = extract_origin(url);
    let session_key = session_store::session_key(url);

    // Determine deposit: use suggested_deposit or default to 1 pathUSD
    let deposit: u128 = session_req
        .suggested_deposit
        .as_ref()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1_000_000);

    let did = format!("did:pkh:eip155:{}:{:#x}", chain_id, from);

    // Check for an existing persisted session
    let existing = session_store::load_session(&session_key)?;
    let reuse = existing
        .as_ref()
        .is_some_and(|r| !r.is_expired() && r.payer == did);

    if reuse {
        let record = existing.unwrap();
        if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
            eprintln!("Reusing existing session for {}", origin);
            eprintln!("  Channel: {}", record.channel_id);
        }

        let channel_id: B256 = record.channel_id_b256()?;

        let prev_cumulative: u128 = record.cumulative_amount_u128()?;

        let mut state = SessionState {
            channel_id,
            escrow_contract,
            chain_id,
            cumulative_amount: prev_cumulative + tick_cost,
        };

        let ctx = SessionContext {
            signer: &signer_ctx.signer,
            echo: &echo,
            did: &did,
            request_ctx,
            url,
            network_name,
            origin: &origin,
            tick_cost,
            deposit,
            salt: record.salt.clone(),
            recipient: format!("{:#x}", recipient),
            currency: format!("{:#x}", currency),
        };

        match send_session_request(&ctx, &mut state).await {
            Ok(result) => {
                persist_session(&ctx, &state)?;
                return Ok(result);
            }
            Err(e) => {
                // If the server rejected us (stale session), delete and fall through
                if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
                    eprintln!("Session reuse failed: {e}");
                    eprintln!("Opening new channel...");
                }
                session_store::delete_session(&session_key)?;
                // Fall through to open a new channel
            }
        }
    } else if let Some(ref record) = existing {
        // Expired or different payer — clean up
        if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
            if record.is_expired() {
                eprintln!("Existing session expired, opening new channel...");
            } else {
                eprintln!("Existing session for different payer, opening new channel...");
            }
        }
        session_store::delete_session(&session_key)?;
    }

    // === Open a new channel ===

    let salt = B256::random();
    let authorized_signer = key_address;
    let channel_id = compute_channel_id(
        from,
        recipient,
        currency,
        salt,
        authorized_signer,
        escrow_contract,
        chain_id,
    );

    if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
        eprintln!("Opening payment channel...");
        eprintln!("  Deposit: {} atomic units", deposit);
        eprintln!("  Channel: {:#x}", channel_id);
    }

    let open_calls = build_open_calls(
        currency,
        escrow_contract,
        deposit,
        recipient,
        salt,
        authorized_signer,
    );

    let initial_cumulative = tick_cost;
    let voucher_sig = sign_voucher(
        &signer_ctx.signer,
        channel_id,
        initial_cumulative,
        escrow_contract,
        chain_id,
    )
    .await
    .context("Failed to sign initial voucher")?;

    let open_credential =
        create_tempo_payment_from_calls(config, &challenge, open_calls, currency).await?;

    let open_tx = open_credential
        .payload
        .get("signature")
        .or_else(|| open_credential.payload.get("transaction"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing transaction in open credential"))?
        .to_string();

    let open_payload = SessionCredentialPayload::Open {
        payload_type: "transaction".to_string(),
        channel_id: format!("{}", channel_id),
        transaction: open_tx,
        authorized_signer: Some(format!("{:#x}", authorized_signer)),
        cumulative_amount: initial_cumulative.to_string(),
        signature: format!("0x{}", hex::encode(&voucher_sig)),
    };

    let session_credential =
        mpp::PaymentCredential::with_source(echo.clone(), did.clone(), open_payload);

    let auth_header = mpp::format_authorization(&session_credential)
        .context("Failed to format open credential")?;

    let open_headers = vec![("Authorization".to_string(), auth_header)];
    let open_response = request_ctx.execute(url, Some(&open_headers)).await?;

    if open_response.status_code >= 400 {
        let body = open_response.body_string().unwrap_or_default();
        anyhow::bail!(
            "Session open failed: HTTP {} — {}",
            open_response.status_code,
            body.chars().take(500).collect::<String>()
        );
    }

    if let Some(receipt_str) = open_response.get_header("payment-receipt") {
        if let Ok(receipt) = parse_receipt(receipt_str) {
            let tx_ref = extract_tx_hash(receipt_str).unwrap_or(receipt.reference);
            let explorer = Network::from_str(network_name)
                .ok()
                .and_then(|n| n.info().explorer);
            if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
                if let Some(exp) = explorer.as_ref() {
                    let tx_url = exp.tx_url(&tx_ref);
                    eprintln!("Channel open tx: {}", tx_url);
                } else {
                    eprintln!("Channel open tx: {}", tx_ref);
                }
            }
        }
    }

    let mut state = SessionState {
        channel_id,
        escrow_contract,
        chain_id,
        cumulative_amount: initial_cumulative,
    };

    let ctx = SessionContext {
        signer: &signer_ctx.signer,
        echo: &echo,
        did: &did,
        request_ctx,
        url,
        network_name,
        origin: &origin,
        tick_cost,
        deposit,
        salt: format!("{}", salt),
        recipient: format!("{:#x}", recipient),
        currency: format!("{:#x}", currency),
    };

    let result = send_session_request(&ctx, &mut state).await?;
    persist_session(&ctx, &state)?;
    Ok(result)
}

/// Close a session from a persisted record.
///
/// Used by `presto session close` to send a close credential to the server.
pub async fn close_session_from_record(record: &session_store::SessionRecord) -> Result<()> {
    let echo: ChallengeEcho = serde_json::from_str(&record.challenge_echo)
        .context("Failed to parse persisted challenge echo")?;

    let signer_ctx = load_signer_with_priority()
        .context("Failed to load wallet. Run 'presto login' to get started.")?;

    let channel_id: B256 = record.channel_id_b256()?;

    let escrow_contract: Address = record
        .escrow_contract
        .parse()
        .context("Invalid escrow_contract in session record")?;

    let cumulative_amount: u128 = record.cumulative_amount_u128()?;

    let sig = sign_voucher(
        &signer_ctx.signer,
        channel_id,
        cumulative_amount,
        escrow_contract,
        record.chain_id,
    )
    .await
    .context("Failed to sign close voucher")?;

    let close_payload = SessionCredentialPayload::Close {
        channel_id: format!("{}", channel_id),
        cumulative_amount: cumulative_amount.to_string(),
        signature: format!("0x{}", hex::encode(&sig)),
    };

    let credential = mpp::PaymentCredential::with_source(echo, record.did.clone(), close_payload);

    let auth =
        mpp::format_authorization(&credential).context("Failed to format close credential")?;

    // Build a minimal HTTP client
    let client = reqwest::Client::builder()
        .build()
        .context("Failed to build HTTP client")?;

    let close_url = if record.request_url.is_empty() {
        format!(
            "{}://{}",
            if record.origin.starts_with("https") {
                "https"
            } else {
                "http"
            },
            record.origin.split("://").nth(1).unwrap_or(&record.origin),
        )
    } else {
        record.request_url.clone()
    };

    match client
        .post(&close_url)
        .header("Authorization", &auth)
        .send()
        .await
    {
        Ok(response) => {
            if let Some(receipt_str) = response.headers().get("payment-receipt") {
                if let Ok(receipt_str) = receipt_str.to_str() {
                    if let Ok(receipt) = parse_receipt(receipt_str) {
                        let tx_ref = extract_tx_hash(receipt_str).unwrap_or(receipt.reference);
                        let explorer = Network::from_str(&record.network_name)
                            .ok()
                            .and_then(|n| n.info().explorer);
                        if let Some(exp) = explorer.as_ref() {
                            let tx_url = exp.tx_url(&tx_ref);
                            eprintln!("Channel settled: {}", tx_url);
                        } else {
                            eprintln!("Channel settled: {}", tx_ref);
                        }
                    }
                }
            } else {
                eprintln!("Channel close sent (no receipt)");
            }
        }
        Err(e) => {
            eprintln!("Channel close failed: {}", e);
        }
    }

    Ok(())
}
