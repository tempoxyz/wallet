//! Session-based payment handling.
//!
//! This module handles session payments (intent="session") using tempo-wallet's
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
//! signing), this implementation uses tempo-wallet's transaction builder to
//! support smart wallet / key (keychain) signing mode.
//!
//! # Module structure
//!
//! - [`channel`] — On-chain channel queries and event scanning
//! - [`streaming`] — SSE streaming with voucher top-ups
//! - [`close`] — Channel close operations (cooperative and on-chain)
//! - [`tx`] — Tempo transaction building and submission

pub(crate) mod channel;
pub(crate) mod close;
pub(crate) mod store;
mod streaming;
mod tx;

/// Fallback grace period (seconds) when escrow grace-period reads fail.
pub(crate) const DEFAULT_GRACE_PERIOD_SECS: u64 = 900;

use alloy::primitives::{Address, B256};
use anyhow::{Context, Result};
use mpp::protocol::core::extract_tx_hash;
use mpp::protocol::methods::tempo::session::{SessionCredentialPayload, TempoSessionExt};
use mpp::protocol::methods::tempo::{compute_channel_id, sign_voucher};
use mpp::{parse_receipt, ChallengeEcho};

use super::dispatch::{map_mpp_validation_error, PaymentResult};
use crate::error::TempoWalletError;
use crate::http::{HttpClient, HttpResponse};
use crate::keys::{Keystore, Signer};
use crate::network::NetworkId;
use crate::payment::session::store::SessionRecord;
use crate::payment::session::store::SessionStatus;
use crate::util::format_token_amount;

// ==================== Types ====================

/// Outcome of an on-chain close attempt.
pub(crate) enum CloseOutcome {
    /// Channel fully closed (withdrawn or cooperatively settled).
    Closed {
        tx_url: Option<String>,
        /// Formatted settlement amount (e.g., "0.002 USDC"), if available.
        amount_display: Option<String>,
    },
    /// `requestClose()` submitted or already pending; waiting for grace period.
    Pending { remaining_secs: u64 },
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
    http: &'a HttpClient,
    url: &'a str,
    network_id: NetworkId,
    origin: &'a str,
    tick_cost: u128,
    deposit: u128,
    salt: String,
    recipient: String,
    currency: String,
    /// Shared reqwest client for connection pooling across session requests.
    reqwest_client: &'a reqwest::Client,
}

// ==================== Helpers ====================

/// Extract the origin (scheme://host\[:port\]) from a URL.
fn extract_origin(url: &str) -> String {
    url::Url::parse(url)
        .map(|u| u.origin().ascii_serialization())
        .unwrap_or_else(|_| url.to_string())
}

/// Build a `SessionCredentialPayload::Open` with the given transaction bytes.
fn build_open_payload(
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

// ==================== Voucher ====================

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

// ==================== Persistence ====================

/// Persist or update the session record to disk.
fn persist_session(ctx: &SessionContext<'_>, state: &SessionState) -> Result<()> {
    let now = store::now_secs();

    let echo_json =
        serde_json::to_string(ctx.echo).context("Failed to serialize challenge echo")?;

    let session_key = store::session_key(ctx.url);
    let existing = store::load_session(&session_key)?;

    let record = if let Some(mut rec) = existing {
        // Update existing record
        rec.set_cumulative_amount(state.cumulative_amount);
        rec.challenge_echo = echo_json;
        rec.touch();
        rec
    } else {
        let dec = ctx.network_id.token().decimals;
        SessionRecord {
            version: 1,
            origin: ctx.origin.to_string(),
            request_url: ctx.url.to_string(),
            network_name: ctx.network_id.as_str().to_string(),
            chain_id: state.chain_id,
            escrow_contract: format!("{:#x}", state.escrow_contract),
            currency: ctx.currency.clone(),
            recipient: ctx.recipient.clone(),
            payer: ctx.did.to_string(),
            authorized_signer: format!("{:#x}", ctx.signer.address()),
            salt: ctx.salt.clone(),
            channel_id: format!("{:#x}", state.channel_id),
            deposit: ctx.deposit.to_string(),
            tick_cost: ctx.tick_cost.to_string(),
            cumulative_amount: state.cumulative_amount.to_string(),
            challenge_echo: echo_json,
            challenge_id: ctx.echo.id.clone(),
            state: SessionStatus::Active,
            close_requested_at: 0,
            grace_ready_at: 0,
            token_decimals: dec,
            created_at: now,
            last_used_at: now,
        }
    };

    store::save_session(&record)?;

    if ctx.http.log_enabled() {
        let cumulative_display = format_token_amount(state.cumulative_amount, ctx.network_id);
        eprintln!("Session persisted (cumulative: {cumulative_display})");
    }

    Ok(())
}

// ==================== Request ====================

/// Send the actual request with a voucher and handle the response.
///
/// Bypasses [`HttpClient::execute()`] and uses the raw reqwest client directly
/// because session streaming requires access to `reqwest::Response` for SSE
/// `bytes_stream()`, which `execute()` does not expose.
async fn send_session_request(
    ctx: &SessionContext<'_>,
    state: &mut SessionState,
) -> Result<PaymentResult> {
    if ctx.http.log_enabled() {
        eprintln!("Sending request with session voucher...");
    }

    let voucher_credential = build_voucher_credential(ctx.signer, ctx.echo, ctx.did, state).await?;

    let voucher_auth = mpp::format_authorization(&voucher_credential)
        .context("Failed to format voucher credential")?;

    let mut data_request = ctx
        .reqwest_client
        .request(ctx.http.plan.method.clone(), ctx.url)
        .header("Authorization", &voucher_auth);
    if let Some(ref body) = ctx.http.plan.body {
        data_request = data_request.body(body.clone());
    }

    let response = data_request
        .send()
        .await
        .context("Failed to send session request")?;

    let status = response.status();
    if status.as_u16() == 402 || status.is_client_error() || status.is_server_error() {
        let body = response.text().await.unwrap_or_default();
        let reason = crate::payment::extract_json_error(&body)
            .unwrap_or_else(|| body.chars().take(500).collect::<String>());
        return Err(TempoWalletError::PaymentRejected {
            reason,
            status_code: status.as_u16(),
        }
        .into());
    }

    let is_sse = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.contains("text/event-stream"));

    let channel_id = format!("{:#x}", state.channel_id);

    if is_sse {
        streaming::stream_sse_response(ctx, state, response).await?;
        Ok(PaymentResult {
            tx_hash: String::new(),
            session_id: Some(channel_id),
            status_code: 200,
            response: None,
        })
    } else {
        let http_response = HttpResponse::from_reqwest(response).await?;
        let status_code = http_response.status_code;

        Ok(PaymentResult {
            tx_hash: String::new(),
            session_id: Some(channel_id),
            status_code,
            response: Some(http_response),
        })
    }
}

// ==================== Main Logic ====================

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
pub(in crate::payment) async fn handle_session_request(
    http: &HttpClient,
    url: &str,
    resolved: super::dispatch::ResolvedChallenge,
    signer: Signer,
    keys: &Keystore,
) -> Result<PaymentResult> {
    let challenge = &resolved.challenge;
    let network_id = resolved.network_id;
    let network_name = network_id.as_str();

    challenge
        .validate_for_session("tempo")
        .map_err(|e| map_mpp_validation_error(e, challenge))?;

    let session_req: mpp::SessionRequest = challenge
        .request
        .decode()
        .context("Failed to parse session request from challenge")?;

    let chain_id = session_req.chain_id().ok_or_else(|| {
        TempoWalletError::InvalidChallenge("Missing chainId in session request".to_string())
    })?;

    let tick_cost: u128 = session_req
        .amount
        .parse()
        .context("Invalid session amount")?;

    let escrow_str = session_req
        .escrow_contract()
        .context("Missing escrow contract in session challenge")?;
    let expected_escrow = resolved.network_id.escrow_contract();
    if !escrow_str.eq_ignore_ascii_case(expected_escrow) {
        return Err(TempoWalletError::InvalidChallenge(format!(
            "Untrusted escrow contract: {} (expected {} for network {})",
            escrow_str, expected_escrow, network_name
        ))
        .into());
    }
    let escrow_contract: Address = escrow_str
        .parse()
        .context("Invalid escrow contract address")?;

    let currency: Address = session_req
        .currency
        .parse()
        .context("Invalid currency address")?;

    let recipient: Address = session_req
        .recipient
        .as_deref()
        .ok_or(TempoWalletError::InvalidChallenge(
            "Missing recipient in session challenge".to_string(),
        ))?
        .parse()
        .context("Invalid recipient address")?;

    if http.log_enabled() {
        let cost_display = format_token_amount(tick_cost, network_id);
        eprintln!(
            "Cost per {}: {}",
            session_req.unit_type.as_deref().unwrap_or("request"),
            cost_display
        );
    }

    // Dry-run: print session parameters and exit without signing or transacting
    if http.dry_run {
        let cost_display = format_token_amount(tick_cost, network_id);

        println!("[DRY RUN] Session payment would be made:");
        println!("Protocol: MPP (https://mpp.dev)");
        println!("Method: {}", challenge.method);
        println!("Intent: session");
        println!("Network: {}", network_name);
        println!(
            "Cost per {}: {}",
            session_req.unit_type.as_deref().unwrap_or("request"),
            cost_display
        );
        println!(
            "Currency: {}",
            resolved.network_id.address_link(&session_req.currency)
        );
        if let Some(ref recipient) = session_req.recipient {
            println!("Recipient: {}", resolved.network_id.address_link(recipient));
        }
        if let Some(ref deposit) = session_req.suggested_deposit {
            let deposit_val: u128 = deposit.parse().unwrap_or(0);
            let deposit_display = format_token_amount(deposit_val, network_id);
            println!("Suggested deposit: {}", deposit_display);
        }

        return Ok(PaymentResult {
            tx_hash: String::new(),
            session_id: None,
            status_code: 200,
            response: None,
        });
    }

    let key_address = signer.signer.address();
    let from = signer.from;

    // Always refresh the challenge echo from the current 402 response
    let echo = challenge.to_echo();
    let origin = extract_origin(url);
    let session_key = store::session_key(url);

    // Determine deposit: use suggested_deposit or default to 1 token (10^decimals atomic units).
    // Cap at 5 tokens to limit exposure to malicious servers.
    // Also clamp to the wallet's available balance so we don't revert on insufficient funds.
    let base_units: u128 = 10u128.saturating_pow(network_id.token().decimals as u32);
    let max_deposit: u128 = 5u128.saturating_mul(base_units);
    let mut deposit: u128 = session_req
        .suggested_deposit
        .as_ref()
        .and_then(|s| s.parse().ok())
        .unwrap_or(base_units)
        .min(max_deposit);

    // Query on-chain token balance and clamp deposit to available funds.
    // Use 50% of the balance to reserve the rest for gas fees (on Tempo,
    // gas is paid in USDC via account abstraction).
    let balance_provider =
        alloy::providers::ProviderBuilder::new().connect_http(resolved.rpc_url.clone());
    if let Ok(balance) = channel::query_token_balance(&balance_provider, currency, from).await {
        let balance_u128: u128 = balance.try_into().unwrap_or(u128::MAX);
        let usable = balance_u128 / 2;
        if usable < deposit {
            deposit = usable;
            if http.log_enabled() {
                eprintln!(
                    "Clamping deposit to 50% of wallet balance: {}",
                    format_token_amount(deposit, network_id)
                );
            }
        }
    }

    let did = format!("did:pkh:eip155:{}:{:#x}", chain_id, from);
    let recipient_hex = format!("{:#x}", recipient);
    let currency_hex = format!("{:#x}", currency);

    // Check for an existing persisted session.
    // Reuse requires matching payer AND challenge parameters (escrow, currency,
    // recipient, chain) to avoid a wasted round trip when the server changes config.
    let existing = store::load_session(&session_key)?;
    let reuse = existing.as_ref().is_some_and(|r| {
        r.payer == did
            && r.escrow_contract == format!("{:#x}", escrow_contract)
            && r.currency == currency_hex
            && r.recipient == recipient_hex
            && r.chain_id == chain_id
    });

    if reuse {
        let record = existing.unwrap();
        if http.log_enabled() {
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
            signer: &signer.signer,
            echo: &echo,
            did: &did,
            http,
            url,
            network_id,
            origin: &origin,
            tick_cost,
            deposit,
            salt: record.salt.clone(),
            recipient: recipient_hex.clone(),
            currency: currency_hex.clone(),
            reqwest_client: http.client(),
        };

        match send_session_request(&ctx, &mut state).await {
            Ok(result) => {
                persist_session(&ctx, &state)?;
                return Ok(result);
            }
            Err(e) => {
                if http.log_enabled() {
                    eprintln!("Session reuse failed: {e}");
                }
                // Best-effort cooperative close of the old channel
                if http.log_enabled() {
                    eprintln!("Attempting cooperative close of old channel...");
                }
                let _ = close::try_cooperative_close_from_record(&record, keys).await;
                store::delete_session(&session_key)?;
                if http.log_enabled() {
                    eprintln!("Opening new channel...");
                }
                // Fall through to open a new channel
            }
        }
    } else if existing.is_some() {
        // Different payer or params — clean up
        if http.log_enabled() {
            eprintln!("Existing session for different payer, opening new channel...");
        }
        store::delete_session(&session_key)?;
    }

    // === Open a new channel ===

    // Acquire per-origin lock to prevent duplicate opens across processes
    let _lock_guard = match store::acquire_origin_lock(&session_key) {
        Ok(l) => Some(l),
        Err(e) => {
            if http.log_enabled() {
                eprintln!("[warn] could not acquire session lock: {e:#}");
            }
            None
        }
    };

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

    if http.log_enabled() {
        let deposit_display = format_token_amount(deposit, network_id);
        eprintln!("Opening payment channel...");
        eprintln!("  Deposit: {}", deposit_display);
        eprintln!("  Channel: {:#x}", channel_id);
    }

    let open_calls = channel::build_open_calls(
        currency,
        escrow_contract,
        deposit,
        recipient,
        salt,
        authorized_signer,
    );

    let initial_cumulative = tick_cost;
    let voucher_sig = sign_voucher(
        &signer.signer,
        channel_id,
        initial_cumulative,
        escrow_contract,
        chain_id,
    )
    .await
    .context("Failed to sign initial voucher")?;

    let payment = tx::create_tempo_payment_from_calls(
        resolved.rpc_url.as_str(),
        &signer,
        open_calls,
        currency,
        chain_id,
    )
    .await?;

    // Send the raw transaction to the server for broadcast (and optional
    // fee-payer co-signing). The server calls sendRawTransactionSync which
    // waits for block inclusion, so no client-side confirm_open is needed.
    let open_tx = format!("0x{}", hex::encode(&payment.tx_bytes));

    let open_payload = build_open_payload(
        channel_id,
        open_tx,
        authorized_signer,
        initial_cumulative,
        &voucher_sig,
    );

    let session_credential =
        mpp::PaymentCredential::with_source(echo.clone(), did.clone(), open_payload);
    let auth_header = mpp::format_authorization(&session_credential)
        .context("Failed to format open credential")?;

    let delays = [2000_u64, 3000, 5000];
    let open_response = tx::send_open_with_retry(http, url, &auth_header, &delays).await?;

    if let Some(receipt_str) = open_response.header("payment-receipt") {
        if let Ok(receipt) = parse_receipt(receipt_str) {
            let tx_ref = extract_tx_hash(receipt_str).unwrap_or(receipt.reference);
            if http.log_enabled() {
                eprintln!("Channel open tx: {}", resolved.network_id.tx_url(&tx_ref));
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
        signer: &signer.signer,
        echo: &echo,
        did: &did,
        http,
        url,
        network_id,
        origin: &origin,
        tick_cost,
        deposit,
        salt: format!("{:#x}", salt),
        recipient: recipient_hex,
        currency: currency_hex,
        reqwest_client: http.client(),
    };

    // For non-SSE responses, the open response already contains the proxied
    // upstream result — use it directly instead of making a duplicate request.
    // For SSE, fall through to send_session_request which returns a raw
    // streaming response.
    let is_sse = open_response
        .header("content-type")
        .is_some_and(|ct| ct.contains("text/event-stream"));

    if !is_sse && open_response.status_code < 400 {
        persist_session(&ctx, &state)?;
        return Ok(PaymentResult {
            tx_hash: String::new(),
            session_id: Some(format!("{:#x}", channel_id)),
            status_code: open_response.status_code,
            response: Some(open_response),
        });
    }

    let result = send_session_request(&ctx, &mut state).await?;

    persist_session(&ctx, &state)?;
    Ok(result)
}
