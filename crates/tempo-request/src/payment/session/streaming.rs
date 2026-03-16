//! SSE streaming for session payments.
//!
//! Handles Server-Sent Events (SSE) response streams with mid-stream
//! voucher top-ups and retry logic for lost server notifications.

use std::{io::Write, time::Duration};

use mpp::server::sse::{parse_event, SseEvent};
use tokio::sync::mpsc;

use super::{
    new_idempotency_key, open,
    persist::{persist_channel_cumulative_floor, persist_session},
    receipt::{parse_validated_session_receipt_header, validate_session_receipt_fields},
    voucher::{build_top_up_payload, build_voucher_credential},
    ChannelContext, ChannelState,
};
use tempo_common::{
    error::{NetworkError, PaymentError, TempoError},
    payment::classify::{parse_problem_details, SessionProblemType},
};

fn protocol_value_error(field: &'static str, value: &str) -> TempoError {
    PaymentError::PaymentRejected {
        reason: format!(
            "Malformed payment protocol field: {field} must be an integer amount (got '{value}')"
        ),
        status_code: 502,
    }
    .into()
}

fn parse_protocol_u128(value: &str, field: &'static str) -> Result<u128, TempoError> {
    value
        .trim()
        .parse::<u128>()
        .map_err(|_| protocol_value_error(field, value))
}

fn parse_protocol_channel_id(
    value: &str,
    field: &'static str,
) -> Result<alloy::primitives::B256, TempoError> {
    value.trim().parse::<alloy::primitives::B256>().map_err(|_| {
        PaymentError::PaymentRejected {
            reason: format!(
                "Malformed payment protocol field: {field} must be a bytes32 channel ID (got '{value}')"
            ),
            status_code: 502,
        }
        .into()
    })
}

async fn send_top_up(
    ctx: &ChannelContext<'_>,
    client: &reqwest::Client,
    state: &ChannelState,
    additional_deposit: u128,
    idempotency_key: &str,
) -> Result<(), TempoError> {
    let calls = tempo_common::payment::session::build_top_up_calls(
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
        mpp::PaymentCredential::with_source(ctx.echo.clone(), ctx.did.to_string(), payload);
    let auth = mpp::format_authorization(&credential).map_err(|source| {
        PaymentError::ChallengeFormatSource {
            context: "topUp credential",
            source: Box::new(source),
        }
    })?;
    let response = client
        .post(ctx.url)
        .header("Authorization", auth)
        .header("Idempotency-Key", idempotency_key)
        .send()
        .await
        .map_err(NetworkError::Reqwest)?;
    let response = crate::http::HttpResponse::from_reqwest(response).await?;
    if response.status_code >= 400 {
        let body = response.body_string().unwrap_or_default();
        let reason = tempo_common::payment::classify::extract_json_error(&body)
            .unwrap_or_else(|| body.chars().take(500).collect::<String>());
        return Err(PaymentError::PaymentRejected {
            reason,
            status_code: response.status_code,
        }
        .into());
    }

    match response.header("payment-receipt") {
        Some(receipt_header) => {
            match parse_validated_session_receipt_header(receipt_header, state.channel_id) {
                Ok(receipt) => {
                    let _ = persist_channel_cumulative_floor(
                        state.channel_id,
                        receipt.accepted_cumulative,
                    );
                }
                Err(reason) => {
                    warn_invalid_payment_receipt("topUp response", &reason);
                }
            }
        }
        None => {
            warn_missing_payment_receipt("topUp response");
        }
    }

    Ok(())
}

fn warn_missing_payment_receipt(context: &str) {
    eprintln!("Warning: missing Payment-Receipt on successful paid {context}");
}

fn warn_invalid_payment_receipt(context: &str, reason: &str) {
    eprintln!("Warning: ignoring invalid Payment-Receipt on paid {context}: {reason}");
}

fn next_voucher_stall_timeout(current: Duration, normal_timeout: Duration) -> Duration {
    current.saturating_mul(2).min(normal_timeout)
}

fn build_voucher_transport_client(base: &reqwest::Client) -> reqwest::Client {
    reqwest::Client::builder()
        .http2_adaptive_window(true)
        .build()
        .unwrap_or_else(|_| base.clone())
}

fn voucher_problem_error(status_code: u16, body: &str) -> TempoError {
    let reason = tempo_common::payment::classify::extract_json_error(body)
        .unwrap_or_else(|| body.chars().take(500).collect::<String>());
    PaymentError::PaymentRejected {
        reason,
        status_code,
    }
    .into()
}

async fn fetch_fresh_session_echo(
    url: &str,
    client: &reqwest::Client,
) -> Result<mpp::ChallengeEcho, TempoError> {
    let response = client
        .head(url)
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

    challenge.validate_for_session("tempo").map_err(|err| {
        tempo_common::payment::classify::map_mpp_validation_error(err, &challenge)
    })?;

    Ok(challenge.to_echo())
}

#[derive(Debug)]
struct VoucherTaskResult {
    idempotency_key: String,
    result: Result<(), TempoError>,
}

struct VoucherSubmitContext<'a> {
    url: &'a str,
    signer: &'a tempo_common::keys::Signer,
    did: &'a str,
    debug_enabled: bool,
    client: &'a reqwest::Client,
    state: &'a ChannelState,
    auth: &'a str,
    idempotency_key: &'a str,
}

const fn should_fallback_to_post(status_code: u16) -> bool {
    matches!(status_code, 405 | 501)
}

async fn submit_voucher_update(ctx: VoucherSubmitContext<'_>) -> Result<(), TempoError> {
    let head_response = ctx
        .client
        .head(ctx.url)
        .header("Authorization", ctx.auth)
        .header("Idempotency-Key", ctx.idempotency_key)
        .send()
        .await;

    let response = match head_response {
        Ok(resp) if should_fallback_to_post(resp.status().as_u16()) => {
            if ctx.debug_enabled {
                eprintln!(
                    "[voucher HEAD unsupported ({}) — falling back to POST]",
                    resp.status()
                );
            }
            ctx.client
                .post(ctx.url)
                .header("Authorization", ctx.auth)
                .header("Idempotency-Key", ctx.idempotency_key)
                .send()
                .await
                .map_err(NetworkError::Reqwest)?
        }
        Ok(resp) => resp,
        Err(_) => {
            if ctx.debug_enabled {
                eprintln!("[voucher HEAD transport failure — falling back to POST]");
            }
            ctx.client
                .post(ctx.url)
                .header("Authorization", ctx.auth)
                .header("Idempotency-Key", ctx.idempotency_key)
                .send()
                .await
                .map_err(NetworkError::Reqwest)?
        }
    };

    if response.status().is_success() {
        match response
            .headers()
            .get("payment-receipt")
            .and_then(|value| value.to_str().ok())
        {
            Some(receipt_header) => {
                match parse_validated_session_receipt_header(receipt_header, ctx.state.channel_id) {
                    Ok(receipt) => {
                        let _ = persist_channel_cumulative_floor(
                            ctx.state.channel_id,
                            receipt.accepted_cumulative,
                        );
                    }
                    Err(reason) => {
                        warn_invalid_payment_receipt("voucher response", &reason);
                    }
                }
            }
            None => {
                warn_missing_payment_receipt("voucher response");
            }
        }
        return Ok(());
    }

    let status_code = response.status().as_u16();
    let body = match tokio::time::timeout(Duration::from_secs(2), response.text()).await {
        Ok(Ok(text)) => text,
        _ => String::new(),
    };

    if let Some(problem) = parse_problem_details(&body) {
        if problem.classify() == SessionProblemType::ChallengeNotFound {
            let fresh_echo = fetch_fresh_session_echo(ctx.url, ctx.client).await?;
            let voucher =
                build_voucher_credential(ctx.signer, &fresh_echo, ctx.did, ctx.state).await?;
            let fresh_auth = mpp::format_authorization(&voucher).map_err(|source| {
                PaymentError::ChallengeFormatSource {
                    context: "voucher credential",
                    source: Box::new(source),
                }
            })?;

            let retry_response = ctx
                .client
                .post(ctx.url)
                .header("Authorization", fresh_auth)
                .header("Idempotency-Key", ctx.idempotency_key)
                .send()
                .await
                .map_err(NetworkError::Reqwest)?;

            if retry_response.status().is_success() {
                if let Some(receipt_header) = retry_response
                    .headers()
                    .get("payment-receipt")
                    .and_then(|value| value.to_str().ok())
                {
                    match parse_validated_session_receipt_header(
                        receipt_header,
                        ctx.state.channel_id,
                    ) {
                        Ok(receipt) => {
                            let _ = persist_channel_cumulative_floor(
                                ctx.state.channel_id,
                                receipt.accepted_cumulative,
                            );
                        }
                        Err(reason) => {
                            warn_invalid_payment_receipt("voucher response", &reason);
                        }
                    }
                } else {
                    warn_missing_payment_receipt("voucher response");
                }
                return Ok(());
            }

            let retry_status = retry_response.status().as_u16();
            let retry_body = retry_response.text().await.unwrap_or_default();
            return Err(voucher_problem_error(retry_status, &retry_body));
        }
    }

    Err(voucher_problem_error(status_code, &body))
}

/// Post a voucher to the server in a background task.
///
/// We MUST NOT await the response inline because the server may respond
/// with a streaming body (treating the POST as a new chat request).
/// Awaiting would deadlock: the server waits for us to read the SSE
/// stream, and we wait for the POST response.
fn post_voucher(
    ctx: &ChannelContext<'_>,
    client: &reqwest::Client,
    auth: &str,
    idempotency_key: &str,
    state: &ChannelState,
    result_tx: mpsc::UnboundedSender<VoucherTaskResult>,
) {
    let signer = ctx.signer.clone();
    let did = ctx.did.to_string();
    let debug_enabled = ctx.http.debug_enabled();
    let url = ctx.url.to_string();

    let state = ChannelState {
        channel_id: state.channel_id,
        escrow_contract: state.escrow_contract,
        chain_id: state.chain_id,
        deposit: state.deposit,
        cumulative_amount: state.cumulative_amount,
    };

    let client = client.clone();
    let auth = auth.to_string();
    let idempotency_key = idempotency_key.to_string();
    tokio::spawn(async move {
        let result = submit_voucher_update(VoucherSubmitContext {
            url: &url,
            signer: &signer,
            did: &did,
            debug_enabled,
            client: &client,
            state: &state,
            auth: &auth,
            idempotency_key: &idempotency_key,
        })
        .await;
        let _ = result_tx.send(VoucherTaskResult {
            idempotency_key,
            result,
        });
    });
}

/// Stream SSE events from a response, handling voucher top-ups mid-stream.
///
/// Persists cumulative amount updates during streaming so that if the
/// process is interrupted, the session record reflects the last voucher sent.
///
/// The server has a known race condition where its `wait_for_update` notification
/// can be lost (`tokio::sync::Notify` without permit storage). When a voucher POST
/// arrives but the server hasn't started awaiting yet, the notification is dropped
/// and the stream stalls. We work around this by re-posting the same voucher if
/// no progress is seen within a short timeout after the last need-voucher event.
pub(super) async fn stream_sse_response(
    ctx: &ChannelContext<'_>,
    state: &mut ChannelState,
    response: reqwest::Response,
) -> Result<(), TempoError> {
    let runtime = ctx.http;
    let mut response = response;
    let mut buffer = String::new();
    let mut token_count: u64 = 0;
    let mut stdout = std::io::stdout();
    let mut saw_header_or_trailer_receipt = false;

    let mut stream_done = false;

    if response.status().is_success() {
        if let Some(receipt_header) = response
            .headers()
            .get("payment-receipt")
            .and_then(|value| value.to_str().ok())
        {
            saw_header_or_trailer_receipt = true;
            match parse_validated_session_receipt_header(receipt_header, state.channel_id) {
                Ok(receipt) => {
                    state.cumulative_amount =
                        state.cumulative_amount.max(receipt.accepted_cumulative);
                    let _ = persist_session(ctx, state);
                }
                Err(reason) => {
                    warn_invalid_payment_receipt("SSE response header", &reason);
                }
            }
        }
    }

    // Cap SSE buffer to prevent unbounded growth from malformed streams
    // that never emit the \n\n event delimiter.
    const MAX_BUFFER_SIZE: usize = 4 * 1024 * 1024; // 4 MB

    // Use a dedicated transport client for voucher/top-up updates.
    let voucher_client = build_voucher_transport_client(ctx.reqwest_client);
    let (voucher_result_tx, mut voucher_result_rx) = mpsc::unbounded_channel::<VoucherTaskResult>();

    // Track pending voucher for retry on stall. When we send a voucher but
    // the server's notify is lost, we need to re-send to wake it up.
    let mut pending_voucher_auth: Option<String> = None;
    let mut pending_voucher_idempotency_key: Option<String> = None;
    let mut voucher_retry_count: u32 = 0;

    // Constants for stream behavior.
    const MAX_VOUCHER_RETRIES: u32 = 5;
    const NORMAL_TIMEOUT_SECS: u64 = 30;
    const VOUCHER_STALL_TIMEOUT_SECS: u64 = 3;

    // Normal timeout for when we're actively receiving tokens.
    let normal_timeout = Duration::from_secs(NORMAL_TIMEOUT_SECS);
    // Short timeout after sending a voucher — if the server doesn't resume
    // quickly, the notify was likely lost and we should re-post.
    let base_stall_timeout = Duration::from_secs(VOUCHER_STALL_TIMEOUT_SECS);
    // Exponential backoff for re-posting the same voucher (caps at normal_timeout)
    let mut current_stall_timeout = base_stall_timeout;

    loop {
        while let Ok(task_result) = voucher_result_rx.try_recv() {
            match task_result.result {
                Ok(()) => {
                    if pending_voucher_idempotency_key
                        .as_deref()
                        .is_some_and(|key| key == task_result.idempotency_key)
                    {
                        pending_voucher_auth = None;
                        pending_voucher_idempotency_key = None;
                        voucher_retry_count = 0;
                        current_stall_timeout = base_stall_timeout;
                    }
                }
                Err(error) => return Err(error),
            }
        }

        if stream_done {
            break;
        }

        let timeout = if pending_voucher_auth.is_some() {
            current_stall_timeout
        } else {
            normal_timeout
        };

        let chunk = match tokio::time::timeout(timeout, response.chunk()).await {
            Ok(Ok(Some(chunk))) => chunk,
            Ok(Ok(None)) => break, // stream ended
            Ok(Err(source)) => return Err(NetworkError::Reqwest(source).into()),
            Err(_) => {
                // Timeout — if we have a pending voucher, re-post it
                if let Some(ref auth) = pending_voucher_auth {
                    voucher_retry_count += 1;
                    if voucher_retry_count > MAX_VOUCHER_RETRIES {
                        if runtime.debug_enabled() {
                            eprintln!(
                                "[stream stall — voucher not accepted after {MAX_VOUCHER_RETRIES} retries]"
                            );
                        }
                        break;
                    }
                    if runtime.debug_enabled() {
                        eprintln!(
                            "[re-posting voucher (retry {voucher_retry_count}/{MAX_VOUCHER_RETRIES})]"
                        );
                    }
                    let idempotency_key = pending_voucher_idempotency_key
                        .as_deref()
                        .unwrap_or_default();
                    post_voucher(
                        ctx,
                        &voucher_client,
                        auth,
                        idempotency_key,
                        state,
                        voucher_result_tx.clone(),
                    );
                    // Backoff the stall timeout for the next retry, up to the normal timeout
                    current_stall_timeout =
                        next_voucher_stall_timeout(current_stall_timeout, normal_timeout);
                    continue;
                }
                if runtime.debug_enabled() {
                    eprintln!(
                        "[stream timeout — no data for {}s]",
                        normal_timeout.as_secs()
                    );
                }
                break;
            }
        };
        let chunk_str = String::from_utf8_lossy(&chunk);
        // Normalize \r\n to \n so SSE event boundary detection works with
        // servers/proxies that emit CRLF line endings.
        if chunk_str.contains('\r') {
            buffer.push_str(&chunk_str.replace("\r\n", "\n"));
        } else {
            buffer.push_str(&chunk_str);
        }

        if buffer.len() > MAX_BUFFER_SIZE {
            return Err(tempo_common::error::NetworkError::ResponseSchema {
                context: "SSE stream",
                reason: format!("buffer exceeded {MAX_BUFFER_SIZE} bytes without a complete event"),
            }
            .into());
        }

        while let Some(pos) = buffer.find("\n\n") {
            let event_str: String = buffer.drain(..pos + 2).collect();

            if let Some(event) = parse_event(&event_str) {
                match event {
                    SseEvent::Message(data) => {
                        // Any message means the voucher was accepted
                        pending_voucher_auth = None;
                        pending_voucher_idempotency_key = None;
                        voucher_retry_count = 0;
                        current_stall_timeout = base_stall_timeout;

                        if data.trim() == "[DONE]" {
                            stream_done = true;
                            break;
                        }
                        let (content, finished) = parse_sse_chunk(&data);
                        if let Some(content) = content {
                            token_count += 1;
                            write!(stdout, "{content}")?;
                            stdout.flush()?;
                        }
                        if finished {
                            stream_done = true;
                            break;
                        }
                    }
                    SseEvent::PaymentNeedVoucher(nv) => {
                        let event_channel_id = parse_protocol_channel_id(
                            &nv.channel_id,
                            "payment-need-voucher.channelId",
                        )?;
                        if event_channel_id != state.channel_id {
                            return Err(PaymentError::PaymentRejected {
                                reason: format!(
                                    "Malformed payment protocol field: payment-need-voucher.channelId mismatch (expected {:#x}, got {event_channel_id:#x})",
                                    state.channel_id
                                ),
                                status_code: 502,
                            }
                            .into());
                        }

                        let required = parse_protocol_u128(
                            &nv.required_cumulative,
                            "payment-need-voucher.requiredCumulative",
                        )?;
                        let accepted = parse_protocol_u128(
                            &nv.accepted_cumulative,
                            "payment-need-voucher.acceptedCumulative",
                        )?;
                        let on_chain_deposit =
                            parse_protocol_u128(&nv.deposit, "payment-need-voucher.deposit")?;

                        let mut effective_deposit = on_chain_deposit;
                        if required > on_chain_deposit {
                            let top_up_idempotency_key = new_idempotency_key();
                            let additional_deposit =
                                (required - on_chain_deposit).max(ctx.top_up_deposit);
                            if runtime.debug_enabled() {
                                eprintln!(
                                    "[channel top-up: required={required} deposit={on_chain_deposit} additional={additional_deposit}]"
                                );
                            }
                            send_top_up(
                                ctx,
                                &voucher_client,
                                state,
                                additional_deposit,
                                &top_up_idempotency_key,
                            )
                            .await?;
                            effective_deposit = on_chain_deposit.saturating_add(additional_deposit);
                            state.deposit = state.deposit.max(effective_deposit);
                        }

                        let next_cumulative = state.cumulative_amount.max(accepted).max(required);

                        // Use the server's required amount, clamped to our known
                        // channel deposit to prevent a malicious server from
                        // coercing an overly large voucher.
                        let authorize_amount = next_cumulative.min(effective_deposit);

                        if runtime.debug_enabled() {
                            eprintln!(
                                "[voucher top-up: required={required} authorizing={authorize_amount}]"
                            );
                        }

                        // Sign the voucher for the authorized amount (monotonic: never decrease)
                        state.cumulative_amount = authorize_amount.max(state.cumulative_amount);
                        let voucher =
                            build_voucher_credential(ctx.signer, ctx.echo, ctx.did, state).await?;
                        let auth = mpp::format_authorization(&voucher).map_err(|source| {
                            PaymentError::ChallengeFormatSource {
                                context: "voucher credential",
                                source: Box::new(source),
                            }
                        })?;
                        let voucher_idempotency_key = new_idempotency_key();

                        post_voucher(
                            ctx,
                            &voucher_client,
                            &auth,
                            &voucher_idempotency_key,
                            state,
                            voucher_result_tx.clone(),
                        );

                        // For our persisted record, keep the exact required amount
                        // (clamped to deposit) so cooperative close can match the
                        // server's expectation precisely.
                        // Enforce monotonicity: never decrease the cumulative amount.
                        state.cumulative_amount = next_cumulative.min(effective_deposit);
                        let _ = persist_session(ctx, state);

                        // Track this voucher for retry if the server stalls
                        pending_voucher_auth = Some(auth);
                        pending_voucher_idempotency_key = Some(voucher_idempotency_key);
                        voucher_retry_count = 0;
                        current_stall_timeout = base_stall_timeout;
                    }
                    SseEvent::PaymentReceipt(receipt) => {
                        pending_voucher_auth = None;
                        pending_voucher_idempotency_key = None;
                        saw_header_or_trailer_receipt = true;
                        match validate_session_receipt_fields(&receipt, state.channel_id) {
                            Ok(accepted_cumulative) => {
                                state.cumulative_amount =
                                    state.cumulative_amount.max(accepted_cumulative);
                                let _ = persist_session(ctx, state);
                            }
                            Err(reason) => {
                                warn_invalid_payment_receipt("SSE payment-receipt event", &reason);
                            }
                        }
                        if runtime.log_enabled() {
                            eprintln!();
                            eprintln!("Stream receipt:");
                            eprintln!("  Channel: {}", receipt.channel_id);
                            eprintln!("  Spent: {}", receipt.spent);
                            if let Some(units) = receipt.units {
                                eprintln!("  Units: {units}");
                            }
                            if let Some(ref tx) = receipt.tx_hash {
                                eprintln!("  TX: {tx}");
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

    if let Some(receipt_header) = response
        .headers()
        .get("payment-receipt")
        .and_then(|value| value.to_str().ok())
    {
        saw_header_or_trailer_receipt = true;
        match parse_validated_session_receipt_header(receipt_header, state.channel_id) {
            Ok(receipt) => {
                state.cumulative_amount = state.cumulative_amount.max(receipt.accepted_cumulative);
                let _ = persist_session(ctx, state);
            }
            Err(reason) => {
                warn_invalid_payment_receipt("SSE response trailer", &reason);
            }
        }
    }

    if response.status().is_success() && !saw_header_or_trailer_receipt {
        warn_missing_payment_receipt("SSE response");
    }

    writeln!(stdout)?;

    if runtime.log_enabled() {
        eprintln!("Tokens streamed: {token_count}");
        let cumulative_display =
            tempo_common::cli::format::format_token_amount(state.cumulative_amount, ctx.network_id);
        eprintln!("Voucher cumulative: {cumulative_display}");
    }

    Ok(())
}

/// Parse an SSE data chunk, extracting token content and finish status.
///
/// Returns `(content, finished)`:
/// - `content`: The text token from an `OpenAI` `delta.content` field, or the raw
///   text for non-JSON SSE. `None` for role-only deltas or empty content.
/// - `finished`: `true` if `finish_reason` is non-null (model done generating).
fn parse_sse_chunk(raw: &str) -> (Option<String>, bool) {
    let trimmed = raw.trim();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
        let choice = v.get("choices").and_then(|c| c.get(0));
        let finished = choice
            .and_then(|c| c.get("finish_reason"))
            .is_some_and(|r| !r.is_null());
        let content = choice
            .and_then(|c| c.get("delta"))
            .and_then(|d| d.get("content"))
            .and_then(|c| c.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from);
        (content, finished)
    } else {
        // Not JSON — return raw content as-is (plain text SSE)
        (Some(trimmed.to_string()), false)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    use axum::{http::StatusCode, routing::head, Router};

    use crate::http::{HttpClient, HttpRequestPlan};

    use super::*;

    const TEST_PRIVATE_KEY: &str =
        "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

    fn test_signer() -> tempo_common::keys::Signer {
        let signer = tempo_common::keys::parse_private_key_signer(TEST_PRIVATE_KEY).unwrap();
        tempo_common::keys::Signer {
            from: signer.address(),
            signer,
            signing_mode: mpp::client::tempo::signing::TempoSigningMode::Direct,
        }
    }

    fn test_echo() -> mpp::ChallengeEcho {
        mpp::ChallengeEcho {
            id: "test-challenge".to_string(),
            realm: "test".to_string(),
            method: mpp::protocol::core::MethodName::from("tempo"),
            intent: mpp::protocol::core::IntentName::from("session"),
            request: "e30".to_string(),
            expires: None,
            digest: None,
            opaque: None,
        }
    }

    fn test_http_client() -> HttpClient {
        let plan = HttpRequestPlan {
            method: reqwest::Method::POST,
            ..Default::default()
        };
        HttpClient::new(
            plan,
            tempo_common::cli::Verbosity {
                level: 0,
                show_output: false,
            },
            None,
            false,
        )
        .unwrap()
    }

    fn test_state(channel_id: alloy::primitives::B256) -> ChannelState {
        ChannelState {
            channel_id,
            escrow_contract: "0x0000000000000000000000000000000000000001"
                .parse()
                .unwrap(),
            chain_id: 4217,
            deposit: 100,
            cumulative_amount: 10,
        }
    }

    async fn spawn_test_server(app: Router) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{addr}/voucher"), server)
    }

    #[test]
    fn parse_protocol_u128_rejects_invalid_value() {
        let err = parse_protocol_u128("abc", "field").unwrap_err();
        assert!(err.to_string().contains("Malformed payment protocol field"));
    }

    #[test]
    fn should_fallback_to_post_only_for_expected_statuses() {
        assert!(should_fallback_to_post(405));
        assert!(should_fallback_to_post(501));
        assert!(!should_fallback_to_post(400));
        assert!(!should_fallback_to_post(500));
    }

    #[test]
    fn parse_protocol_u128_accepts_trimmed_integer() {
        let value = parse_protocol_u128(" 42 ", "field").unwrap();
        assert_eq!(value, 42);
    }

    #[test]
    fn parse_protocol_u128_rejects_empty_value() {
        let err = parse_protocol_u128("   ", "requiredCumulative").unwrap_err();
        assert!(err.to_string().contains("requiredCumulative"));
    }

    #[test]
    fn parse_protocol_channel_id_rejects_invalid_value() {
        let err = parse_protocol_channel_id("0x1234", "field").unwrap_err();
        assert!(err.to_string().contains("bytes32 channel ID"));
    }

    #[test]
    fn next_voucher_stall_timeout_doubles_and_caps() {
        let normal = Duration::from_secs(30);
        assert_eq!(
            next_voucher_stall_timeout(Duration::from_secs(3), normal),
            Duration::from_secs(6)
        );
        assert_eq!(
            next_voucher_stall_timeout(Duration::from_secs(20), normal),
            Duration::from_secs(30)
        );
    }

    #[test]
    fn test_parse_sse_chunk_openai_delta_content() {
        let raw = r#"{"choices":[{"delta":{"content":"Hello"},"finish_reason":null}]}"#;
        let (content, finished) = parse_sse_chunk(raw);
        assert_eq!(content.as_deref(), Some("Hello"));
        assert!(!finished);
    }

    #[test]
    fn test_parse_sse_chunk_finish_reason_stop() {
        let raw = r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#;
        let (content, finished) = parse_sse_chunk(raw);
        assert!(content.is_none());
        assert!(finished);
    }

    #[test]
    fn test_parse_sse_chunk_role_only_delta() {
        let raw = r#"{"choices":[{"delta":{"role":"assistant"},"finish_reason":null}]}"#;
        let (content, finished) = parse_sse_chunk(raw);
        assert!(content.is_none());
        assert!(!finished);
    }

    #[test]
    fn test_parse_sse_chunk_empty_content() {
        let raw = r#"{"choices":[{"delta":{"content":""},"finish_reason":null}]}"#;
        let (content, finished) = parse_sse_chunk(raw);
        assert!(content.is_none());
        assert!(!finished);
    }

    #[test]
    fn test_parse_sse_chunk_plain_text() {
        let raw = "some plain text response";
        let (content, finished) = parse_sse_chunk(raw);
        assert_eq!(content.as_deref(), Some("some plain text response"));
        assert!(!finished);
    }

    #[test]
    fn test_parse_sse_chunk_whitespace_trimmed() {
        let raw = "  hello world  \n";
        let (content, finished) = parse_sse_chunk(raw);
        assert_eq!(content.as_deref(), Some("hello world"));
        assert!(!finished);
    }

    #[test]
    fn test_parse_sse_chunk_json_no_choices() {
        let raw = r#"{"model":"gpt-4"}"#;
        let (content, finished) = parse_sse_chunk(raw);
        assert!(content.is_none());
        assert!(!finished);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn submit_voucher_update_falls_back_to_post_when_head_not_supported() {
        let head_calls = Arc::new(AtomicUsize::new(0));
        let post_calls = Arc::new(AtomicUsize::new(0));

        let head_calls_clone = Arc::clone(&head_calls);
        let post_calls_clone = Arc::clone(&post_calls);
        let app = Router::new().route(
            "/voucher",
            head(move || {
                let head_calls = Arc::clone(&head_calls_clone);
                async move {
                    head_calls.fetch_add(1, Ordering::Relaxed);
                    StatusCode::METHOD_NOT_ALLOWED
                }
            })
            .post(move || {
                let post_calls = Arc::clone(&post_calls_clone);
                async move {
                    post_calls.fetch_add(1, Ordering::Relaxed);
                    StatusCode::OK
                }
            }),
        );

        let (url, server) = spawn_test_server(app).await;

        let signer = test_signer();
        let channel_id = alloy::primitives::B256::from([0x44; 32]);
        let state = test_state(channel_id);
        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        let result = submit_voucher_update(VoucherSubmitContext {
            url: &url,
            signer: &signer,
            did: "did:pkh:eip155:4217:0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            debug_enabled: false,
            client: &client,
            state: &state,
            auth: "Payment test",
            idempotency_key: "idem-123",
        })
        .await;

        server.abort();
        let _ = server.await;

        assert!(result.is_ok());
        assert_eq!(head_calls.load(Ordering::Relaxed), 1);
        assert_eq!(post_calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn submit_voucher_update_head_success_does_not_fallback_to_post() {
        let head_calls = Arc::new(AtomicUsize::new(0));
        let post_calls = Arc::new(AtomicUsize::new(0));

        let head_calls_clone = Arc::clone(&head_calls);
        let post_calls_clone = Arc::clone(&post_calls);
        let app = Router::new().route(
            "/voucher",
            head(move || {
                let head_calls = Arc::clone(&head_calls_clone);
                async move {
                    head_calls.fetch_add(1, Ordering::Relaxed);
                    StatusCode::OK
                }
            })
            .post(move || {
                let post_calls = Arc::clone(&post_calls_clone);
                async move {
                    post_calls.fetch_add(1, Ordering::Relaxed);
                    StatusCode::OK
                }
            }),
        );

        let (url, server) = spawn_test_server(app).await;

        let signer = test_signer();
        let channel_id = alloy::primitives::B256::from([0x45; 32]);
        let state = test_state(channel_id);
        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        let result = submit_voucher_update(VoucherSubmitContext {
            url: &url,
            signer: &signer,
            did: "did:pkh:eip155:4217:0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            debug_enabled: false,
            client: &client,
            state: &state,
            auth: "Payment test",
            idempotency_key: "idem-456",
        })
        .await;

        server.abort();
        let _ = server.await;

        assert!(result.is_ok());
        assert_eq!(head_calls.load(Ordering::Relaxed), 1);
        assert_eq!(post_calls.load(Ordering::Relaxed), 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn post_voucher_surfaces_background_transport_failure() {
        let head_calls = Arc::new(AtomicUsize::new(0));
        let head_calls_clone = Arc::clone(&head_calls);
        let app = Router::new().route(
            "/voucher",
            head(move || {
                let head_calls = Arc::clone(&head_calls_clone);
                async move {
                    head_calls.fetch_add(1, Ordering::Relaxed);
                    (StatusCode::INTERNAL_SERVER_ERROR, "voucher failed")
                }
            })
            .post(|| async move { StatusCode::OK }),
        );

        let (url, server) = spawn_test_server(app).await;

        let signer = test_signer();
        let echo = test_echo();
        let did = format!("did:pkh:eip155:4217:{:#x}", signer.from);
        let http = test_http_client();
        let reqwest_client = reqwest::Client::builder().no_proxy().build().unwrap();

        let ctx = ChannelContext {
            signer: &signer,
            payer: signer.from,
            echo: &echo,
            did: &did,
            http: &http,
            url: &url,
            rpc_url: "http://127.0.0.1:8545",
            network_id: tempo_common::network::NetworkId::Tempo,
            origin: "http://127.0.0.1",
            top_up_deposit: 100,
            fee_payer: false,
            salt: "0x00".to_string(),
            payee: "0x0000000000000000000000000000000000000002"
                .parse()
                .unwrap(),
            token: "0x0000000000000000000000000000000000000003"
                .parse()
                .unwrap(),
            reqwest_client: &reqwest_client,
        };

        let state = test_state(alloy::primitives::B256::from([0x55; 32]));
        let voucher_client = reqwest::Client::builder().no_proxy().build().unwrap();
        let (result_tx, mut result_rx) = mpsc::unbounded_channel::<VoucherTaskResult>();

        post_voucher(
            &ctx,
            &voucher_client,
            "Payment test",
            "idem-background",
            &state,
            result_tx,
        );

        let task_result = tokio::time::timeout(Duration::from_secs(2), result_rx.recv())
            .await
            .expect("background task timed out")
            .expect("background task result missing");

        server.abort();
        let _ = server.await;

        assert_eq!(head_calls.load(Ordering::Relaxed), 1);
        assert_eq!(task_result.idempotency_key, "idem-background");
        assert!(task_result.result.is_err());
    }
}
