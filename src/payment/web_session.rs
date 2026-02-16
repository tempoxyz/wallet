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

use alloy::primitives::{Address, B256, U256};
use anyhow::{Context, Result};
use futures::StreamExt;
use std::io::Write;
use std::str::FromStr;

use serde_json;

use mpp::protocol::methods::tempo::session::{SessionCredentialPayload, TempoSessionExt};
use mpp::protocol::methods::tempo::{compute_channel_id, sign_voucher};
use mpp::server::sse::{parse_event, SseEvent};
use mpp::{parse_receipt, parse_www_authenticate, ChallengeEcho, SessionRequest};

use crate::config::Config;
use crate::http::request::RequestContext;
use crate::http::HttpResponse;
use crate::network::Network;
use crate::payment::abi::encode_approve;
use crate::payment::mpp_ext::{method_to_network, validate_session_challenge};
use crate::payment::providers::tempo::create_tempo_payment_from_calls;
use crate::payment::session_store::{self, SessionRecord, SESSION_TTL_SECS};
use crate::wallet::signer::load_signer_with_priority;

/// Result of a session request — either streamed (already printed) or a buffered response.
pub enum SessionResult {
    /// SSE tokens were streamed directly to stdout.
    Streamed,
    /// A normal (non-SSE) response that should be handled by the regular output path.
    Response(HttpResponse),
}

/// State for an active session channel.
struct SessionState {
    channel_id: B256,
    escrow_contract: Address,
    chain_id: u64,
    cumulative_amount: u128,
}

/// Shared context for session operations (streaming, closing).
struct SessionContext<'a> {
    signer: &'a alloy::signers::local::PrivateKeySigner,
    echo: &'a ChallengeEcho,
    did: &'a str,
    request_ctx: &'a RequestContext,
    url: &'a str,
    network_name: &'a str,
    origin: &'a str,
    tick_cost: u128,
    deposit: u128,
    salt: String,
    recipient: String,
    currency: String,
}

/// Handle a Web Payment Auth session (402 with intent="session").
///
/// This manages the session lifecycle with persistence:
/// 1. Parse the session challenge from the initial 402 response
/// 2. Check for an existing persisted session for this origin
/// 3. If found and not expired, reuse it (skip channel open)
/// 4. If not found or expired, open a new channel on-chain
/// 5. Send the real request with a voucher
/// 6. Stream SSE events (or return buffered response)
/// 7. Persist/update the session (do NOT close the channel)
pub async fn handle_web_session_request(
    config: &Config,
    request_ctx: &RequestContext,
    url: &str,
    initial_response: &HttpResponse,
) -> Result<SessionResult> {
    let www_auth = initial_response
        .get_header("www-authenticate")
        .ok_or_else(|| anyhow::anyhow!("Missing WWW-Authenticate header in 402 response"))?;

    let challenge =
        parse_www_authenticate(www_auth).context("Failed to parse WWW-Authenticate header")?;

    validate_session_challenge(&challenge)?;

    let network_name = method_to_network(&challenge.method)
        .ok_or_else(|| anyhow::anyhow!("Unsupported payment method: {}", challenge.method))?;

    let session_req: SessionRequest = challenge
        .request
        .decode()
        .context("Failed to parse session request from challenge")?;

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

        let channel_id: B256 = record
            .channel_id
            .parse()
            .context("Invalid channel_id in persisted session")?;

        let prev_cumulative: u128 = record
            .cumulative_amount
            .parse()
            .context("Invalid cumulative_amount in persisted session")?;

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
            let explorer = Network::from_str(network_name)
                .ok()
                .and_then(|n| n.info().explorer);
            if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
                if let Some(exp) = explorer.as_ref() {
                    let tx_url = exp.tx_url(&receipt.reference);
                    eprintln!("Channel open tx: {}", tx_url);
                } else {
                    eprintln!("Channel open tx: {}", receipt.reference);
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

/// Send the actual request with a voucher and handle the response.
async fn send_session_request(
    ctx: &SessionContext<'_>,
    state: &mut SessionState,
) -> Result<SessionResult> {
    if ctx.request_ctx.cli.is_verbose() && ctx.request_ctx.cli.should_show_output() {
        eprintln!("Sending request with session voucher...");
    }

    let voucher_credential = build_voucher_credential(ctx.signer, ctx.echo, ctx.did, state).await?;

    let voucher_auth = mpp::format_authorization(&voucher_credential)
        .context("Failed to format voucher credential")?;

    let reqwest_client = ctx.request_ctx.build_reqwest_client(None)?;
    let data_request = build_reqwest_request(&reqwest_client, ctx.request_ctx, ctx.url)
        .header("Authorization", &voucher_auth);

    let response = data_request
        .send()
        .await
        .context("Failed to send session request")?;

    let status = response.status();
    if status.as_u16() == 402 || status.is_client_error() || status.is_server_error() {
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!(
            "Session request failed: HTTP {} — {}",
            status,
            body.chars().take(500).collect::<String>()
        );
    }

    let is_sse = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.contains("text/event-stream"));

    if is_sse {
        stream_sse_response(ctx, state, response).await?;
        Ok(SessionResult::Streamed)
    } else {
        let status_code = status.as_u16() as u32;
        let mut headers = std::collections::HashMap::new();
        for (key, value) in response.headers() {
            if let Ok(value_str) = value.to_str() {
                headers.insert(key.as_str().to_lowercase(), value_str.to_string());
            }
        }
        let body = response.bytes().await?.to_vec();

        Ok(SessionResult::Response(HttpResponse {
            status_code,
            headers,
            body,
        }))
    }
}

/// Persist or update the session record to disk.
fn persist_session(ctx: &SessionContext<'_>, state: &SessionState) -> Result<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let echo_json =
        serde_json::to_string(ctx.echo).context("Failed to serialize challenge echo")?;

    let session_key = session_store::session_key(ctx.url);
    let existing = session_store::load_session(&session_key)?;

    let record = if let Some(mut rec) = existing {
        // Update existing record
        rec.cumulative_amount = state.cumulative_amount.to_string();
        rec.challenge_echo = echo_json;
        rec.touch();
        rec
    } else {
        SessionRecord {
            version: 1,
            origin: ctx.origin.to_string(),
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
        eprintln!(
            "Session persisted (cumulative: {:.6} pathUSD)",
            cumulative_f64
        );
    }

    Ok(())
}

/// Close a session from a persisted record.
///
/// Used by `presto session close` to send a close credential to the server.
pub async fn close_session_from_record(record: &SessionRecord) -> Result<()> {
    let echo: ChallengeEcho = serde_json::from_str(&record.challenge_echo)
        .context("Failed to parse persisted challenge echo")?;

    let signer_ctx = load_signer_with_priority()
        .context("Failed to load wallet. Run 'presto login' to get started.")?;

    let channel_id: B256 = record
        .channel_id
        .parse()
        .context("Invalid channel_id in session record")?;

    let escrow_contract: Address = record
        .escrow_contract
        .parse()
        .context("Invalid escrow_contract in session record")?;

    let cumulative_amount: u128 = record
        .cumulative_amount
        .parse()
        .context("Invalid cumulative_amount in session record")?;

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

    let close_url = format!(
        "{}://{}",
        if record.origin.starts_with("https") {
            "https"
        } else {
            "http"
        },
        record.origin.split("://").nth(1).unwrap_or(&record.origin),
    );

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
                        let explorer = Network::from_str(&record.network_name)
                            .ok()
                            .and_then(|n| n.info().explorer);
                        if let Some(exp) = explorer.as_ref() {
                            let tx_url = exp.tx_url(&receipt.reference);
                            eprintln!("Channel settled: {}", tx_url);
                        } else {
                            eprintln!("Channel settled: {}", receipt.reference);
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

/// Extract the origin (scheme://host[:port]) from a URL.
fn extract_origin(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(parsed) => {
            let scheme = parsed.scheme();
            let host = parsed.host_str().unwrap_or("unknown");
            match parsed.port() {
                Some(port) => format!("{scheme}://{host}:{port}"),
                None => format!("{scheme}://{host}"),
            }
        }
        Err(_) => url.to_string(),
    }
}

/// Build the escrow open calls: approve + open.
fn build_open_calls(
    currency: Address,
    escrow_contract: Address,
    deposit: u128,
    payee: Address,
    salt: B256,
    authorized_signer: Address,
) -> Vec<tempo_primitives::transaction::Call> {
    use alloy::sol_types::SolCall;
    use tempo_primitives::transaction::Call;

    alloy::sol! {
        interface IEscrow {
            function open(
                address payee,
                address token,
                uint128 deposit,
                bytes32 salt,
                address authorizedSigner
            ) external;
        }
    }

    let approve_data = encode_approve(escrow_contract, U256::from(deposit));
    let open_data =
        IEscrow::openCall::new((payee, currency, deposit, salt, authorized_signer)).abi_encode();

    vec![
        Call {
            to: alloy::primitives::TxKind::Call(currency),
            value: U256::ZERO,
            input: approve_data,
        },
        Call {
            to: alloy::primitives::TxKind::Call(escrow_contract),
            value: U256::ZERO,
            input: alloy::primitives::Bytes::from(open_data),
        },
    ]
}

/// Build a voucher credential for an existing session.
async fn build_voucher_credential(
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
        channel_id: format!("{}", state.channel_id),
        cumulative_amount: state.cumulative_amount.to_string(),
        signature: format!("0x{}", hex::encode(&sig)),
    };

    Ok(mpp::PaymentCredential::with_source(
        echo.clone(),
        did.to_string(),
        payload,
    ))
}

/// Check if an OpenAI streaming chunk signals completion.
///
/// Returns true if the chunk contains `"finish_reason":"stop"` (or any non-null
/// finish_reason), indicating the model is done generating.
fn is_stream_finished(raw: &str) -> bool {
    let trimmed = raw.trim();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(reason) = v
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("finish_reason"))
        {
            return !reason.is_null();
        }
    }
    false
}

/// Extract token content from an OpenAI chat completion chunk.
///
/// Handles the `data:` payload from SSE streams in OpenAI format:
/// `{"choices":[{"delta":{"content":"token"}}]}`.
/// Returns None for `[DONE]`, empty deltas, or unparseable data.
fn extract_sse_content(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed == "[DONE]" {
        return None;
    }
    // Try to parse as OpenAI chat completion chunk
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(content) = v
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("delta"))
            .and_then(|d| d.get("content"))
            .and_then(|c| c.as_str())
        {
            if !content.is_empty() {
                return Some(content.to_string());
            }
            return None;
        }
        // No delta.content — could be role-only or finish delta
        return None;
    }
    // Not JSON — return raw content as-is (plain text SSE)
    Some(trimmed.to_string())
}

/// Stream SSE events from a response, handling voucher top-ups mid-stream.
///
/// Persists cumulative amount updates during streaming so that if the
/// process is interrupted, the session record reflects the last voucher sent.
async fn stream_sse_response(
    ctx: &SessionContext<'_>,
    state: &mut SessionState,
    response: reqwest::Response,
) -> Result<()> {
    let cli = &ctx.request_ctx.cli;
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut token_count: u64 = 0;
    let mut stdout = std::io::stdout();

    let mut stream_done = false;

    // Per-chunk timeout: if no data arrives within 30 seconds, assume the
    // stream has stalled (e.g. server waiting for a voucher that was lost).
    let chunk_timeout = std::time::Duration::from_secs(30);

    loop {
        if stream_done {
            break;
        }
        let chunk = match tokio::time::timeout(chunk_timeout, stream.next()).await {
            Ok(Some(chunk)) => chunk,
            Ok(None) => break, // stream ended
            Err(_) => {
                if cli.is_verbose() && cli.should_show_output() {
                    eprintln!("[stream timeout — no data for 30s]");
                }
                break;
            }
        };
        let chunk = chunk.context("Stream error")?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(pos) = buffer.find("\n\n") {
            let event_str = buffer[..pos + 2].to_string();
            buffer = buffer[pos + 2..].to_string();

            if let Some(event) = parse_event(&event_str) {
                match event {
                    SseEvent::Message(data) => {
                        if data.trim() == "[DONE]" {
                            stream_done = true;
                            break;
                        }
                        if let Some(content) = extract_sse_content(&data) {
                            token_count += 1;
                            write!(stdout, "{}", content)?;
                            stdout.flush()?;
                        }
                        // Detect OpenAI finish_reason to know stream is ending
                        if is_stream_finished(&data) {
                            stream_done = true;
                            break;
                        }
                    }
                    SseEvent::PaymentNeedVoucher(nv) => {
                        let required: u128 = nv.required_cumulative.parse().unwrap_or(0);
                        if cli.is_verbose() && cli.should_show_output() {
                            eprintln!("[voucher top-up: required={}]", required);
                        }

                        state.cumulative_amount = required;

                        // Persist the updated cumulative mid-stream
                        let _ = persist_session(ctx, state);

                        let voucher =
                            build_voucher_credential(ctx.signer, ctx.echo, ctx.did, state).await?;
                        let auth = mpp::format_authorization(&voucher)
                            .context("Failed to format voucher")?;

                        // POST the voucher to the server. The server reads the
                        // Authorization header to update the channel balance,
                        // then unblocks our SSE stream. We use a short timeout
                        // because the proxy may respond with a new SSE stream
                        // (treating the POST as a chat request); we only need
                        // the request to be received, not the full response.
                        let voucher_url = ctx.url.split('?').next().unwrap_or(ctx.url).to_string();
                        let voucher_client = reqwest::Client::builder()
                            .timeout(std::time::Duration::from_secs(5))
                            .build()
                            .unwrap_or_default();
                        tokio::spawn(async move {
                            let _ = voucher_client
                                .post(&voucher_url)
                                .header("Authorization", &auth)
                                .send()
                                .await;
                        });
                    }
                    SseEvent::PaymentReceipt(receipt) => {
                        if cli.is_verbose() && cli.should_show_output() {
                            eprintln!();
                            eprintln!("Stream receipt:");
                            eprintln!("  Channel: {}", receipt.channel_id);
                            eprintln!("  Spent: {}", receipt.spent);
                            if let Some(units) = receipt.units {
                                eprintln!("  Units: {}", units);
                            }
                            if let Some(ref tx) = receipt.tx_hash {
                                eprintln!("  TX: {}", tx);
                            }
                        }
                        // Receipt signals stream completion
                        stream_done = true;
                        break;
                    }
                }
            }
        }
    }

    writeln!(stdout)?;

    if cli.is_verbose() && cli.should_show_output() {
        eprintln!("Tokens streamed: {}", token_count);
        let cumulative_f64 = state.cumulative_amount as f64 / 1e6;
        eprintln!("Voucher cumulative: {:.6} pathUSD", cumulative_f64);
    }

    Ok(())
}

/// Build a reqwest request from the RequestContext's configuration.
fn build_reqwest_request(
    client: &reqwest::Client,
    request_ctx: &RequestContext,
    url: &str,
) -> reqwest::RequestBuilder {
    let method = match &request_ctx.method {
        crate::http::HttpMethod::Get => reqwest::Method::GET,
        crate::http::HttpMethod::Post => reqwest::Method::POST,
        crate::http::HttpMethod::Put => reqwest::Method::PUT,
        crate::http::HttpMethod::Patch => reqwest::Method::PATCH,
        crate::http::HttpMethod::Delete => reqwest::Method::DELETE,
        crate::http::HttpMethod::Head => reqwest::Method::HEAD,
        crate::http::HttpMethod::Options => reqwest::Method::OPTIONS,
        crate::http::HttpMethod::Custom(s) => {
            reqwest::Method::from_bytes(s.as_bytes()).unwrap_or(reqwest::Method::GET)
        }
    };

    let mut builder = client.request(method, url);

    for header in &request_ctx.query.headers {
        if let Some((name, value)) = header.split_once(':') {
            builder = builder.header(name.trim(), value.trim());
        }
    }

    if let Some(ref body) = request_ctx.body {
        builder = builder.body(body.clone());
    }

    if request_ctx.query.json.is_some() {
        builder = builder.header("Content-Type", "application/json");
    }

    builder
}
