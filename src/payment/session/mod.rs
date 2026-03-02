//! Session-based payment handling.
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
//! support smart wallet / key (keychain) signing mode.
//!
//! # Module structure
//!
//! - [`channel`] — On-chain channel queries and event scanning
//! - [`streaming`] — SSE streaming with voucher top-ups
//! - [`close`] — Channel close operations (cooperative and on-chain)
//! - [`tx`] — Tempo transaction building and submission

pub(crate) mod channel;
mod close;
pub(crate) mod store;
mod streaming;
mod tx;

use std::collections::HashMap;

use alloy::primitives::{Address, B256};
use anyhow::{Context, Result};
use mpp::protocol::core::extract_tx_hash;
use mpp::protocol::methods::tempo::session::{SessionCredentialPayload, TempoSessionExt};
use mpp::protocol::methods::tempo::{compute_channel_id, sign_voucher};
use mpp::{parse_receipt, parse_www_authenticate, ChallengeEcho};

use crate::config::Config;
use crate::error::{map_mpp_validation_error, PrestoError};
use crate::http::{HttpClient, HttpResponse, RequestContext};
use crate::network::{format_address_link, resolve_token_meta, Network};
use crate::util::format_token_amount;
use crate::wallet::credentials::WalletCredentials;
use crate::wallet::signer::load_wallet_signer;
use store::{SessionRecord, SESSION_TTL_SECS};

// Re-export public API
pub(crate) use channel::{find_all_channels_for_payer, query_channel_state, read_grace_period};
pub(crate) use close::{close_channel_by_id, close_discovered_channel, close_session_from_record};

// ==================== Types ====================

/// Outcome of an on-chain close attempt.
pub(crate) enum CloseOutcome {
    /// Channel fully closed (withdrawn or cooperatively settled).
    Closed,
    /// `requestClose()` submitted or already pending; waiting for grace period.
    Pending { remaining_secs: u64 },
}

/// Result of a session request — either streamed (already printed) or a buffered response.
pub(crate) enum SessionResult {
    /// SSE tokens were streamed directly to stdout.
    Streamed { channel_id: String },
    /// A normal (non-SSE) response that should be handled by the regular output path.
    Response {
        response: HttpResponse,
        channel_id: String,
    },
}

/// State for an active session channel.
pub(crate) struct SessionState {
    pub channel_id: B256,
    pub escrow_contract: Address,
    pub chain_id: u64,
    pub cumulative_amount: u128,
}

/// Shared context for session operations (streaming, closing).
pub(crate) struct SessionContext<'a> {
    pub signer: &'a alloy::signers::local::PrivateKeySigner,
    pub echo: &'a ChallengeEcho,
    pub did: &'a str,
    pub request_ctx: &'a RequestContext,
    pub url: &'a str,
    pub network_name: &'a str,
    pub origin: &'a str,
    pub tick_cost: u128,
    pub deposit: u128,
    pub salt: String,
    pub recipient: String,
    pub currency: String,
    /// Shared reqwest client for connection pooling across session requests.
    pub reqwest_client: &'a reqwest::Client,
}

impl<'a> SessionContext<'a> {
    #[allow(clippy::too_many_arguments)]
    fn new(
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
        reqwest_client: &'a reqwest::Client,
    ) -> Self {
        Self {
            signer,
            echo,
            did,
            request_ctx,
            url,
            network_name,
            origin,
            tick_cost,
            deposit,
            salt,
            recipient,
            currency,
            reqwest_client,
        }
    }

    /// Resolve the token symbol for the current session (e.g., "USDC" or "pathUSD").
    pub(crate) fn token_symbol(&self) -> &'static str {
        resolve_token_meta(self.network_name, &self.currency).0
    }
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
pub(crate) async fn build_voucher_credential(
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
pub(crate) fn persist_session(ctx: &SessionContext<'_>, state: &SessionState) -> Result<()> {
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
        SessionRecord {
            version: 1,
            origin: ctx.origin.to_string(),
            request_url: ctx.url.to_string(),
            network_name: ctx.network_name.to_string(),
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
            did: ctx.did.to_string(),
            challenge_echo: echo_json,
            challenge_id: ctx.echo.id.clone(),
            created_at: now,
            last_used_at: now,
            expires_at: now + SESSION_TTL_SECS,
        }
    };

    store::save_session(&record)?;

    if ctx.request_ctx.log_enabled() {
        let cumulative_f64 = state.cumulative_amount as f64 / 1e6;
        let symbol = ctx.token_symbol();
        eprintln!("Session persisted (cumulative: {cumulative_f64:.6} {symbol})");
    }

    Ok(())
}

// ==================== Request ====================

/// Send the actual request with a voucher and handle the response.
async fn send_session_request(
    ctx: &SessionContext<'_>,
    state: &mut SessionState,
) -> Result<SessionResult> {
    if ctx.request_ctx.log_enabled() {
        eprintln!("Sending request with session voucher...");
    }

    let voucher_credential = build_voucher_credential(ctx.signer, ctx.echo, ctx.did, state).await?;

    let voucher_auth = mpp::format_authorization(&voucher_credential)
        .context("Failed to format voucher credential")?;

    let mut data_request = ctx
        .reqwest_client
        .request(ctx.request_ctx.plan.method.clone(), ctx.url)
        .header("Authorization", &voucher_auth);
    if let Some(ref body) = ctx.request_ctx.plan.body {
        data_request = data_request.body(body.clone());
    }

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
        streaming::stream_sse_response(ctx, state, response).await?;
        Ok(SessionResult::Streamed {
            channel_id: format!("{:#x}", state.channel_id),
        })
    } else {
        let status_code = status.as_u16();
        let mut headers = HashMap::new();
        for (key, value) in response.headers() {
            if let Ok(value_str) = value.to_str() {
                headers.insert(key.as_str().to_lowercase(), value_str.to_string());
            }
        }
        let body = response.bytes().await?.to_vec();

        Ok(SessionResult::Response {
            response: HttpResponse {
                status_code,
                headers,
                body,
                final_url: None,
            },
            channel_id: format!("{:#x}", state.channel_id),
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
pub async fn handle_session_request(
    config: &Config,
    request_ctx: &RequestContext,
    http_client: &HttpClient,
    url: &str,
    initial_response: &HttpResponse,
) -> Result<SessionResult> {
    let www_auth = initial_response
        .get_header("www-authenticate")
        .ok_or_else(|| PrestoError::MissingHeader("WWW-Authenticate".to_string()))?;

    let challenge =
        parse_www_authenticate(www_auth).context("Failed to parse WWW-Authenticate header")?;

    challenge
        .validate_for_session("tempo")
        .map_err(|e| map_mpp_validation_error(e, &challenge))?;

    let session_req: mpp::SessionRequest = challenge
        .request
        .decode()
        .context("Failed to parse session request from challenge")?;

    let chain_id = session_req.chain_id().ok_or_else(|| {
        PrestoError::InvalidConfig("Missing chainId in session request".to_string())
    })?;
    let network = Network::require_chain_id(chain_id)?;
    let network_name = network.as_str();

    // Validate --network constraint if set (matches charge.rs enforcement)
    if let Some(ref networks) = request_ctx.runtime.network {
        let allowed: Vec<&str> = networks.split(',').map(|s| s.trim()).collect();
        anyhow::ensure!(
            allowed.contains(&network_name),
            "Network '{}' not in allowed networks: {:?}",
            network_name,
            allowed
        );
    }

    let tick_cost: u128 = session_req
        .amount
        .parse()
        .context("Invalid session amount")?;

    let escrow_str = session_req
        .escrow_contract()
        .context("Missing escrow contract in session challenge")?;
    let expected_escrow = network.escrow_contract();
    anyhow::ensure!(
        escrow_str.eq_ignore_ascii_case(expected_escrow),
        "Untrusted escrow contract: {} (expected {} for network {})",
        escrow_str,
        expected_escrow,
        network_name
    );
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
        .ok_or_else(|| anyhow::anyhow!("Missing recipient in session challenge"))?
        .parse()
        .context("Invalid recipient address")?;

    // Resolve token metadata for human-readable amounts
    let (token_symbol, token_decimals) = resolve_token_meta(network_name, &session_req.currency);

    if request_ctx.log_enabled() {
        let cost_display = format_token_amount(tick_cost, token_symbol, token_decimals);
        eprintln!(
            "Cost per {}: {}",
            session_req.unit_type.as_deref().unwrap_or("request"),
            cost_display
        );
    }

    // Dry-run: print session parameters and exit without signing or transacting
    if request_ctx.runtime.dry_run {
        let explorer = network.info().explorer;
        let cost_display = format_token_amount(tick_cost, token_symbol, token_decimals);

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
            format_address_link(&session_req.currency, explorer.as_ref())
        );
        if let Some(ref recipient) = session_req.recipient {
            println!(
                "Recipient: {}",
                format_address_link(recipient, explorer.as_ref())
            );
        }
        if let Some(ref deposit) = session_req.suggested_deposit {
            let deposit_val: u128 = deposit.parse().unwrap_or(0);
            let deposit_display = format_token_amount(deposit_val, token_symbol, token_decimals);
            println!("Suggested deposit: {}", deposit_display);
        }

        return Ok(SessionResult::Response {
            response: HttpResponse {
                status_code: 200,
                headers: HashMap::new(),
                body: Vec::new(),
                final_url: None,
            },
            channel_id: String::new(),
        });
    }

    // Load signer and resolve signing mode (direct or keychain)
    let signing = load_wallet_signer(network_name)?;

    let key_address = signing.signer.address();
    let from = signing.from;
    let reqwest_client = http_client.inner_client();

    // Always refresh the challenge echo from the current 402 response
    let echo = challenge.to_echo();
    let origin = extract_origin(url);
    let session_key = store::session_key(url);

    // Determine deposit: use suggested_deposit or default to 1 token (1_000_000 atomic units).
    // Cap at 5 tokens (5_000_000 atomic units) to limit exposure to malicious servers.
    // Also clamp to the wallet's available balance so we don't revert on insufficient funds.
    const MAX_DEPOSIT: u128 = 5_000_000;
    let mut deposit: u128 = session_req
        .suggested_deposit
        .as_ref()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1_000_000)
        .min(MAX_DEPOSIT);

    // Query on-chain token balance and clamp deposit to available funds.
    // Use 50% of the balance to reserve the rest for gas fees (on Tempo,
    // gas is paid in USDC via account abstraction).
    let network_info = config.resolve_network(network_name)?;
    let rpc_url: url::Url = network_info
        .rpc_url
        .parse()
        .context("Invalid RPC URL for balance query")?;
    let balance_provider = alloy::providers::ProviderBuilder::new().connect_http(rpc_url);
    if let Ok(balance) = query_token_balance(&balance_provider, currency, from).await {
        let balance_u128: u128 = balance.try_into().unwrap_or(u128::MAX);
        let usable = balance_u128 / 2;
        if usable < deposit {
            deposit = usable;
            if request_ctx.log_enabled() {
                let (sym, dec) = resolve_token_meta(network_name, &session_req.currency);
                eprintln!(
                    "Clamping deposit to 50% of wallet balance: {}",
                    format_token_amount(deposit, sym, dec)
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
        !r.is_expired()
            && r.payer == did
            && r.escrow_contract == format!("{:#x}", escrow_contract)
            && r.currency == currency_hex
            && r.recipient == recipient_hex
            && r.chain_id == chain_id
    });

    if reuse {
        let record = existing.unwrap();
        if request_ctx.log_enabled() {
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

        let ctx = SessionContext::new(
            &signing.signer,
            &echo,
            &did,
            request_ctx,
            url,
            network_name,
            &origin,
            tick_cost,
            deposit,
            record.salt.clone(),
            recipient_hex.clone(),
            currency_hex.clone(),
            &reqwest_client,
        );

        match send_session_request(&ctx, &mut state).await {
            Ok(result) => {
                persist_session(&ctx, &state)?;
                return Ok(result);
            }
            Err(e) => {
                // If the server rejected us (stale session), delete and fall through
                if request_ctx.log_enabled() {
                    eprintln!("Session reuse failed: {e}");
                    eprintln!("Opening new channel...");
                }
                store::delete_session(&session_key)?;
                // Fall through to open a new channel
            }
        }
    } else if let Some(ref record) = existing {
        // Expired or different payer — clean up
        if request_ctx.log_enabled() {
            if record.is_expired() {
                eprintln!("Existing session expired, opening new channel...");
            } else {
                eprintln!("Existing session for different payer, opening new channel...");
            }
        }
        store::delete_session(&session_key)?;
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

    if request_ctx.log_enabled() {
        let deposit_display = format_token_amount(deposit, token_symbol, token_decimals);
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
        &signing.signer,
        channel_id,
        initial_cumulative,
        escrow_contract,
        chain_id,
    )
    .await
    .context("Failed to sign initial voucher")?;

    let payment =
        tx::create_tempo_payment_from_calls(config, &signing, open_calls, currency, chain_id)
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
    let open_response =
        tx::send_open_with_retry(request_ctx, http_client, url, &auth_header, &delays).await?;

    if let Some(receipt_str) = open_response.get_header("payment-receipt") {
        if let Ok(receipt) = parse_receipt(receipt_str) {
            let tx_ref = extract_tx_hash(receipt_str).unwrap_or(receipt.reference);
            let explorer = network.info().explorer;
            if request_ctx.log_enabled() {
                if let Some(exp) = explorer.as_ref() {
                    eprintln!("Channel open tx: {}", exp.tx_url(&tx_ref));
                } else {
                    eprintln!("Channel open tx: {}", tx_ref);
                }
            }
        }
    }

    WalletCredentials::mark_provisioned(network_name);

    let mut state = SessionState {
        channel_id,
        escrow_contract,
        chain_id,
        cumulative_amount: initial_cumulative,
    };

    let ctx = SessionContext::new(
        &signing.signer,
        &echo,
        &did,
        request_ctx,
        url,
        network_name,
        &origin,
        tick_cost,
        deposit,
        format!("{:#x}", salt),
        recipient_hex,
        currency_hex,
        &reqwest_client,
    );

    // For non-SSE responses, the open response already contains the proxied
    // upstream result — use it directly instead of making a duplicate request.
    // For SSE, fall through to send_session_request which returns a raw
    // streaming response.
    let is_sse = open_response
        .get_header("content-type")
        .is_some_and(|ct| ct.contains("text/event-stream"));

    if !is_sse && open_response.status_code < 400 {
        persist_session(&ctx, &state)?;
        return Ok(SessionResult::Response {
            response: open_response,
            channel_id: format!("{:#x}", channel_id),
        });
    }

    let result = send_session_request(&ctx, &mut state).await?;

    persist_session(&ctx, &state)?;
    Ok(result)
}

/// Query the on-chain TIP20 token balance for an account.
async fn query_token_balance(
    provider: &impl alloy::providers::Provider,
    token: Address,
    account: Address,
) -> anyhow::Result<alloy::primitives::U256> {
    use alloy::sol;

    sol! {
        #[sol(rpc)]
        interface ITIP20 {
            function balanceOf(address account) external view returns (uint256);
        }
    }

    let contract = ITIP20::new(token, provider);
    let balance = contract.balanceOf(account).call().await?;
    Ok(balance)
}
