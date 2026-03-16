use std::error::Error;

use alloy::primitives::{Address, B256};
use mpp::protocol::methods::tempo::{compute_channel_id, session::TempoSessionExt, sign_voucher};

use super::{
    extract_origin, new_idempotency_key, open,
    persist::persist_session,
    receipt::parse_validated_session_receipt_header,
    streaming,
    voucher::{build_open_payload, build_top_up_payload, build_voucher_credential},
    ChannelContext, ChannelState,
};
use crate::{
    http::HttpResponse,
    payment::types::{PaymentResult, ResolvedChallenge},
};
use tempo_common::{
    cli::{
        format::format_token_amount,
        terminal::{address_link, sanitize_for_terminal},
    },
    error::{KeyError, NetworkError, PaymentError, TempoError},
    keys::{Keystore, Signer},
    payment::{
        classify::{map_mpp_validation_error, parse_problem_details, SessionProblemType},
        session,
    },
};

const DEFAULT_SESSION_CHAIN_ID: u64 = 42_431;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionRequestFailureKind {
    ChannelInvalidated,
    Other,
}

struct SessionRequestFailure {
    source: TempoError,
    kind: SessionRequestFailureKind,
}

impl SessionRequestFailure {
    fn into_tempo(self) -> TempoError {
        self.source
    }
}

fn session_store_error<E>(operation: &'static str, source: E) -> TempoError
where
    E: Error + Send + Sync + 'static,
{
    PaymentError::ChannelPersistenceSource {
        operation,
        source: Box::new(source),
    }
    .into()
}

fn session_reuse_preserved_error(source: TempoError) -> TempoError {
    PaymentError::ChannelPersistenceContextSource {
        operation: "session request reuse",
        context: "Session request failed; session state preserved for on-chain dispute",
        source: Box::new(source),
    }
    .into()
}

const fn challenge_missing_field(context: &'static str, field: &'static str) -> PaymentError {
    PaymentError::ChallengeMissingField { context, field }
}

fn challenge_address_parse(
    context: &'static str,
    source: alloy::hex::FromHexError,
) -> PaymentError {
    PaymentError::ChallengeAddressParse {
        context,
        source: Box::new(source),
    }
}

fn challenge_u128_parse(value: &str, context: &'static str) -> Result<u128, TempoError> {
    value.parse::<u128>().map_err(|source| {
        PaymentError::ChallengeValueParse {
            context,
            source: Box::new(source),
        }
        .into()
    })
}

fn normalize_hex_identifier(value: &str) -> String {
    if let Ok(address) = value.parse::<Address>() {
        return format!("{address:#x}");
    }
    if let Ok(channel_id) = value.parse::<B256>() {
        return format!("{channel_id:#x}");
    }

    let trimmed = value.trim();
    if let Some(hex) = trimmed.strip_prefix("0x") {
        return format!("0x{}", hex.to_ascii_lowercase());
    }
    if let Some(hex) = trimmed.strip_prefix("0X") {
        return format!("0x{}", hex.to_ascii_lowercase());
    }
    trimmed.to_ascii_lowercase()
}

fn classify_session_failure(status_code: u16, body: &str) -> SessionRequestFailureKind {
    if status_code == 410 {
        if let Some(problem) = parse_problem_details(body) {
            if matches!(
                problem.classify(),
                SessionProblemType::ChannelNotFound | SessionProblemType::ChannelFinalized
            ) {
                return SessionRequestFailureKind::ChannelInvalidated;
            }
        }
    }

    SessionRequestFailureKind::Other
}

fn challenge_channel_id_parse(value: &str, context: &'static str) -> Result<B256, TempoError> {
    value.parse::<B256>().map_err(|_| {
        PaymentError::ChallengeParse {
            context,
            reason: format!("invalid channelId bytes32 value: {value}"),
        }
        .into()
    })
}

fn assess_on_chain_reusability(channel: &session::OnChainChannel, amount: u128) -> Option<u128> {
    if channel.close_requested_at != 0 {
        return None;
    }

    let available_balance = channel.deposit.saturating_sub(channel.settled);
    if available_balance < amount {
        return None;
    }

    Some(available_balance)
}

async fn validate_reusable_channel_on_chain(
    provider: &alloy::providers::RootProvider<alloy::network::Ethereum>,
    escrow_contract: Address,
    channel_id: B256,
    amount: u128,
) -> Result<Option<(session::OnChainChannel, u128)>, TempoError> {
    let Some(on_chain) =
        session::get_channel_on_chain(provider, escrow_contract, channel_id).await?
    else {
        return Ok(None);
    };

    let Some(available_balance) = assess_on_chain_reusability(&on_chain, amount) else {
        return Ok(None);
    };

    Ok(Some((on_chain, available_balance)))
}

fn is_on_chain_identity_match(
    on_chain: &session::OnChainChannel,
    payer: Address,
    payee: Address,
    token: Address,
    authorized_signer: Address,
) -> bool {
    on_chain.payer == payer
        && on_chain.payee == payee
        && on_chain.token == token
        && on_chain.authorized_signer == authorized_signer
}

fn warn_missing_payment_receipt(context: &str) {
    eprintln!("Warning: missing Payment-Receipt on successful paid {context}");
}

fn warn_invalid_payment_receipt(context: &str, reason: &str) {
    eprintln!("Warning: ignoring invalid Payment-Receipt on paid {context}: {reason}");
}

fn apply_response_receipt(
    response: &HttpResponse,
    state: &mut ChannelState,
    context: &str,
) -> Result<Option<String>, TempoError> {
    if !(200..=299).contains(&response.status_code) {
        return Ok(None);
    }

    let Some(receipt_header) = response.header("payment-receipt") else {
        warn_missing_payment_receipt(context);
        return Ok(None);
    };

    match parse_validated_session_receipt_header(receipt_header, state.channel_id) {
        Ok(receipt) => {
            state.cumulative_amount = state.cumulative_amount.max(receipt.accepted_cumulative);
            Ok(receipt.tx_reference)
        }
        Err(reason) => {
            warn_invalid_payment_receipt(context, &reason);
            Ok(None)
        }
    }
}

fn parse_positive_problem_amount(value: &str, context: &'static str) -> Result<u128, TempoError> {
    let amount = value
        .parse::<u128>()
        .map_err(|_| PaymentError::ChallengeParse {
            context,
            reason: format!("must be a positive integer amount (got '{value}')"),
        })?;
    if amount == 0 {
        return Err(PaymentError::ChallengeSchema {
            context,
            reason: "must be > 0".to_string(),
        }
        .into());
    }
    Ok(amount)
}

fn parse_rejected_reason(status_code: u16, body: &str) -> TempoError {
    let raw_reason = tempo_common::payment::classify::extract_json_error(body)
        .unwrap_or_else(|| body.chars().take(500).collect::<String>());
    let reason = sanitize_for_terminal(&raw_reason);
    PaymentError::PaymentRejected {
        reason,
        status_code,
    }
    .into()
}

fn validate_problem_channel_id(
    problem: &tempo_common::payment::classify::ProblemDetails,
    expected: B256,
) -> Result<(), TempoError> {
    let Some(value) = problem.channel_id.as_deref() else {
        return Ok(());
    };
    let parsed = challenge_channel_id_parse(value, "session Problem Details channelId")?;
    if parsed != expected {
        return Err(PaymentError::ChallengeSchema {
            context: "session Problem Details channelId",
            reason: format!("channelId mismatch (expected {expected:#x}, got {parsed:#x})"),
        }
        .into());
    }
    Ok(())
}

async fn fetch_fresh_session_echo(
    ctx: &ChannelContext<'_>,
) -> Result<mpp::ChallengeEcho, TempoError> {
    let response = ctx
        .http
        .build_raw_request(ctx.url)
        .send()
        .await
        .map_err(NetworkError::Reqwest)?;

    if response.status().as_u16() != 402 {
        return Err(PaymentError::PaymentRejected {
            reason: format!(
                "Expected 402 while refreshing challenge, got HTTP {}",
                response.status().as_u16()
            ),
            status_code: response.status().as_u16(),
        }
        .into());
    }

    let challenge_header = response
        .headers()
        .get("www-authenticate")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| PaymentError::MissingHeader("WWW-Authenticate".to_string()))?;

    let challenge = mpp::parse_www_authenticate(challenge_header).map_err(|source| {
        PaymentError::ChallengeParseSource {
            context: "WWW-Authenticate header",
            source: Box::new(source),
        }
    })?;

    challenge
        .validate_for_session("tempo")
        .map_err(|err| map_mpp_validation_error(err, &challenge))?;

    Ok(challenge.to_echo())
}

async fn send_top_up_request(
    ctx: &ChannelContext<'_>,
    echo: &mpp::ChallengeEcho,
    state: &ChannelState,
    additional_deposit: u128,
    idempotency_key: &str,
) -> Result<HttpResponse, TempoError> {
    let calls = session::build_top_up_calls(
        ctx.token,
        state.escrow_contract,
        state.channel_id,
        additional_deposit,
    );

    let payment = open::create_tempo_payment_from_calls(
        ctx.rpc_url,
        ctx.signer,
        calls,
        ctx.token,
        state.chain_id,
        ctx.fee_payer,
    )
    .await?;
    let tx_hex = format!("0x{}", hex::encode(&payment.tx_bytes));
    let payload = build_top_up_payload(state.channel_id, tx_hex, additional_deposit);

    let credential =
        mpp::PaymentCredential::with_source(echo.clone(), ctx.did.to_string(), payload);
    let auth_header = mpp::format_authorization(&credential).map_err(|source| {
        PaymentError::ChallengeFormatSource {
            context: "topUp credential",
            source: Box::new(source),
        }
    })?;

    let response = ctx
        .reqwest_client
        .post(ctx.url)
        .header("Authorization", auth_header)
        .header("Idempotency-Key", idempotency_key)
        .send()
        .await
        .map_err(NetworkError::Reqwest)?;

    HttpResponse::from_reqwest(response).await
}

async fn run_non_streaming_top_up_recovery(
    ctx: &ChannelContext<'_>,
    state: &mut ChannelState,
    required_top_up: u128,
) -> Result<(), TempoError> {
    let additional_deposit = required_top_up.max(ctx.top_up_deposit);
    let mut echo = ctx.echo.clone();
    let top_up_idempotency_key = new_idempotency_key();

    for retry in 0..=1 {
        let top_up_response = send_top_up_request(
            ctx,
            &echo,
            state,
            additional_deposit,
            &top_up_idempotency_key,
        )
        .await?;
        if top_up_response.status_code < 400 {
            let _ = apply_response_receipt(&top_up_response, state, "topUp response")?;
            state.deposit = state.deposit.saturating_add(additional_deposit);
            let _ = persist_session(ctx, state);
            return Ok(());
        }

        let body = top_up_response.body_string().unwrap_or_default();
        if retry == 0
            && top_up_response.status_code == 410
            && parse_problem_details(&body)
                .is_some_and(|problem| problem.classify() == SessionProblemType::ChallengeNotFound)
        {
            echo = fetch_fresh_session_echo(ctx).await?;
            continue;
        }

        return Err(parse_rejected_reason(top_up_response.status_code, &body));
    }

    Err(PaymentError::PaymentRejected {
        reason: "Top-up retry exhausted".to_string(),
        status_code: 410,
    }
    .into())
}

/// Send the actual session request with a voucher and handle the response.
///
/// Bypasses [`crate::http::HttpClient::execute()`] and uses the raw reqwest client directly
/// because session streaming requires access to `reqwest::Response` for SSE
/// `bytes_stream()`, which `execute()` does not expose.
async fn send_session_request(
    ctx: &ChannelContext<'_>,
    state: &mut ChannelState,
) -> Result<PaymentResult, SessionRequestFailure> {
    let mut attempted_non_stream_top_up = false;
    let request_idempotency_key = new_idempotency_key();
    loop {
        let voucher_credential = build_voucher_credential(ctx.signer, ctx.echo, ctx.did, state)
            .await
            .map_err(|source| SessionRequestFailure {
                source,
                kind: SessionRequestFailureKind::Other,
            })?;

        let voucher_auth = mpp::format_authorization(&voucher_credential).map_err(|source| {
            SessionRequestFailure {
                source: PaymentError::ChallengeFormatSource {
                    context: "voucher credential",
                    source: Box::new(source),
                }
                .into(),
                kind: SessionRequestFailureKind::Other,
            }
        })?;

        let mut data_request = ctx
            .reqwest_client
            .request(ctx.http.method().clone(), ctx.url)
            .header("Authorization", &voucher_auth)
            .header("Idempotency-Key", &request_idempotency_key);
        data_request = crate::http::HttpClient::apply_body_from(data_request, ctx.http.body());

        let response = data_request
            .send()
            .await
            .map_err(NetworkError::Reqwest)
            .map_err(|source| SessionRequestFailure {
                source: source.into(),
                kind: SessionRequestFailureKind::Other,
            })?;

        let status = response.status();
        if status.as_u16() == 402 || status.is_client_error() || status.is_server_error() {
            let body = response.text().await.unwrap_or_default();

            if !attempted_non_stream_top_up && status.as_u16() == 402 {
                if let Some(problem) = parse_problem_details(&body) {
                    if problem.classify() == SessionProblemType::InsufficientBalance {
                        validate_problem_channel_id(&problem, state.channel_id).map_err(
                            |source| SessionRequestFailure {
                                source,
                                kind: SessionRequestFailureKind::Other,
                            },
                        )?;

                        let required_top_up_value =
                            problem.required_top_up.as_deref().ok_or_else(|| {
                                SessionRequestFailure {
                                    source: PaymentError::ChallengeMissingField {
                                        context: "session insufficient-balance requiredTopUp",
                                        field: "requiredTopUp",
                                    }
                                    .into(),
                                    kind: SessionRequestFailureKind::Other,
                                }
                            })?;
                        let required_top_up = parse_positive_problem_amount(
                            required_top_up_value,
                            "session insufficient-balance requiredTopUp",
                        )
                        .map_err(|source| SessionRequestFailure {
                            source,
                            kind: SessionRequestFailureKind::Other,
                        })?;

                        run_non_streaming_top_up_recovery(ctx, state, required_top_up)
                            .await
                            .map_err(|source| SessionRequestFailure {
                                source,
                                kind: SessionRequestFailureKind::Other,
                            })?;
                        attempted_non_stream_top_up = true;
                        continue;
                    }
                }
            }

            let failure_kind = classify_session_failure(status.as_u16(), &body);
            return Err(SessionRequestFailure {
                source: parse_rejected_reason(status.as_u16(), &body),
                kind: failure_kind,
            });
        }

        let is_sse = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|ct| ct.contains("text/event-stream"));

        let channel_id = format!("{:#x}", state.channel_id);

        if is_sse {
            streaming::stream_sse_response(ctx, state, response)
                .await
                .map_err(|source| SessionRequestFailure {
                    source,
                    kind: SessionRequestFailureKind::Other,
                })?;
            return Ok(PaymentResult {
                tx_hash: None,
                channel_id: Some(channel_id),
                status_code: 200,
                response: None,
            });
        }

        let http_response = HttpResponse::from_reqwest(response)
            .await
            .map_err(|source| SessionRequestFailure {
                source,
                kind: SessionRequestFailureKind::Other,
            })?;
        let status_code = http_response.status_code;
        let tx_hash = apply_response_receipt(&http_response, state, "session response").map_err(
            |source| SessionRequestFailure {
                source,
                kind: SessionRequestFailureKind::Other,
            },
        )?;

        return Ok(PaymentResult {
            tx_hash,
            channel_id: Some(channel_id),
            status_code,
            response: Some(http_response),
        });
    }
}

/// Handle the full MPP session flow (402 with intent="session").
///
/// This manages the session lifecycle with persistence:
/// 1. Parse the session challenge from the initial 402 response
/// 2. Check for an existing persisted session for this origin
/// 3. If found and not expired, reuse it (skip channel open)
/// 4. If not found or expired, open a new channel on-chain
/// 5. Send the real request with a voucher
/// 6. Stream SSE events (or return buffered response)
/// 7. Persist/update the session (do NOT close the channel)
pub(crate) async fn handle_session_request(
    http: &crate::http::HttpClient,
    url: &str,
    resolved: ResolvedChallenge,
    signer: Signer,
    _keys: &Keystore,
) -> Result<PaymentResult, TempoError> {
    let challenge = &resolved.challenge;
    let network_id = resolved.network_id;
    let network_name = network_id.as_str();

    challenge
        .validate_for_session("tempo")
        .map_err(|e| map_mpp_validation_error(e, challenge))?;

    let session_req: mpp::SessionRequest =
        challenge
            .request
            .decode()
            .map_err(|source| PaymentError::ChallengeParseSource {
                context: "session request from challenge",
                source: Box::new(source),
            })?;

    let chain_id = session_req.chain_id().unwrap_or(DEFAULT_SESSION_CHAIN_ID);

    let amount = challenge_u128_parse(&session_req.amount, "session request amount")?;

    let escrow_str = session_req.escrow_contract().map_err(|_| {
        challenge_missing_field("session challenge escrow contract", "escrow contract")
    })?;
    let escrow_contract: Address = escrow_str
        .parse()
        .map_err(|source| challenge_address_parse("session challenge escrow contract", source))?;
    let expected_escrow = resolved.network_id.escrow_contract();
    if escrow_contract != expected_escrow {
        return Err(PaymentError::ChallengeUntrustedEscrow {
            context: "session challenge escrow contract",
            provided: escrow_str.clone(),
            expected: expected_escrow.to_string(),
            network: network_name.to_string(),
        }
        .into());
    }

    let token: Address = session_req
        .currency
        .parse()
        .map_err(|source| challenge_address_parse("session challenge currency", source))?;

    let payee: Address = session_req
        .recipient
        .as_deref()
        .ok_or_else(|| challenge_missing_field("session challenge recipient", "recipient"))?
        .parse()
        .map_err(|source| challenge_address_parse("session challenge recipient", source))?;

    if http.log_enabled() {
        let cost_display = format_token_amount(amount, network_id);
        eprintln!(
            "Cost per {}: {}",
            session_req.unit_type.as_deref().unwrap_or("request"),
            cost_display
        );
    }

    // Dry-run: print session parameters and exit without signing or transacting
    if http.dry_run {
        let cost_display = format_token_amount(amount, network_id);

        println!("[DRY RUN] Session payment would be made:");
        println!("Protocol: MPP (https://mpp.dev)");
        println!("Method: {}", challenge.method);
        println!("Intent: session");
        println!("Network: {network_name}");
        println!(
            "Cost per {}: {}",
            session_req.unit_type.as_deref().unwrap_or("request"),
            cost_display
        );
        println!(
            "Currency: {}",
            address_link(resolved.network_id, &session_req.currency)
        );
        if let Some(ref payee) = session_req.recipient {
            println!("Recipient: {}", address_link(resolved.network_id, payee));
        }
        if let Some(ref deposit) = session_req.suggested_deposit {
            let deposit_val = challenge_u128_parse(deposit, "session request suggestedDeposit")?;
            let deposit_display = format_token_amount(deposit_val, network_id);
            println!("Suggested deposit: {deposit_display}");
        }

        return Ok(PaymentResult {
            tx_hash: None,
            channel_id: None,
            status_code: 200,
            response: None,
        });
    }

    let key_address = signer.signer.address();
    let from = signer.from;
    let fee_payer = session_req.fee_payer();
    let suggested_channel_id = session_req
        .channel_id()
        .as_deref()
        .map(|value| challenge_channel_id_parse(value, "session request methodDetails.channelId"))
        .transpose()?;

    // Always refresh the challenge echo from the current 402 response
    let echo = challenge.to_echo();
    let origin = extract_origin(url);
    let session_key = session::session_key(url);
    let payer_hex = format!("{from:#x}");

    // Determine deposit: use suggested_deposit or default to 1 token (10^decimals atomic units).
    // Cap at 5 tokens to limit exposure to malicious servers.
    // Also clamp to the wallet's available balance so we don't revert on insufficient funds.
    let base_units: u128 = 10u128.saturating_pow(u32::from(network_id.token().decimals));
    let max_deposit: u128 = 5u128.saturating_mul(base_units);
    let suggested_deposit = session_req
        .suggested_deposit
        .as_deref()
        .map(|value| challenge_u128_parse(value, "session request suggestedDeposit"))
        .transpose()?;
    let mut deposit: u128 = suggested_deposit.unwrap_or(base_units).min(max_deposit);

    // Query on-chain token balance and clamp deposit to available funds.
    // Use 50% of the balance to reserve the rest for gas fees (on Tempo,
    // gas is paid in USDC via account abstraction).
    let provider = alloy::providers::RootProvider::new_http(resolved.rpc_url.clone());
    if let Ok(balance) = session::query_token_balance(&provider, token, from).await {
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

    let did = format!("did:pkh:eip155:{chain_id}:{from:#x}");
    let payee_hex = format!("{payee:#x}");
    let token_hex = format!("{token:#x}");

    // === Reuse check + open (under lock) ===
    //
    // Hold a blocking per-origin lock for the full paid request lifecycle.
    // A channel permits only one active session at a time, so requests to
    // the same origin are intentionally serialized until completion.
    let _lock_guard = session::acquire_origin_lock(&session_key)
        .map_err(|e| session_store_error("acquire session lock", e))?;

    // Build reuse candidates in priority order:
    // 1) server-suggested channelId (if present),
    // 2) most-recent local reusable channel for this origin.
    let mut reuse_candidates: Vec<session::ChannelRecord> = Vec::new();
    if let Some(channel_id) = suggested_channel_id {
        let channel_id_hex = format!("{channel_id:#x}");
        let loaded = session::load_channel(&channel_id_hex)
            .map_err(|err| session_store_error("load channel", err))?;
        if let Some(record) = loaded {
            reuse_candidates.push(record);
        } else if let Some((on_chain, _available_balance)) =
            validate_reusable_channel_on_chain(&provider, escrow_contract, channel_id, amount)
                .await?
        {
            if is_on_chain_identity_match(&on_chain, from, payee, token, key_address) {
                let now = session::now_secs();
                let challenge_echo = serde_json::to_string(&echo)
                    .map_err(|err| session_store_error("serialize challenge echo", err))?;
                reuse_candidates.push(session::ChannelRecord {
                    version: 1,
                    origin: origin.clone(),
                    request_url: url.to_string(),
                    chain_id,
                    escrow_contract,
                    token: token_hex.clone(),
                    payee: payee_hex.clone(),
                    payer: payer_hex.clone(),
                    authorized_signer: key_address,
                    salt: "0x00".to_string(),
                    channel_id,
                    deposit: on_chain.deposit,
                    cumulative_amount: on_chain.settled,
                    challenge_echo,
                    state: session::ChannelStatus::Active,
                    close_requested_at: 0,
                    grace_ready_at: 0,
                    created_at: now,
                    last_used_at: now,
                });
            }
        }
    }

    if let Some(record) = session::find_reusable_channel(
        &origin,
        &payer_hex,
        escrow_contract,
        &token_hex,
        &payee_hex,
        chain_id,
    )
    .map_err(|err| session_store_error("load session", err))?
    {
        let seen = reuse_candidates.iter().any(|candidate| {
            normalize_hex_identifier(&candidate.channel_id_hex())
                == normalize_hex_identifier(&record.channel_id_hex())
        });
        if !seen {
            reuse_candidates.push(record);
        }
    }

    let mut reusable_channel = None;
    for candidate in reuse_candidates {
        if !is_session_reusable(
            &candidate,
            &payer_hex,
            escrow_contract,
            &token_hex,
            &payee_hex,
            chain_id,
            key_address,
        ) {
            continue;
        }

        let Some((on_chain, available_balance)) = validate_reusable_channel_on_chain(
            &provider,
            escrow_contract,
            candidate.channel_id,
            amount,
        )
        .await?
        else {
            continue;
        };

        if !is_on_chain_identity_match(&on_chain, from, payee, token, key_address) {
            continue;
        }

        reusable_channel = Some((candidate, on_chain.deposit, available_balance));
        break;
    }

    if let Some((record, on_chain_deposit, available_balance)) = reusable_channel {
        if http.log_enabled() {
            eprintln!("Reusing session {} for {}", record.channel_id_hex(), origin);
        }

        let channel_id: B256 = record.channel_id;
        let prev_cumulative: u128 = record.cumulative_amount_u128();

        let mut state = ChannelState {
            channel_id,
            escrow_contract,
            chain_id,
            deposit: on_chain_deposit,
            cumulative_amount: (prev_cumulative + amount).min(available_balance),
        };

        let ctx = ChannelContext {
            signer: &signer,
            payer: from,
            echo: &echo,
            did: &did,
            http,
            url,
            rpc_url: resolved.rpc_url.as_str(),
            network_id,
            origin: &origin,
            top_up_deposit: deposit,
            fee_payer,
            salt: record.salt.clone(),
            payee,
            token,
            reqwest_client: http.client(),
        };

        match send_session_request(&ctx, &mut state).await {
            Ok(result) => {
                persist_session(&ctx, &state)?;
                return Ok(result);
            }
            Err(failure) => {
                if failure.kind == SessionRequestFailureKind::ChannelInvalidated {
                    let channel_id_hex = format!("{channel_id:#x}");
                    let _ = session::delete_channel(&channel_id_hex);
                    if http.log_enabled() {
                        eprintln!(
                            "Persisted channel {channel_id_hex} rejected by server; opening a new channel"
                        );
                    }
                } else {
                    // A signed voucher may already have been transmitted, so
                    // persist the updated state and propagate the error instead
                    // of silently opening a new channel (which would double-charge).
                    let _ = persist_session(&ctx, &state);
                    return Err(session_reuse_preserved_error(failure.into_tempo()));
                }
            }
        }
    }

    let salt = B256::random();
    let authorized_signer = key_address;
    let channel_id = compute_channel_id(
        from,
        payee,
        token,
        salt,
        authorized_signer,
        escrow_contract,
        chain_id,
    );

    if http.log_enabled() {
        let deposit_display = format_token_amount(deposit, network_id);
        eprintln!("Opening channel {channel_id:#x} (deposit: {deposit_display})");
    }

    let open_calls = session::build_open_calls(
        token,
        escrow_contract,
        deposit,
        payee,
        salt,
        authorized_signer,
    );

    // Deviation from draft-tempo-session-00 §8.3.1: use `amount`
    // (instead of the typical `0`) to pre-authorize one service unit and
    // avoid an immediate extra voucher round trip before delivery starts.
    // Server-side verification still enforces cumulativeAmount >= settled.
    let initial_cumulative = amount;
    let voucher_sig = sign_voucher(
        &signer.signer,
        channel_id,
        initial_cumulative,
        escrow_contract,
        chain_id,
    )
    .await
    .map_err(|source| KeyError::SigningOperationSource {
        operation: "sign initial voucher",
        source: Box::new(source),
    })?;

    let payment = open::create_tempo_payment_from_calls(
        resolved.rpc_url.as_str(),
        &signer,
        open_calls,
        token,
        chain_id,
        fee_payer,
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
    let auth_header = mpp::format_authorization(&session_credential).map_err(|source| {
        PaymentError::ChallengeFormatSource {
            context: "open credential",
            source: Box::new(source),
        }
    })?;

    let delays = [2000_u64, 3000, 5000];
    let open_idempotency_key = new_idempotency_key();
    let open_response =
        open::send_open_with_retry(http, url, &auth_header, &open_idempotency_key, &delays).await?;

    let mut state = ChannelState {
        channel_id,
        escrow_contract,
        chain_id,
        deposit,
        cumulative_amount: initial_cumulative,
    };

    let open_tx_hash = apply_response_receipt(&open_response, &mut state, "open response")?;
    if let Some(tx_ref) = open_tx_hash.as_deref() {
        if http.log_enabled() {
            eprintln!("Channel open tx: {}", resolved.network_id.tx_url(tx_ref));
        }
    }

    let ctx = ChannelContext {
        signer: &signer,
        payer: from,
        echo: &echo,
        did: &did,
        http,
        url,
        rpc_url: resolved.rpc_url.as_str(),
        network_id,
        origin: &origin,
        top_up_deposit: deposit,
        fee_payer,
        salt: format!("{salt:#x}"),
        payee,
        token,
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
            tx_hash: open_tx_hash,
            channel_id: Some(format!("{channel_id:#x}")),
            status_code: open_response.status_code,
            response: Some(open_response),
        });
    }

    // Persist the opened channel before proceeding with voucher requests.
    persist_session(&ctx, &state)?;

    let result = send_session_request(&ctx, &mut state)
        .await
        .map_err(SessionRequestFailure::into_tempo)?;

    // Re-persist after streaming to update cumulative_amount.
    persist_session(&ctx, &state)?;
    Ok(result)
}

/// Check whether a persisted session record can be reused for a new request.
///
/// Reuse requires matching payer and channel identity fields (escrow,
/// token, payee, chain). The request price is not checked —
/// it is pricing metadata, not channel identity, and varying prices
/// (e.g. different models on the same origin) must not cause channel churn.
fn is_session_reusable(
    record: &tempo_common::payment::session::ChannelRecord,
    payer: &str,
    escrow: Address,
    token: &str,
    payee: &str,
    chain_id: u64,
    authorized_signer: Address,
) -> bool {
    record.state == tempo_common::payment::session::ChannelStatus::Active
        && normalize_hex_identifier(&record.payer) == normalize_hex_identifier(payer)
        && record.escrow_contract == escrow
        && normalize_hex_identifier(&record.token) == normalize_hex_identifier(token)
        && normalize_hex_identifier(&record.payee) == normalize_hex_identifier(payee)
        && record.chain_id == chain_id
        && record.authorized_signer == authorized_signer
}

#[cfg(test)]
mod tests {
    use super::{
        assess_on_chain_reusability, challenge_channel_id_parse, classify_session_failure,
        is_on_chain_identity_match, is_session_reusable, normalize_hex_identifier,
        parse_positive_problem_amount, session_store_error, validate_problem_channel_id,
        SessionRequestFailureKind,
    };
    use alloy::primitives::{Address, B256};
    use tempo_common::{
        error::{PaymentError, TempoError},
        payment::{
            classify::ProblemDetails,
            session::{now_secs, ChannelRecord, ChannelStatus},
        },
    };

    fn make_record() -> ChannelRecord {
        let now = now_secs();
        ChannelRecord {
            version: 1,
            origin: "https://openrouter.mpp.tempo.xyz".into(),
            request_url: "https://openrouter.mpp.tempo.xyz/v1/chat/completions".into(),
            chain_id: 4217,
            escrow_contract: Address::ZERO,
            token: "0x0000000000000000000000000000000000000001".into(),
            payee: "0x0000000000000000000000000000000000000002".into(),
            payer: "0x0000000000000000000000000000000000000099".into(),
            authorized_signer: Address::ZERO,
            salt: "0x00".into(),
            channel_id: alloy::primitives::B256::ZERO,
            deposit: 1_000_000,
            cumulative_amount: 500,
            challenge_echo: "echo".into(),
            state: ChannelStatus::Active,
            close_requested_at: 0,
            grace_ready_at: 0,
            created_at: now,
            last_used_at: now,
        }
    }

    #[test]
    fn session_store_error_maps_to_payment_error() {
        let err = session_store_error("load session", std::io::Error::other("sqlite busy"));
        match err {
            TempoError::Payment(PaymentError::ChannelPersistenceSource { operation, source }) => {
                assert_eq!(operation, "load session");
                assert_eq!(source.to_string(), "sqlite busy");
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[test]
    fn reuse_same_channel_identity() {
        let record = make_record();
        assert!(is_session_reusable(
            &record,
            "0x0000000000000000000000000000000000000099",
            Address::ZERO,
            "0x0000000000000000000000000000000000000001",
            "0x0000000000000000000000000000000000000002",
            4217,
            Address::ZERO,
        ));
    }

    #[test]
    fn reuse_rejects_different_payer() {
        let record = make_record();
        assert!(!is_session_reusable(
            &record,
            "0xdifferentpayer",
            Address::ZERO,
            "0x0000000000000000000000000000000000000001",
            "0x0000000000000000000000000000000000000002",
            4217,
            Address::ZERO,
        ));
    }

    #[test]
    fn reuse_rejects_different_recipient() {
        let record = make_record();
        assert!(!is_session_reusable(
            &record,
            "0x0000000000000000000000000000000000000099",
            Address::ZERO,
            "0x0000000000000000000000000000000000000001",
            "0xdifferentrecipient",
            4217,
            Address::ZERO,
        ));
    }

    #[test]
    fn reuse_rejects_different_chain() {
        let record = make_record();
        assert!(!is_session_reusable(
            &record,
            "0x0000000000000000000000000000000000000099",
            Address::ZERO,
            "0x0000000000000000000000000000000000000001",
            "0x0000000000000000000000000000000000000002",
            42431, // different chain
            Address::ZERO,
        ));
    }

    #[test]
    fn reuse_normalizes_hex_fields() {
        let mut record = make_record();
        record.token = "0X0000000000000000000000000000000000000001".into();
        record.payee = "0X0000000000000000000000000000000000000002".into();
        record.payer = "0X0000000000000000000000000000000000000099".into();
        assert!(is_session_reusable(
            &record,
            "0x0000000000000000000000000000000000000099",
            Address::ZERO,
            "0x0000000000000000000000000000000000000001",
            "0x0000000000000000000000000000000000000002",
            4217,
            Address::ZERO,
        ));
    }

    #[test]
    fn channel_not_found_problem_is_recoverable() {
        let body = r#"{"type":"https://paymentauth.org/problems/session/channel-not-found","detail":"not found"}"#;
        assert_eq!(
            classify_session_failure(410, body),
            SessionRequestFailureKind::ChannelInvalidated
        );
    }

    #[test]
    fn unknown_problem_is_not_recoverable() {
        let body = r#"{"type":"https://paymentauth.org/problems/session/invalid-signature","detail":"bad sig"}"#;
        assert_eq!(
            classify_session_failure(402, body),
            SessionRequestFailureKind::Other
        );
    }

    #[test]
    fn normalize_hex_identifier_handles_prefix_case() {
        assert_eq!(normalize_hex_identifier("0XABCDEF"), "0xabcdef".to_string());
    }

    #[test]
    fn challenge_channel_id_parse_rejects_invalid_bytes32() {
        assert!(
            challenge_channel_id_parse("0x1234", "ctx").is_err(),
            "short channel IDs must be rejected"
        );
    }

    #[test]
    fn parse_positive_problem_amount_rejects_zero() {
        let err = parse_positive_problem_amount("0", "ctx").unwrap_err();
        assert!(err.to_string().contains("must be > 0"));
    }

    #[test]
    fn validate_problem_channel_id_rejects_mismatch() {
        let expected = B256::from([0x11; 32]);
        let problem = ProblemDetails {
            problem_type: "https://paymentauth.org/problems/session/insufficient-balance"
                .to_string(),
            title: None,
            status: Some(402),
            detail: Some("need more deposit".to_string()),
            required_top_up: Some("100".to_string()),
            channel_id: Some(format!("{:#x}", B256::from([0x22; 32]))),
            extensions: std::collections::BTreeMap::new(),
        };

        let err = validate_problem_channel_id(&problem, expected).unwrap_err();
        assert!(err.to_string().contains("channelId mismatch"));
    }

    #[test]
    fn on_chain_reusability_requires_open_channel_and_balance() {
        let channel = tempo_common::payment::session::OnChainChannel {
            payer: Address::ZERO,
            payee: Address::ZERO,
            token: Address::ZERO,
            authorized_signer: Address::ZERO,
            deposit: 1_000,
            settled: 700,
            close_requested_at: 0,
        };
        assert_eq!(assess_on_chain_reusability(&channel, 200), Some(300));
        assert_eq!(assess_on_chain_reusability(&channel, 400), None);

        let closing_channel = tempo_common::payment::session::OnChainChannel {
            close_requested_at: 1,
            ..channel
        };
        assert_eq!(assess_on_chain_reusability(&closing_channel, 100), None);
    }

    #[test]
    fn on_chain_identity_match_checks_all_identity_fields() {
        let channel = tempo_common::payment::session::OnChainChannel {
            payer: Address::from([0x11; 20]),
            payee: Address::from([0x22; 20]),
            token: Address::from([0x33; 20]),
            authorized_signer: Address::from([0x44; 20]),
            deposit: 1,
            settled: 0,
            close_requested_at: 0,
        };

        assert!(is_on_chain_identity_match(
            &channel,
            Address::from([0x11; 20]),
            Address::from([0x22; 20]),
            Address::from([0x33; 20]),
            Address::from([0x44; 20])
        ));
        assert!(!is_on_chain_identity_match(
            &channel,
            Address::from([0x10; 20]),
            Address::from([0x22; 20]),
            Address::from([0x33; 20]),
            Address::from([0x44; 20])
        ));
    }
}
