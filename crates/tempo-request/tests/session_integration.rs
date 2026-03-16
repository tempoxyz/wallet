//! Integration coverage for session-intent payment flows.

mod common;
#[path = "session_integration/spec_alignment.rs"]
mod spec_alignment;

use std::sync::{Arc, Mutex};

use axum::{
    body::{Body, Bytes},
    extract::State,
    http::{HeaderMap, Response, StatusCode},
    response::IntoResponse,
    routing::any,
    Json, Router,
};
use futures::{stream, StreamExt};
use mpp::{
    protocol::methods::tempo::{session::SessionCredentialPayload, SessionReceipt},
    Base64UrlJson, PaymentChallenge,
};
use rusqlite::Connection;
use serde_json::json;
use tempo_common::{keys::parse_private_key_signer, payment::session::session_key};

use crate::common::{get_combined_output, setup_config_only, test_command, HARDHAT_PRIVATE_KEY};

const MODERATO_ESCROW: &str = "0x542831e3e4ace07559b7c8787395f4fb99f70787";
const MODERATO_TOKEN: &str = "0x20c0000000000000000000000000000000000000";
const PAYEE_A: &str = "0x1111111111111111111111111111111111111111";
const PAYEE_B: &str = "0x2222222222222222222222222222222222222222";
const SESSION_AMOUNT: u128 = 1_000_000;

#[derive(Debug, Clone)]
struct StoredChannel {
    channel_id: String,
    payee: String,
    state: String,
    deposit: u128,
    cumulative_amount: u128,
}

#[derive(Debug, Default)]
struct SessionObservations {
    open_count: usize,
    voucher_count: usize,
    top_up_count: usize,
    top_up_actions: Vec<String>,
    credential_sources: Vec<String>,
    open_payload_keys: Vec<Vec<String>>,
    voucher_payload_keys: Vec<Vec<String>>,
    invalidating_problem_count: usize,
    insufficient_balance_problem_count: usize,
    idempotent_replay_problem_count: usize,
    missing_idempotency_key_count: usize,
    open_idempotency_keys: Vec<String>,
    voucher_idempotency_keys: Vec<String>,
    top_up_idempotency_keys: Vec<String>,
    open_transactions: Vec<String>,
    voucher_cumulative: Vec<u128>,
    voucher_paths: Vec<String>,
    top_up_paths: Vec<String>,
    voucher_head_updates: usize,
    voucher_head_statuses: Vec<u16>,
    voucher_post_updates: usize,
    unauth_head_count: usize,
    unauth_non_head_count: usize,
    top_up_challenge_not_found_problem_count: usize,
}

#[derive(Debug, Clone, Copy)]
enum PayeeMode {
    Fixed,
    ByPath,
}

#[derive(Debug)]
struct SessionServerConfig {
    payee_mode: PayeeMode,
    open_receipt_accepted: Option<u128>,
    sse_voucher_flow: bool,
    voucher_head_unsupported: bool,
    sse_receipt_accepted: Option<u128>,
    sse_required_cumulative: Option<u128>,
    sse_reported_deposit: Option<u128>,
    invalidating_problem_type_once: Option<&'static str>,
    insufficient_balance_once: bool,
    error_after_payment_once_status: Option<u16>,
    response_delay_ms: u64,
}

#[derive(Clone)]
struct SessionServerState {
    realm: String,
    config: Arc<SessionServerConfig>,
    observations: Arc<Mutex<SessionObservations>>,
}

struct SessionServer {
    base_url: String,
    observations: Arc<Mutex<SessionObservations>>,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    _handle: tokio::task::JoinHandle<()>,
}

struct SessionRpcServer {
    base_url: String,
    observations: Arc<Mutex<SessionRpcObservations>>,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    _handle: tokio::task::JoinHandle<()>,
}

#[derive(Debug, Clone, Default)]
struct SessionRpcObservations {
    eth_call_count: usize,
    send_raw_count: usize,
}

impl SessionServer {
    async fn start(config: SessionServerConfig) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{}:{}", addr.ip(), addr.port());
        let realm = format!("{}:{}", addr.ip(), addr.port());

        let observations = Arc::new(Mutex::new(SessionObservations::default()));
        let state = SessionServerState {
            realm,
            config: Arc::new(config),
            observations: observations.clone(),
        };

        let app = Router::new()
            .route("/{*path}", any(session_handler))
            .with_state(state);

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap();
        });

        Self {
            base_url,
            observations,
            shutdown_tx: Some(shutdown_tx),
            _handle: handle,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    fn snapshot(&self) -> SessionObservations {
        let guard = self.observations.lock().unwrap();
        SessionObservations {
            open_count: guard.open_count,
            voucher_count: guard.voucher_count,
            top_up_count: guard.top_up_count,
            top_up_actions: guard.top_up_actions.clone(),
            credential_sources: guard.credential_sources.clone(),
            open_payload_keys: guard.open_payload_keys.clone(),
            voucher_payload_keys: guard.voucher_payload_keys.clone(),
            invalidating_problem_count: guard.invalidating_problem_count,
            insufficient_balance_problem_count: guard.insufficient_balance_problem_count,
            idempotent_replay_problem_count: guard.idempotent_replay_problem_count,
            missing_idempotency_key_count: guard.missing_idempotency_key_count,
            open_idempotency_keys: guard.open_idempotency_keys.clone(),
            voucher_idempotency_keys: guard.voucher_idempotency_keys.clone(),
            top_up_idempotency_keys: guard.top_up_idempotency_keys.clone(),
            open_transactions: guard.open_transactions.clone(),
            voucher_cumulative: guard.voucher_cumulative.clone(),
            voucher_paths: guard.voucher_paths.clone(),
            top_up_paths: guard.top_up_paths.clone(),
            voucher_head_updates: guard.voucher_head_updates,
            voucher_head_statuses: guard.voucher_head_statuses.clone(),
            voucher_post_updates: guard.voucher_post_updates,
            unauth_head_count: guard.unauth_head_count,
            unauth_non_head_count: guard.unauth_non_head_count,
            top_up_challenge_not_found_problem_count: guard
                .top_up_challenge_not_found_problem_count,
        }
    }
}

impl SessionRpcServer {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{}:{}", addr.ip(), addr.port());

        let observations = Arc::new(Mutex::new(SessionRpcObservations::default()));
        let app = Router::new().route(
            "/",
            axum::routing::post({
                let observations = observations.clone();
                move |Json(body): Json<serde_json::Value>| {
                    let observations = observations.clone();
                    async move {
                        let response = if let Some(batch) = body.as_array() {
                            serde_json::Value::Array(
                                batch
                                    .iter()
                                    .map(|request| session_rpc_response(request, &observations))
                                    .collect(),
                            )
                        } else {
                            session_rpc_response(&body, &observations)
                        };
                        Json(response)
                    }
                }
            }),
        );
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap();
        });

        Self {
            base_url,
            observations,
            shutdown_tx: Some(shutdown_tx),
            _handle: handle,
        }
    }

    fn snapshot(&self) -> SessionRpcObservations {
        self.observations.lock().unwrap().clone()
    }
}

impl Drop for SessionRpcServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

impl Drop for SessionServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

fn session_rpc_response(
    req: &serde_json::Value,
    observations: &Arc<Mutex<SessionRpcObservations>>,
) -> serde_json::Value {
    let method = req["method"].as_str().unwrap_or("");
    let id = req["id"].clone();

    let result = match method {
        "eth_chainId" => json!("0xa5bf"), // 42431
        "eth_getTransactionCount" => json!("0x0"),
        "eth_estimateGas" => json!("0x5208"),
        "eth_maxPriorityFeePerGas" => json!("0x3b9aca00"),
        "eth_gasPrice" => json!("0x4a817c800"),
        "eth_getBalance" => json!("0xde0b6b3a7640000"),
        "eth_call" => {
            let mut guard = observations.lock().unwrap();
            guard.eth_call_count += 1;
            json!(encode_active_channel_return_data())
        }
        "eth_sendRawTransaction" => {
            let mut guard = observations.lock().unwrap();
            guard.send_raw_count += 1;
            json!("0x0000000000000000000000000000000000000000000000000000000000000001")
        }
        "eth_getBlockByNumber" => {
            let zeros = "0".repeat(512);
            json!({
                "number": "0x1",
                "hash": "0x0000000000000000000000000000000000000000000000000000000000000001",
                "parentHash": "0x0000000000000000000000000000000000000000000000000000000000000000",
                "baseFeePerGas": "0x3b9aca00",
                "timestamp": "0x60000000",
                "gasLimit": "0x1c9c380",
                "gasUsed": "0x0",
                "miner": "0x0000000000000000000000000000000000000000",
                "difficulty": "0x0",
                "totalDifficulty": "0x0",
                "extraData": "0x",
                "size": "0x100",
                "nonce": "0x0000000000000000",
                "sha3Uncles": "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347",
                "logsBloom": format!("0x{zeros}"),
                "transactionsRoot": "0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421",
                "stateRoot": "0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421",
                "receiptsRoot": "0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421",
                "transactions": [],
                "uncles": [],
            })
        }
        _ => serde_json::Value::Null,
    };

    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })
}

fn encode_active_channel_return_data() -> String {
    let payer = parse_private_key_signer(HARDHAT_PRIVATE_KEY)
        .unwrap()
        .address();
    let payee = PAYEE_A.parse().unwrap();
    let token = MODERATO_TOKEN.parse().unwrap();
    let authorized_signer = payer;

    let mut encoded = Vec::with_capacity(32 * 8);
    encoded.extend(encode_address_word(payer));
    encoded.extend(encode_address_word(payee));
    encoded.extend(encode_address_word(token));
    encoded.extend(encode_address_word(authorized_signer));
    encoded.extend(encode_u128_word(10_000_000));
    encoded.extend(encode_u128_word(0));
    encoded.extend(encode_u64_word(0));
    encoded.extend(encode_bool_word(false));

    format!("0x{}", hex::encode(encoded))
}

fn encode_address_word(address: alloy::primitives::Address) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[12..].copy_from_slice(address.as_slice());
    out
}

fn encode_u128_word(value: u128) -> [u8; 32] {
    alloy::primitives::U256::from(value).to_be_bytes::<32>()
}

fn encode_u64_word(value: u64) -> [u8; 32] {
    alloy::primitives::U256::from(value).to_be_bytes::<32>()
}

fn encode_bool_word(value: bool) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[31] = u8::from(value);
    out
}

async fn session_handler(
    State(state): State<SessionServerState>,
    headers: HeaderMap,
    request: axum::http::Request<Body>,
) -> impl IntoResponse {
    let path = request.uri().path().to_string();
    let method = request.method().clone();
    let payee = match state.config.payee_mode {
        PayeeMode::Fixed => PAYEE_A,
        PayeeMode::ByPath => {
            if path.contains("payee-b") {
                PAYEE_B
            } else {
                PAYEE_A
            }
        }
    };

    let Some(auth_value) = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
    else {
        {
            let mut observations = state.observations.lock().unwrap();
            if method == reqwest::Method::HEAD {
                observations.unauth_head_count += 1;
            } else {
                observations.unauth_non_head_count += 1;
            }
        }

        let recipient_field = if path.contains("wire-payee") {
            "payee"
        } else {
            "recipient"
        };
        let mut method_details = serde_json::json!({
            "escrowContract": MODERATO_ESCROW,
        });
        if !path.contains("missing-chain-id") {
            method_details["chainId"] = serde_json::json!(42431);
        }
        if path.contains("fee-payer-true") {
            method_details["feePayer"] = serde_json::json!(true);
        } else if path.contains("fee-payer-false") {
            method_details["feePayer"] = serde_json::json!(false);
        }
        let request_json = serde_json::json!({
            "amount": SESSION_AMOUNT.to_string(),
            "currency": MODERATO_TOKEN,
            recipient_field: payee,
            "suggestedDeposit": "2000000",
            "methodDetails": method_details,
        });
        let request = Base64UrlJson::from_value(&request_json).unwrap();
        let challenge =
            PaymentChallenge::new("session-it", &state.realm, "tempo", "session", request);
        let www_authenticate = mpp::format_www_authenticate(&challenge).unwrap();
        return Response::builder()
            .status(StatusCode::PAYMENT_REQUIRED)
            .header("www-authenticate", www_authenticate)
            .body(Body::from("Payment Required"))
            .unwrap();
    };

    let parsed = match mpp::parse_authorization(auth_value) {
        Ok(value) => value,
        Err(error) => {
            return Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Body::from(format!("invalid auth header: {error}")))
                .unwrap();
        }
    };

    let payload: SessionCredentialPayload = match parsed.payload_as() {
        Ok(value) => value,
        Err(error) => {
            return Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Body::from(format!("invalid session payload: {error}")))
                .unwrap();
        }
    };

    let payload_json = parsed.payload_as::<serde_json::Value>().ok();
    let payload_action = payload_json.as_ref().and_then(|payload| {
        payload
            .get("action")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    });
    let payload_keys = payload_json.as_ref().and_then(|payload| {
        payload.as_object().map(|object| {
            let mut keys = object.keys().cloned().collect::<Vec<_>>();
            keys.sort();
            keys
        })
    });
    let credential_source = parsed.source.clone();
    let idempotency_key = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);

    if state.config.response_delay_ms > 0 {
        tokio::time::sleep(std::time::Duration::from_millis(
            state.config.response_delay_ms,
        ))
        .await;
    }

    match payload {
        SessionCredentialPayload::Open {
            channel_id,
            transaction,
            cumulative_amount,
            ..
        } => {
            {
                let mut observations = state.observations.lock().unwrap();
                observations.open_count += 1;
                observations.open_transactions.push(transaction);
                if let Some(key) = idempotency_key.clone() {
                    observations.open_idempotency_keys.push(key);
                } else {
                    observations.missing_idempotency_key_count += 1;
                }
                if let Some(source) = credential_source.clone() {
                    observations.credential_sources.push(source);
                }
                if let Some(keys) = payload_keys.clone() {
                    observations.open_payload_keys.push(keys);
                }
            }

            let mut builder = Response::builder().status(StatusCode::OK);
            if let Some(accepted) = state.config.open_receipt_accepted {
                let receipt = build_session_receipt(&channel_id, accepted);
                builder = builder.header("payment-receipt", receipt);
            } else {
                let accepted = cumulative_amount.parse::<u128>().unwrap_or(SESSION_AMOUNT);
                let receipt = build_session_receipt(&channel_id, accepted);
                builder = builder.header("payment-receipt", receipt);
            }
            builder.body(Body::from("open-ok")).unwrap()
        }
        SessionCredentialPayload::Voucher {
            channel_id,
            cumulative_amount,
            ..
        } => {
            let cumulative = cumulative_amount.parse::<u128>().unwrap_or_default();
            {
                let mut observations = state.observations.lock().unwrap();
                observations.voucher_count += 1;
                observations.voucher_cumulative.push(cumulative);
                if let Some(key) = idempotency_key.clone() {
                    observations.voucher_idempotency_keys.push(key);
                } else {
                    observations.missing_idempotency_key_count += 1;
                }
                if let Some(source) = credential_source.clone() {
                    observations.credential_sources.push(source);
                }
                if let Some(keys) = payload_keys.clone() {
                    observations.voucher_payload_keys.push(keys);
                }
                observations.voucher_paths.push(path.clone());
            }

            if method == reqwest::Method::HEAD {
                let mut observations = state.observations.lock().unwrap();
                observations.voucher_head_updates += 1;
                let status = if state.config.voucher_head_unsupported {
                    StatusCode::METHOD_NOT_ALLOWED
                } else {
                    StatusCode::OK
                };
                observations.voucher_head_statuses.push(status.as_u16());
                return Response::builder()
                    .status(status)
                    .body(Body::empty())
                    .unwrap();
            }

            if method == reqwest::Method::POST {
                let mut observations = state.observations.lock().unwrap();
                observations.voucher_post_updates += 1;
                if path.contains("idempotent-replay")
                    && observations.idempotent_replay_problem_count == 0
                {
                    observations.idempotent_replay_problem_count += 1;
                    let body = json!({
                        "type": "https://paymentauth.org/problems/session/delta-too-small",
                        "title": "Delta too small",
                        "status": 409,
                        "detail": "voucher cumulative amount is not increasing",
                        "channelId": channel_id,
                        "acceptedCumulative": cumulative.to_string(),
                    })
                    .to_string();
                    return Response::builder()
                        .status(StatusCode::CONFLICT)
                        .header("content-type", "application/problem+json")
                        .body(Body::from(body))
                        .unwrap();
                }
                return Response::builder()
                    .status(StatusCode::OK)
                    .body(Body::from("voucher-update-ok"))
                    .unwrap();
            }

            if state.config.sse_voucher_flow {
                let required = state
                    .config
                    .sse_required_cumulative
                    .unwrap_or(2_000_000_u128);
                let required_cumulative_value = if path.contains("required-malformed") {
                    "not-a-number".to_string()
                } else if path.contains("required-empty") {
                    String::new()
                } else {
                    required.to_string()
                };
                let deposit = state.config.sse_reported_deposit.unwrap_or(3_000_000_u128);
                let receipt_accepted = state.config.sse_receipt_accepted.unwrap_or(required);
                let receipt_json = serde_json::to_string(&SessionReceipt {
                    method: "tempo".to_string(),
                    intent: "session".to_string(),
                    status: "success".to_string(),
                    timestamp: "2026-03-15T00:00:01Z".to_string(),
                    reference: "0xcccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
                        .to_string(),
                    challenge_id: "session-it".to_string(),
                    channel_id: channel_id.clone(),
                    accepted_cumulative: receipt_accepted.to_string(),
                    spent: receipt_accepted.to_string(),
                    units: Some(1),
                    tx_hash: Some(
                        "0xdddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
                            .to_string(),
                    ),
                })
                .unwrap();

                let trailing_events = if path.contains("receipt-tail") {
                    "data: {\"choices\":[{\"delta\":{\"content\":\"after-receipt\"},\"finish_reason\":null}]}\n\n"
                } else {
                    ""
                };

                let include_receipt_event = !path.contains("stall");
                let receipt_event = if include_receipt_event {
                    format!(
                        "event: payment-receipt\n\
data: {receipt_json}\n\n\
{trailing_events}"
                    )
                } else {
                    String::new()
                };

                let message_event = if path.contains("stall") {
                    String::new()
                } else {
                    "data: {\"choices\":[{\"delta\":{\"content\":\"stream\"},\"finish_reason\":null}]}\n\n\
"
                    .to_string()
                };

                if path.contains("delayed-receipt") {
                    let initial_header_accepted = required.saturating_add(100_000);
                    let first_chunk = format!("event: payment-need-voucher\ndata: {{\"channelId\":\"{channel_id}\",\"requiredCumulative\":\"{required_cumulative_value}\",\"acceptedCumulative\":\"{cumulative}\",\"deposit\":\"{deposit}\"}}\n\n{message_event}");
                    let receipt_chunk = format!("event: payment-receipt\ndata: {receipt_json}\n\n");
                    let delayed_stream = stream::once(async move {
                        Ok::<Bytes, std::convert::Infallible>(Bytes::from(first_chunk))
                    })
                    .chain(stream::once(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(350)).await;
                        Ok::<Bytes, std::convert::Infallible>(Bytes::from(receipt_chunk))
                    }));
                    return Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "text/event-stream")
                        .header(
                            "payment-receipt",
                            build_session_receipt(&channel_id, initial_header_accepted),
                        )
                        .body(Body::from_stream(delayed_stream))
                        .unwrap();
                }

                let sse = format!(
                    "event: payment-need-voucher\n\
data: {{\"channelId\":\"{channel_id}\",\"requiredCumulative\":\"{required_cumulative_value}\",\"acceptedCumulative\":\"{cumulative}\",\"deposit\":\"{deposit}\"}}\n\n\
{message_event}{receipt_event}"
                );

                if path.contains("stall") {
                    let stream = stream::once(async move {
                        Ok::<Bytes, std::convert::Infallible>(Bytes::from(sse))
                    })
                    .chain(stream::pending::<Result<Bytes, std::convert::Infallible>>());
                    return Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "text/event-stream")
                        .body(Body::from_stream(stream))
                        .unwrap();
                }

                return Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/event-stream")
                    .body(Body::from(sse))
                    .unwrap();
            }

            if let Some(problem_type) = state.config.invalidating_problem_type_once {
                let mut observations = state.observations.lock().unwrap();
                if observations.invalidating_problem_count == 0 {
                    observations.invalidating_problem_count += 1;
                    let body = json!({
                        "type": problem_type,
                        "title": "Channel invalid",
                        "status": 410,
                        "detail": "server invalidated prior channel",
                        "channelId": channel_id,
                    })
                    .to_string();
                    return Response::builder()
                        .status(StatusCode::GONE)
                        .header("content-type", "application/problem+json")
                        .body(Body::from(body))
                        .unwrap();
                }
            }

            if state.config.insufficient_balance_once {
                let mut observations = state.observations.lock().unwrap();
                if observations.insufficient_balance_problem_count == 0 {
                    observations.insufficient_balance_problem_count += 1;
                    let body = json!({
                        "type": "https://paymentauth.org/problems/session/insufficient-balance",
                        "title": "Insufficient balance",
                        "status": 402,
                        "detail": "need top up",
                        "channelId": channel_id,
                        "requiredTopUp": "500000",
                    })
                    .to_string();
                    return Response::builder()
                        .status(StatusCode::PAYMENT_REQUIRED)
                        .header("content-type", "application/problem+json")
                        .body(Body::from(body))
                        .unwrap();
                }
            }

            if let Some(status_code) = state.config.error_after_payment_once_status {
                let observations = state.observations.lock().unwrap();
                if observations.voucher_count == 1 {
                    let status = StatusCode::from_u16(status_code)
                        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                    let mut builder = Response::builder().status(status);
                    if path.contains("error-with-receipt") {
                        builder = builder.header(
                            "payment-receipt",
                            build_session_receipt(&channel_id, 9_999_999),
                        );
                    }
                    return builder
                        .body(Body::from("upstream failed after voucher authorization"))
                        .unwrap();
                }
            }

            Response::builder()
                .status(StatusCode::OK)
                .body(Body::from("voucher-ok"))
                .unwrap()
        }
        SessionCredentialPayload::TopUp { channel_id, .. } => {
            let mut observations = state.observations.lock().unwrap();
            observations.top_up_count += 1;
            if let Some(key) = idempotency_key {
                observations.top_up_idempotency_keys.push(key);
            } else {
                observations.missing_idempotency_key_count += 1;
            }
            if let Some(source) = credential_source {
                observations.credential_sources.push(source);
            }
            if let Some(action) = payload_action {
                observations.top_up_actions.push(action);
            }
            observations.top_up_paths.push(path.clone());
            if path.contains("topup-challenge-not-found")
                && observations.top_up_challenge_not_found_problem_count == 0
            {
                observations.top_up_challenge_not_found_problem_count += 1;
                let body = json!({
                    "type": "https://paymentauth.org/problems/session/challenge-not-found",
                    "title": "Challenge not found",
                    "status": 410,
                    "detail": "stale challenge during top-up",
                    "channelId": channel_id,
                })
                .to_string();
                return Response::builder()
                    .status(StatusCode::GONE)
                    .header("content-type", "application/problem+json")
                    .body(Body::from(body))
                    .unwrap();
            }
            Response::builder()
                .status(StatusCode::OK)
                .body(Body::from("topup-ok"))
                .unwrap()
        }
        other => Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(Body::from(format!("unexpected session payload: {other:?}")))
            .unwrap(),
    }
}

fn build_session_receipt(channel_id: &str, accepted_cumulative: u128) -> String {
    let receipt = SessionReceipt {
        method: "tempo".to_string(),
        intent: "session".to_string(),
        status: "success".to_string(),
        timestamp: "2026-03-15T00:00:00Z".to_string(),
        reference: "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
        challenge_id: "session-it".to_string(),
        channel_id: channel_id.to_string(),
        accepted_cumulative: accepted_cumulative.to_string(),
        spent: accepted_cumulative.to_string(),
        units: Some(1),
        tx_hash: Some(
            "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
        ),
    };

    let encoded = serde_json::to_vec(&receipt).unwrap();
    mpp::base64url_encode(&encoded)
}

fn run_session_request(temp: &tempfile::TempDir, url: &str) -> std::process::Output {
    run_session_request_with_env(temp, url, &[])
}

fn run_session_request_with_env(
    temp: &tempfile::TempDir,
    url: &str,
    envs: &[(&str, &str)],
) -> std::process::Output {
    test_command(temp)
        .envs(envs.iter().copied())
        .args([
            "--private-key",
            HARDHAT_PRIVATE_KEY,
            "--network",
            "tempo-moderato",
            url,
        ])
        .output()
        .unwrap()
}

fn spawn_session_request(temp: &tempfile::TempDir, url: &str) -> std::process::Child {
    spawn_session_request_with_env(temp, url, &[])
}

fn spawn_session_request_with_env(
    temp: &tempfile::TempDir,
    url: &str,
    envs: &[(&str, &str)],
) -> std::process::Child {
    test_command(temp)
        .envs(envs.iter().copied())
        .args([
            "--private-key",
            HARDHAT_PRIVATE_KEY,
            "--network",
            "tempo-moderato",
            url,
        ])
        .spawn()
        .unwrap()
}

fn load_channels(temp: &tempfile::TempDir) -> Vec<StoredChannel> {
    let db_path = temp.path().join(".tempo/wallet/channels.db");
    let conn = Connection::open(db_path).unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT channel_id, payee, state, deposit, cumulative_amount
             FROM channels
             ORDER BY created_at ASC",
        )
        .unwrap();

    let rows = stmt
        .query_map([], |row| {
            let deposit_raw: String = row.get(3)?;
            let cumulative_raw: String = row.get(4)?;
            Ok(StoredChannel {
                channel_id: row.get(0)?,
                payee: row.get(1)?,
                state: row.get(2)?,
                deposit: deposit_raw.parse::<u128>().unwrap_or_default(),
                cumulative_amount: cumulative_raw.parse::<u128>().unwrap_or_default(),
            })
        })
        .unwrap();

    rows.map(|row| row.unwrap()).collect()
}

fn set_all_channel_state(temp: &tempfile::TempDir, state: &str) {
    let db_path = temp.path().join(".tempo/wallet/channels.db");
    let conn = Connection::open(db_path).unwrap();
    conn.execute("UPDATE channels SET state = ?1", [state])
        .unwrap();
}

fn wait_for_channel_cumulative(
    temp: &tempfile::TempDir,
    expected: u128,
    timeout: std::time::Duration,
) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() <= deadline {
        let channels = load_channels(temp);
        if channels
            .iter()
            .any(|channel| channel.cumulative_amount == expected)
        {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    false
}

fn run_two_concurrent_session_requests(
    temp: &tempfile::TempDir,
    url: &str,
) -> (std::process::Output, std::process::Output) {
    std::thread::scope(|scope| {
        let first = scope.spawn(|| run_session_request(temp, url));
        std::thread::sleep(std::time::Duration::from_millis(25));
        let second = scope.spawn(|| run_session_request(temp, url));
        (first.join().unwrap(), second.join().unwrap())
    })
}

fn assert_payload_has_spec_fields(keys: &[String], context: &str, fields: &[&str]) {
    for field in fields {
        assert!(
            keys.iter().any(|key| key == field),
            "{context} should include spec field '{field}', observed keys={keys:?}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it01_it02_open_persist_and_reuse_authorized_replay() {
    let rpc = SessionRpcServer::start().await;
    let server = SessionServer::start(SessionServerConfig {
        payee_mode: PayeeMode::Fixed,
        open_receipt_accepted: None,
        sse_voucher_flow: false,
        voucher_head_unsupported: false,
        sse_receipt_accepted: None,
        sse_required_cumulative: None,
        sse_reported_deposit: None,
        invalidating_problem_type_once: None,
        insufficient_balance_once: false,
        error_after_payment_once_status: None,
        response_delay_ms: 0,
    })
    .await;

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let first_output = run_session_request(&temp, &server.url("/resource"));
    assert!(
        first_output.status.success(),
        "first request should succeed: {}",
        get_combined_output(&first_output)
    );

    let after_first = server.snapshot();
    assert_eq!(
        after_first.open_count, 1,
        "first request should open a channel"
    );
    assert_eq!(
        after_first.voucher_count, 0,
        "first request should not reuse voucher"
    );
    assert_eq!(after_first.open_transactions.len(), 1);
    assert!(
        after_first.open_transactions[0].starts_with("0x"),
        "open credential should carry signed transaction bytes"
    );

    let channels_after_first = load_channels(&temp);
    assert_eq!(
        channels_after_first.len(),
        1,
        "exactly one channel should be persisted"
    );
    assert_eq!(channels_after_first[0].state, "active");
    assert_eq!(channels_after_first[0].cumulative_amount, SESSION_AMOUNT);

    let second_output = run_session_request(&temp, &server.url("/resource"));
    assert!(
        second_output.status.success(),
        "second request should succeed: {}",
        get_combined_output(&second_output)
    );

    let after_second = server.snapshot();
    assert_eq!(
        after_second.open_count, 1,
        "reuse should not trigger a second open tx"
    );
    assert_eq!(
        after_second.voucher_count, 1,
        "second request should use voucher replay"
    );
    assert_eq!(
        after_second.voucher_cumulative,
        vec![SESSION_AMOUNT * 2],
        "reused request should advance cumulative amount"
    );

    let channels_after_second = load_channels(&temp);
    assert_eq!(
        channels_after_second.len(),
        1,
        "reuse should keep one channel row"
    );
    assert_eq!(
        channels_after_second[0].cumulative_amount,
        SESSION_AMOUNT * 2
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it03a_reuse_guardrail_payee_mismatch_forces_new_open() {
    let rpc = SessionRpcServer::start().await;
    let server = SessionServer::start(SessionServerConfig {
        payee_mode: PayeeMode::ByPath,
        open_receipt_accepted: None,
        sse_voucher_flow: false,
        voucher_head_unsupported: false,
        sse_receipt_accepted: None,
        sse_required_cumulative: None,
        sse_reported_deposit: None,
        invalidating_problem_type_once: None,
        insufficient_balance_once: false,
        error_after_payment_once_status: None,
        response_delay_ms: 0,
    })
    .await;

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let first_output = run_session_request(&temp, &server.url("/payee-a"));
    assert!(
        first_output.status.success(),
        "first request should succeed: {}",
        get_combined_output(&first_output)
    );

    let second_output = run_session_request(&temp, &server.url("/payee-b"));
    assert!(
        second_output.status.success(),
        "second request should succeed: {}",
        get_combined_output(&second_output)
    );

    let observed = server.snapshot();
    assert_eq!(
        observed.open_count, 2,
        "payee mismatch must prevent channel reuse"
    );
    assert_eq!(
        observed.voucher_count, 0,
        "mismatched payee should not attempt voucher reuse"
    );

    let channels = load_channels(&temp);
    assert_eq!(
        channels.len(),
        2,
        "two separate channels should be persisted"
    );
    assert_ne!(
        channels[0].channel_id, channels[1].channel_id,
        "payee mismatch should create distinct channel ids"
    );
    let payees: Vec<String> = channels.into_iter().map(|channel| channel.payee).collect();
    assert!(
        payees.contains(&PAYEE_A.to_string()) && payees.contains(&PAYEE_B.to_string()),
        "persisted channels should retain distinct payees"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it03b_reuse_guardrail_non_active_state_forces_new_open() {
    let rpc = SessionRpcServer::start().await;
    let server = SessionServer::start(SessionServerConfig {
        payee_mode: PayeeMode::Fixed,
        open_receipt_accepted: None,
        sse_voucher_flow: false,
        voucher_head_unsupported: false,
        sse_receipt_accepted: None,
        sse_required_cumulative: None,
        sse_reported_deposit: None,
        invalidating_problem_type_once: None,
        insufficient_balance_once: false,
        error_after_payment_once_status: None,
        response_delay_ms: 0,
    })
    .await;

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let first_output = run_session_request(&temp, &server.url("/resource"));
    assert!(
        first_output.status.success(),
        "first request should succeed: {}",
        get_combined_output(&first_output)
    );

    set_all_channel_state(&temp, "closing");

    let second_output = run_session_request(&temp, &server.url("/resource"));
    assert!(
        second_output.status.success(),
        "second request should succeed: {}",
        get_combined_output(&second_output)
    );

    let observed = server.snapshot();
    assert_eq!(observed.open_count, 2, "closing channel must not be reused");
    assert_eq!(
        observed.voucher_count, 0,
        "closing channel should skip voucher replay path"
    );

    let channels = load_channels(&temp);
    assert_eq!(
        channels.len(),
        2,
        "new open should create a second channel row"
    );
    assert_ne!(
        channels[0].channel_id, channels[1].channel_id,
        "non-active guardrail should create distinct channel ids"
    );
    let states: Vec<String> = channels.into_iter().map(|channel| channel.state).collect();
    assert!(
        states.contains(&"closing".to_string()) && states.contains(&"active".to_string()),
        "expected one preserved closing row and one newly active row"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it05a_it05d_open_receipt_persists_and_sets_next_reuse_baseline() {
    let rpc = SessionRpcServer::start().await;
    let server = SessionServer::start(SessionServerConfig {
        payee_mode: PayeeMode::Fixed,
        open_receipt_accepted: Some(5_000_000),
        sse_voucher_flow: false,
        voucher_head_unsupported: false,
        sse_receipt_accepted: None,
        sse_required_cumulative: None,
        sse_reported_deposit: None,
        invalidating_problem_type_once: None,
        insufficient_balance_once: false,
        error_after_payment_once_status: None,
        response_delay_ms: 0,
    })
    .await;

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let first_output = run_session_request(&temp, &server.url("/resource"));
    assert!(
        first_output.status.success(),
        "first request should succeed: {}",
        get_combined_output(&first_output)
    );

    let channels_after_first = load_channels(&temp);
    assert_eq!(channels_after_first.len(), 1);
    assert_eq!(
        channels_after_first[0].cumulative_amount, 5_000_000,
        "open response receipt acceptedCumulative should be persisted"
    );

    let second_output = run_session_request(&temp, &server.url("/resource"));
    assert!(
        second_output.status.success(),
        "second request should succeed: {}",
        get_combined_output(&second_output)
    );

    let observed = server.snapshot();
    assert_eq!(
        observed.open_count, 1,
        "second request should reuse existing channel"
    );
    assert_eq!(observed.voucher_count, 1);
    assert_eq!(
        observed.voucher_cumulative,
        vec![6_000_000],
        "next voucher baseline should use persisted accepted cumulative"
    );

    let channels_after_second = load_channels(&temp);
    assert_eq!(channels_after_second.len(), 1);
    assert_eq!(channels_after_second[0].cumulative_amount, 6_000_000);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it04_it05c_sse_voucher_flow_with_head_fallback_and_receipt_persistence() {
    let rpc = SessionRpcServer::start().await;
    let server = SessionServer::start(SessionServerConfig {
        payee_mode: PayeeMode::Fixed,
        open_receipt_accepted: None,
        sse_voucher_flow: true,
        voucher_head_unsupported: true,
        sse_receipt_accepted: Some(2_500_000),
        sse_required_cumulative: None,
        sse_reported_deposit: None,
        invalidating_problem_type_once: None,
        insufficient_balance_once: false,
        error_after_payment_once_status: None,
        response_delay_ms: 0,
    })
    .await;

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let first_output = run_session_request(&temp, &server.url("/stream"));
    assert!(
        first_output.status.success(),
        "first request should succeed: {}",
        get_combined_output(&first_output)
    );

    let second_output = run_session_request(&temp, &server.url("/stream"));
    assert!(
        second_output.status.success(),
        "SSE request should succeed: {}",
        get_combined_output(&second_output)
    );
    let second_stdout = String::from_utf8_lossy(&second_output.stdout);
    assert!(
        second_stdout.contains("stream"),
        "SSE message payload should be emitted to stdout: {second_stdout}"
    );

    let observed = server.snapshot();
    assert!(
        observed.voucher_head_updates >= 1,
        "SSE voucher flow should attempt HEAD transport first"
    );
    assert!(
        observed.voucher_post_updates >= 1,
        "SSE voucher flow should fall back to POST when HEAD is unsupported"
    );

    let channels = load_channels(&temp);
    assert_eq!(
        channels.len(),
        1,
        "SSE reuse should continue on the same channel"
    );
    assert_eq!(
        channels[0].cumulative_amount, 2_500_000,
        "payment-receipt SSE event acceptedCumulative should persist to channels.db"
    );
}

async fn run_invalidating_problem_reopen_case(problem_type: &'static str) {
    let rpc = SessionRpcServer::start().await;
    let server = SessionServer::start(SessionServerConfig {
        payee_mode: PayeeMode::Fixed,
        open_receipt_accepted: None,
        sse_voucher_flow: false,
        voucher_head_unsupported: false,
        sse_receipt_accepted: None,
        sse_required_cumulative: None,
        sse_reported_deposit: None,
        invalidating_problem_type_once: Some(problem_type),
        insufficient_balance_once: false,
        error_after_payment_once_status: None,
        response_delay_ms: 0,
    })
    .await;

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let first_output = run_session_request(&temp, &server.url("/resource"));
    assert!(
        first_output.status.success(),
        "first request should succeed: {}",
        get_combined_output(&first_output)
    );
    let channels_after_first = load_channels(&temp);
    assert_eq!(channels_after_first.len(), 1);
    let first_channel_id = channels_after_first[0].channel_id.clone();

    let second_output = run_session_request(&temp, &server.url("/resource"));
    assert!(
        second_output.status.success(),
        "second request should reopen cleanly: {}",
        get_combined_output(&second_output)
    );

    let observed = server.snapshot();
    assert_eq!(
        observed.invalidating_problem_count, 1,
        "problem+json 410 should be emitted exactly once"
    );
    assert_eq!(
        observed.voucher_count, 1,
        "reopen path should attempt a single voucher on the invalidated channel"
    );
    assert_eq!(
        observed.open_count, 2,
        "invalidated channel should trigger opening a replacement channel"
    );

    let channels_after_second = load_channels(&temp);
    assert_eq!(
        channels_after_second.len(),
        1,
        "invalidated local channel should be replaced, not duplicated"
    );
    assert_ne!(
        channels_after_second[0].channel_id, first_channel_id,
        "replacement session should persist a new channel id"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it07a_channel_not_found_problem_triggers_reopen() {
    run_invalidating_problem_reopen_case(
        "https://paymentauth.org/problems/session/channel-not-found",
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it07b_channel_finalized_problem_triggers_reopen() {
    run_invalidating_problem_reopen_case(
        "https://paymentauth.org/problems/session/channel-finalized",
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it07c_insufficient_balance_problem_runs_structured_top_up_recovery() {
    let rpc = SessionRpcServer::start().await;
    let server = SessionServer::start(SessionServerConfig {
        payee_mode: PayeeMode::Fixed,
        open_receipt_accepted: None,
        sse_voucher_flow: false,
        voucher_head_unsupported: false,
        sse_receipt_accepted: None,
        sse_required_cumulative: None,
        sse_reported_deposit: None,
        invalidating_problem_type_once: None,
        insufficient_balance_once: true,
        error_after_payment_once_status: None,
        response_delay_ms: 0,
    })
    .await;

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let first_output = run_session_request(&temp, &server.url("/resource"));
    assert!(
        first_output.status.success(),
        "first request should succeed: {}",
        get_combined_output(&first_output)
    );

    let second_output = run_session_request(&temp, &server.url("/resource"));
    assert!(
        second_output.status.success(),
        "second request should recover via top-up: {}",
        get_combined_output(&second_output)
    );

    let observed = server.snapshot();
    assert_eq!(
        observed.insufficient_balance_problem_count, 1,
        "insufficient-balance problem should be emitted exactly once"
    );
    assert_eq!(
        observed.top_up_count, 1,
        "client should submit one structured top-up credential"
    );
    assert_eq!(
        observed.voucher_count, 2,
        "client should retry voucher after successful top-up"
    );
    assert_eq!(
        observed.open_count, 1,
        "top-up recovery should stay on the same channel without reopening"
    );

    let channels = load_channels(&temp);
    assert_eq!(channels.len(), 1);
    assert_eq!(
        channels[0].cumulative_amount,
        SESSION_AMOUNT * 2,
        "successful post-top-up voucher should persist updated cumulative"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it08a_concurrent_same_origin_requests_do_not_double_open() {
    let rpc = SessionRpcServer::start().await;
    let server = SessionServer::start(SessionServerConfig {
        payee_mode: PayeeMode::Fixed,
        open_receipt_accepted: None,
        sse_voucher_flow: false,
        voucher_head_unsupported: false,
        sse_receipt_accepted: None,
        sse_required_cumulative: None,
        sse_reported_deposit: None,
        invalidating_problem_type_once: None,
        insufficient_balance_once: false,
        error_after_payment_once_status: None,
        response_delay_ms: 250,
    })
    .await;

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let (first_output, second_output) =
        run_two_concurrent_session_requests(&temp, &server.url("/resource"));
    assert!(
        first_output.status.success(),
        "first concurrent request should succeed: {}",
        get_combined_output(&first_output)
    );
    assert!(
        second_output.status.success(),
        "second concurrent request should succeed: {}",
        get_combined_output(&second_output)
    );

    let observed = server.snapshot();
    assert_eq!(
        observed.open_count, 1,
        "concurrent requests on same origin should not double-open"
    );
    assert_eq!(
        observed.voucher_count, 1,
        "one concurrent request should reuse the channel via voucher"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it08b_stale_lock_file_does_not_block_reuse() {
    let rpc = SessionRpcServer::start().await;
    let server = SessionServer::start(SessionServerConfig {
        payee_mode: PayeeMode::Fixed,
        open_receipt_accepted: None,
        sse_voucher_flow: false,
        voucher_head_unsupported: false,
        sse_receipt_accepted: None,
        sse_required_cumulative: None,
        sse_reported_deposit: None,
        invalidating_problem_type_once: None,
        insufficient_balance_once: false,
        error_after_payment_once_status: None,
        response_delay_ms: 0,
    })
    .await;

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let request_url = server.url("/resource");
    let origin = url::Url::parse(&request_url)
        .unwrap()
        .origin()
        .ascii_serialization();
    let lock_path = temp
        .path()
        .join(".tempo/wallet")
        .join(format!("{}.lock", session_key(&origin)));
    std::fs::create_dir_all(lock_path.parent().unwrap()).unwrap();
    std::fs::write(&lock_path, b"stale-lock-file").unwrap();

    let first_output = run_session_request(&temp, &request_url);
    assert!(
        first_output.status.success(),
        "first request should succeed with pre-existing lock file: {}",
        get_combined_output(&first_output)
    );
    let second_output = run_session_request(&temp, &request_url);
    assert!(
        second_output.status.success(),
        "second request should reuse despite stale lock file: {}",
        get_combined_output(&second_output)
    );

    let observed = server.snapshot();
    assert_eq!(
        observed.open_count, 1,
        "stale lock file should not force a second open"
    );
    assert_eq!(
        observed.voucher_count, 1,
        "channel should become reusable after stale lock file path"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it08c_concurrent_writers_preserve_single_row_and_progress_cumulative() {
    let rpc = SessionRpcServer::start().await;
    let server = SessionServer::start(SessionServerConfig {
        payee_mode: PayeeMode::Fixed,
        open_receipt_accepted: None,
        sse_voucher_flow: false,
        voucher_head_unsupported: false,
        sse_receipt_accepted: None,
        sse_required_cumulative: None,
        sse_reported_deposit: None,
        invalidating_problem_type_once: None,
        insufficient_balance_once: false,
        error_after_payment_once_status: None,
        response_delay_ms: 250,
    })
    .await;

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let (first_output, second_output) =
        run_two_concurrent_session_requests(&temp, &server.url("/resource"));
    assert!(first_output.status.success());
    assert!(second_output.status.success());

    let channels = load_channels(&temp);
    assert_eq!(
        channels.len(),
        1,
        "concurrent writers should preserve exactly one persisted channel"
    );
    assert_eq!(
        channels[0].cumulative_amount,
        SESSION_AMOUNT * 2,
        "concurrent write path should retain cumulative progression"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it16_dry_run_session_challenge_has_no_tx_no_db_write_and_shows_cost() {
    let rpc = SessionRpcServer::start().await;
    let server = SessionServer::start(SessionServerConfig {
        payee_mode: PayeeMode::Fixed,
        open_receipt_accepted: None,
        sse_voucher_flow: false,
        voucher_head_unsupported: false,
        sse_receipt_accepted: None,
        sse_required_cumulative: None,
        sse_reported_deposit: None,
        invalidating_problem_type_once: None,
        insufficient_balance_once: false,
        error_after_payment_once_status: None,
        response_delay_ms: 0,
    })
    .await;

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let output = test_command(&temp)
        .args([
            "--dry-run",
            "--private-key",
            HARDHAT_PRIVATE_KEY,
            "--network",
            "tempo-moderato",
            &server.url("/resource"),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "dry-run request should succeed: {}",
        get_combined_output(&output)
    );

    let combined = get_combined_output(&output);
    assert!(
        combined.contains("[DRY RUN] Session payment would be made:"),
        "dry-run output should include dry-run banner: {combined}"
    );
    assert!(
        combined.contains("Cost per request:"),
        "dry-run output should include cost display: {combined}"
    );

    let session_observed = server.snapshot();
    assert_eq!(
        session_observed.open_count, 0,
        "dry-run must not submit open credentials"
    );
    assert_eq!(
        session_observed.voucher_count, 0,
        "dry-run must not submit voucher credentials"
    );
    assert_eq!(
        session_observed.top_up_count, 0,
        "dry-run must not submit top-up credentials"
    );

    let rpc_observed = rpc.snapshot();
    assert_eq!(
        rpc_observed.eth_call_count, 0,
        "dry-run should avoid on-chain read RPC calls"
    );
    assert_eq!(
        rpc_observed.send_raw_count, 0,
        "dry-run should never submit transactions"
    );

    let db_path = temp.path().join(".tempo/wallet/channels.db");
    assert!(
        !db_path.exists(),
        "dry-run should not create a local session database"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it17_error_after_payment_preserves_state_and_surfaces_dispute_message() {
    let rpc = SessionRpcServer::start().await;
    let server = SessionServer::start(SessionServerConfig {
        payee_mode: PayeeMode::Fixed,
        open_receipt_accepted: None,
        sse_voucher_flow: false,
        voucher_head_unsupported: false,
        sse_receipt_accepted: None,
        sse_required_cumulative: None,
        sse_reported_deposit: None,
        invalidating_problem_type_once: None,
        insufficient_balance_once: false,
        error_after_payment_once_status: Some(500),
        response_delay_ms: 0,
    })
    .await;

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let first_output = run_session_request(&temp, &server.url("/resource"));
    assert!(
        first_output.status.success(),
        "first request should open channel: {}",
        get_combined_output(&first_output)
    );

    let second_output = run_session_request(&temp, &server.url("/resource"));
    assert!(
        !second_output.status.success(),
        "second request should fail after paid voucher path"
    );

    let combined = get_combined_output(&second_output);
    assert!(
        combined.contains("channel state preserved for on-chain dispute"),
        "error should surface preserved-state dispute message: {combined}"
    );

    let observed = server.snapshot();
    assert_eq!(
        observed.open_count, 1,
        "failure after payment should not open a replacement channel"
    );
    assert_eq!(
        observed.voucher_count, 1,
        "reuse path should send exactly one voucher before surfacing the error"
    );

    let channels = load_channels(&temp);
    assert_eq!(
        channels.len(),
        1,
        "existing channel row should be preserved"
    );
    assert_eq!(channels[0].state, "active");
    assert_eq!(
        channels[0].cumulative_amount,
        SESSION_AMOUNT * 2,
        "preserved channel state should keep the advanced cumulative amount for dispute"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it18_sse_voucher_clamps_when_required_cumulative_exceeds_deposit() {
    let rpc = SessionRpcServer::start().await;
    let server = SessionServer::start(SessionServerConfig {
        payee_mode: PayeeMode::Fixed,
        open_receipt_accepted: None,
        sse_voucher_flow: true,
        voucher_head_unsupported: true,
        sse_receipt_accepted: Some(2_200_000),
        sse_required_cumulative: Some(2_000_000),
        sse_reported_deposit: Some(1_000_000),
        invalidating_problem_type_once: None,
        insufficient_balance_once: false,
        error_after_payment_once_status: None,
        response_delay_ms: 0,
    })
    .await;

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let first_output = run_session_request(&temp, &server.url("/stream"));
    assert!(
        first_output.status.success(),
        "first stream request should open channel: {}",
        get_combined_output(&first_output)
    );

    let second_output = run_session_request(&temp, &server.url("/stream"));
    assert!(
        second_output.status.success(),
        "second stream request should recover with top-up: {}",
        get_combined_output(&second_output)
    );

    let observed = server.snapshot();
    assert_eq!(
        observed.top_up_count, 1,
        "requiredCumulative > deposit should trigger exactly one top-up in SSE flow"
    );
    assert!(
        !observed.voucher_cumulative.is_empty()
            && observed
                .voucher_cumulative
                .iter()
                .all(|amount| *amount == 2_000_000),
        "voucher cumulative retries should stay clamped at requiredCumulative: {:?}",
        observed.voucher_cumulative
    );

    let channels = load_channels(&temp);
    assert_eq!(channels.len(), 1);
    assert_eq!(
        channels[0].cumulative_amount, 2_200_000,
        "stream receipt should persist post-voucher cumulative after required>deposit flow"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it19_payment_receipt_event_terminates_stream_without_processing_trailing_events() {
    let rpc = SessionRpcServer::start().await;
    let server = SessionServer::start(SessionServerConfig {
        payee_mode: PayeeMode::Fixed,
        open_receipt_accepted: None,
        sse_voucher_flow: true,
        voucher_head_unsupported: false,
        sse_receipt_accepted: Some(2_750_000),
        sse_required_cumulative: None,
        sse_reported_deposit: None,
        invalidating_problem_type_once: None,
        insufficient_balance_once: false,
        error_after_payment_once_status: None,
        response_delay_ms: 0,
    })
    .await;

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let first_output = run_session_request(&temp, &server.url("/resource"));
    assert!(
        first_output.status.success(),
        "first request should open channel: {}",
        get_combined_output(&first_output)
    );

    let second_output = run_session_request(&temp, &server.url("/stream-receipt-tail"));
    assert!(
        second_output.status.success(),
        "stream request should succeed: {}",
        get_combined_output(&second_output)
    );

    let second_stdout = String::from_utf8_lossy(&second_output.stdout);
    assert!(
        second_stdout.contains("stream"),
        "SSE content before payment-receipt should still be emitted: {second_stdout}"
    );
    assert!(
        !second_stdout.contains("after-receipt"),
        "client must terminate on payment-receipt and ignore trailing SSE events: {second_stdout}"
    );

    let observed = server.snapshot();
    assert_eq!(
        observed.voucher_count, 2,
        "stream request should send one voucher request plus one HEAD voucher update"
    );
    assert_eq!(
        observed.voucher_head_updates, 1,
        "stream should perform a single HEAD voucher update before terminating"
    );
    assert_eq!(
        observed.voucher_post_updates, 0,
        "HEAD success path should not fallback to POST"
    );

    let channels = load_channels(&temp);
    assert_eq!(channels.len(), 1);
    assert_eq!(
        channels[0].cumulative_amount, 2_750_000,
        "payment-receipt event should persist accepted cumulative and end stream"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it20_stalled_voucher_resume_retries_with_backoff_up_to_configured_max() {
    let rpc = SessionRpcServer::start().await;
    let server = SessionServer::start(SessionServerConfig {
        payee_mode: PayeeMode::Fixed,
        open_receipt_accepted: None,
        sse_voucher_flow: true,
        voucher_head_unsupported: false,
        sse_receipt_accepted: None,
        sse_required_cumulative: None,
        sse_reported_deposit: None,
        invalidating_problem_type_once: None,
        insufficient_balance_once: false,
        error_after_payment_once_status: None,
        response_delay_ms: 0,
    })
    .await;

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let first_output = run_session_request(&temp, &server.url("/resource"));
    assert!(
        first_output.status.success(),
        "first request should open channel: {}",
        get_combined_output(&first_output)
    );

    let started = std::time::Instant::now();
    let second_output = run_session_request_with_env(
        &temp,
        &server.url("/stream-stall"),
        &[
            ("TEMPO_SESSION_MAX_VOUCHER_RETRIES", "3"),
            ("TEMPO_SESSION_STALL_TIMEOUT_MS", "20"),
            ("TEMPO_SESSION_NORMAL_TIMEOUT_MS", "80"),
        ],
    );
    let elapsed = started.elapsed();
    assert!(
        second_output.status.success(),
        "stalled stream request should finish after retry budget: {}",
        get_combined_output(&second_output)
    );

    let combined = get_combined_output(&second_output);
    assert!(
        combined.contains("Warning: missing Payment-Receipt on successful paid SSE response"),
        "stalled path should remain warning-only when retries exhaust without receipt: {combined}"
    );
    assert!(
        elapsed >= std::time::Duration::from_millis(150),
        "retry path should spend measurable time in backoff; elapsed={elapsed:?}"
    );

    let observed = server.snapshot();
    assert_eq!(
        observed.voucher_head_updates, 4,
        "configured retry budget (3) should yield one initial HEAD plus three retry HEADs"
    );
    assert_eq!(
        observed.voucher_post_updates, 0,
        "HEAD success transport should not fallback to POST"
    );
    assert_eq!(
        observed.voucher_count, 5,
        "stalled flow should submit one initial voucher request plus four voucher-update heads"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it21_required_cumulative_above_deposit_sends_topup_and_resumes_voucher_flow() {
    let rpc = SessionRpcServer::start().await;
    let server = SessionServer::start(SessionServerConfig {
        payee_mode: PayeeMode::Fixed,
        open_receipt_accepted: None,
        sse_voucher_flow: true,
        voucher_head_unsupported: true,
        sse_receipt_accepted: Some(2_500_000),
        sse_required_cumulative: Some(2_500_000),
        sse_reported_deposit: Some(500_000),
        invalidating_problem_type_once: None,
        insufficient_balance_once: false,
        error_after_payment_once_status: None,
        response_delay_ms: 0,
    })
    .await;

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let first_output = run_session_request(&temp, &server.url("/stream"));
    assert!(
        first_output.status.success(),
        "first stream request should open channel: {}",
        get_combined_output(&first_output)
    );

    let deposit_before = load_channels(&temp)
        .first()
        .map(|channel| channel.deposit)
        .unwrap_or_default();

    let second_output = run_session_request(&temp, &server.url("/stream"));
    assert!(
        second_output.status.success(),
        "second stream request should succeed after top-up recovery: {}",
        get_combined_output(&second_output)
    );

    let observed = server.snapshot();
    assert_eq!(observed.top_up_count, 1, "expected one top-up credential");
    assert_eq!(
        observed.top_up_actions,
        vec!["topUp".to_string()],
        "top-up credential should preserve the spec action field"
    );
    assert!(
        observed.voucher_count >= 3,
        "voucher flow should continue after top-up on the same request"
    );

    let channels = load_channels(&temp);
    assert_eq!(channels.len(), 1);
    assert!(
        channels[0].deposit > deposit_before,
        "local persisted deposit should increase after top-up: before={deposit_before}, after={} ",
        channels[0].deposit
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it22_challenge_request_requires_wire_recipient_field_without_local_rename_leakage() {
    let rpc = SessionRpcServer::start().await;
    let server = SessionServer::start(SessionServerConfig {
        payee_mode: PayeeMode::Fixed,
        open_receipt_accepted: None,
        sse_voucher_flow: false,
        voucher_head_unsupported: false,
        sse_receipt_accepted: None,
        sse_required_cumulative: None,
        sse_reported_deposit: None,
        invalidating_problem_type_once: None,
        insufficient_balance_once: false,
        error_after_payment_once_status: None,
        response_delay_ms: 0,
    })
    .await;

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let output = run_session_request(&temp, &server.url("/wire-payee"));
    assert!(
        !output.status.success(),
        "challenge with renamed payee field should fail parsing: {}",
        get_combined_output(&output)
    );

    let combined = get_combined_output(&output);
    assert!(
        combined.contains("missing recipient"),
        "error should clearly report missing wire recipient field: {combined}"
    );

    let observed = server.snapshot();
    assert_eq!(
        observed.open_count, 0,
        "client must not send payment credentials when recipient wire field is absent"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it23_open_and_voucher_credentials_keep_spec_field_names() {
    let rpc = SessionRpcServer::start().await;
    let server = SessionServer::start(SessionServerConfig {
        payee_mode: PayeeMode::Fixed,
        open_receipt_accepted: None,
        sse_voucher_flow: false,
        voucher_head_unsupported: false,
        sse_receipt_accepted: None,
        sse_required_cumulative: None,
        sse_reported_deposit: None,
        invalidating_problem_type_once: None,
        insufficient_balance_once: true,
        error_after_payment_once_status: None,
        response_delay_ms: 0,
    })
    .await;

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let first_output = run_session_request(&temp, &server.url("/resource"));
    assert!(
        first_output.status.success(),
        "first request should open channel: {}",
        get_combined_output(&first_output)
    );
    let second_output = run_session_request(&temp, &server.url("/resource"));
    assert!(
        second_output.status.success(),
        "second request should include voucher/top-up recovery path: {}",
        get_combined_output(&second_output)
    );

    let observed = server.snapshot();
    assert!(
        !observed.open_payload_keys.is_empty(),
        "expected captured open credential payload keys"
    );
    assert!(
        !observed.voucher_payload_keys.is_empty(),
        "expected captured voucher credential payload keys"
    );

    assert_payload_has_spec_fields(
        &observed.open_payload_keys[0],
        "open credential payload",
        &[
            "action",
            "channelId",
            "cumulativeAmount",
            "signature",
            "transaction",
        ],
    );
    assert_payload_has_spec_fields(
        &observed.voucher_payload_keys[0],
        "voucher credential payload",
        &["action", "channelId", "cumulativeAmount", "signature"],
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it24_credential_source_is_did_pkh_eip155_chainid_address() {
    let rpc = SessionRpcServer::start().await;
    let server = SessionServer::start(SessionServerConfig {
        payee_mode: PayeeMode::Fixed,
        open_receipt_accepted: None,
        sse_voucher_flow: false,
        voucher_head_unsupported: false,
        sse_receipt_accepted: None,
        sse_required_cumulative: None,
        sse_reported_deposit: None,
        invalidating_problem_type_once: None,
        insufficient_balance_once: true,
        error_after_payment_once_status: None,
        response_delay_ms: 0,
    })
    .await;

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let first_output = run_session_request(&temp, &server.url("/resource"));
    assert!(first_output.status.success());
    let second_output = run_session_request(&temp, &server.url("/resource"));
    assert!(second_output.status.success());

    let observations = server.snapshot();
    assert!(
        !observations.credential_sources.is_empty(),
        "expected at least one captured credential source"
    );
    for source in observations.credential_sources {
        assert!(
            source.starts_with("did:pkh:eip155:42431:0x"),
            "source must use did:pkh:eip155:{{chainId}}:{{address}} format: {source}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it05b_missing_receipt_on_successful_paid_response_is_warning_only() {
    let rpc = SessionRpcServer::start().await;
    let server = SessionServer::start(SessionServerConfig {
        payee_mode: PayeeMode::Fixed,
        open_receipt_accepted: None,
        sse_voucher_flow: false,
        voucher_head_unsupported: false,
        sse_receipt_accepted: None,
        sse_required_cumulative: None,
        sse_reported_deposit: None,
        invalidating_problem_type_once: None,
        insufficient_balance_once: false,
        error_after_payment_once_status: None,
        response_delay_ms: 0,
    })
    .await;

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let first_output = run_session_request(&temp, &server.url("/resource"));
    assert!(first_output.status.success());

    let second_output = run_session_request(&temp, &server.url("/resource"));
    assert!(
        second_output.status.success(),
        "missing receipt on paid voucher response should not fail request: {}",
        get_combined_output(&second_output)
    );
    let combined = get_combined_output(&second_output);
    assert!(
        combined.contains("Warning: missing Payment-Receipt on successful paid session response"),
        "client should emit warning-only message when paid response lacks receipt: {combined}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it05e_sse_initial_header_receipt_persists_before_delayed_receipt_event() {
    let rpc = SessionRpcServer::start().await;
    let server = SessionServer::start(SessionServerConfig {
        payee_mode: PayeeMode::Fixed,
        open_receipt_accepted: None,
        sse_voucher_flow: true,
        voucher_head_unsupported: false,
        sse_receipt_accepted: Some(2_800_000),
        sse_required_cumulative: Some(2_000_000),
        sse_reported_deposit: Some(3_000_000),
        invalidating_problem_type_once: None,
        insufficient_balance_once: false,
        error_after_payment_once_status: None,
        response_delay_ms: 0,
    })
    .await;

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let first_output = run_session_request(&temp, &server.url("/resource"));
    assert!(
        first_output.status.success(),
        "first request should open channel: {}",
        get_combined_output(&first_output)
    );

    let mut delayed_stream = spawn_session_request(&temp, &server.url("/stream-delayed-receipt"));
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert!(
        delayed_stream.try_wait().unwrap().is_none(),
        "delayed stream should still be in-flight before SSE payment-receipt event arrives"
    );
    assert!(
        wait_for_channel_cumulative(&temp, 2_100_000, std::time::Duration::from_millis(500)),
        "initial SSE payment-receipt header acceptedCumulative should persist before delayed receipt event"
    );

    let second_output = delayed_stream.wait_with_output().unwrap();
    assert!(
        second_output.status.success(),
        "delayed stream request should complete successfully: {}",
        get_combined_output(&second_output)
    );

    let channels = load_channels(&temp);
    assert_eq!(channels.len(), 1);
    assert_eq!(
        channels[0].cumulative_amount, 2_800_000,
        "delayed SSE payment-receipt event should still advance persisted cumulative"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it25_head_first_voucher_405_fallback_to_post_and_stream_continues() {
    let rpc = SessionRpcServer::start().await;
    let server = SessionServer::start(SessionServerConfig {
        payee_mode: PayeeMode::Fixed,
        open_receipt_accepted: None,
        sse_voucher_flow: true,
        voucher_head_unsupported: true,
        sse_receipt_accepted: Some(2_500_000),
        sse_required_cumulative: None,
        sse_reported_deposit: None,
        invalidating_problem_type_once: None,
        insufficient_balance_once: false,
        error_after_payment_once_status: None,
        response_delay_ms: 0,
    })
    .await;

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let first_output = run_session_request(&temp, &server.url("/stream"));
    assert!(
        first_output.status.success(),
        "first request should open channel: {}",
        get_combined_output(&first_output)
    );

    let second_output = run_session_request(&temp, &server.url("/stream"));
    assert!(
        second_output.status.success(),
        "second stream request should succeed via HEAD->POST fallback: {}",
        get_combined_output(&second_output)
    );
    let second_stdout = String::from_utf8_lossy(&second_output.stdout);
    assert!(
        second_stdout.contains("stream"),
        "stream output should continue after voucher transport fallback: {second_stdout}"
    );

    let observed = server.snapshot();
    assert_eq!(
        observed.voucher_head_updates, 1,
        "voucher update should attempt HEAD transport exactly once"
    );
    assert_eq!(
        observed.voucher_head_statuses,
        vec![405],
        "HEAD voucher transport should explicitly receive 405 before fallback"
    );
    assert_eq!(
        observed.voucher_post_updates, 1,
        "405 HEAD response should trigger one POST fallback voucher update"
    );
    assert_eq!(
        observed.voucher_count, 3,
        "expected one voucher request plus one HEAD and one POST voucher update"
    );

    let channels = load_channels(&temp);
    assert_eq!(channels.len(), 1);
    assert_eq!(
        channels[0].cumulative_amount, 2_500_000,
        "stream should complete successfully and persist final receipt cumulative"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it26_missing_method_details_chainid_defaults_to_moderato() {
    let rpc = SessionRpcServer::start().await;
    let server = SessionServer::start(SessionServerConfig {
        payee_mode: PayeeMode::Fixed,
        open_receipt_accepted: None,
        sse_voucher_flow: false,
        voucher_head_unsupported: false,
        sse_receipt_accepted: None,
        sse_required_cumulative: None,
        sse_reported_deposit: None,
        invalidating_problem_type_once: None,
        insufficient_balance_once: false,
        error_after_payment_once_status: None,
        response_delay_ms: 0,
    })
    .await;

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let output = run_session_request(&temp, &server.url("/missing-chain-id"));
    assert!(
        output.status.success(),
        "missing methodDetails.chainId should default to Moderato instead of failing: {}",
        get_combined_output(&output)
    );

    let observed = server.snapshot();
    assert_eq!(
        observed.open_count, 1,
        "request should still open a channel"
    );
    assert!(
        observed
            .credential_sources
            .iter()
            .any(|source| source.starts_with("did:pkh:eip155:42431:0x")),
        "fallback chainId should be reflected in credential source DID: {:?}",
        observed.credential_sources
    );

    let channels = load_channels(&temp);
    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0].state, "active");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it27_malformed_required_cumulative_fails_stream_path_deterministically() {
    let rpc = SessionRpcServer::start().await;
    let server = SessionServer::start(SessionServerConfig {
        payee_mode: PayeeMode::Fixed,
        open_receipt_accepted: None,
        sse_voucher_flow: true,
        voucher_head_unsupported: false,
        sse_receipt_accepted: None,
        sse_required_cumulative: None,
        sse_reported_deposit: None,
        invalidating_problem_type_once: None,
        insufficient_balance_once: false,
        error_after_payment_once_status: None,
        response_delay_ms: 0,
    })
    .await;

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let first_output = run_session_request(&temp, &server.url("/resource"));
    assert!(first_output.status.success());

    let second_output = run_session_request(&temp, &server.url("/stream-required-malformed"));
    assert!(
        !second_output.status.success(),
        "malformed requiredCumulative must fail stream path: {}",
        get_combined_output(&second_output)
    );
    let combined = get_combined_output(&second_output);
    assert!(
        combined.contains("payment-need-voucher.requiredCumulative")
            && combined.contains("must be an integer amount")
            && combined.contains("not-a-number"),
        "failure should clearly describe malformed requiredCumulative: {combined}"
    );

    let observed = server.snapshot();
    assert_eq!(
        observed.voucher_head_updates, 0,
        "stream must fail before issuing voucher update transport calls"
    );
    assert_eq!(
        observed.voucher_post_updates, 0,
        "stream must fail before POST fallback is considered"
    );

    let channels = load_channels(&temp);
    assert_eq!(channels.len(), 1);
    assert_eq!(
        channels[0].cumulative_amount,
        SESSION_AMOUNT * 2,
        "failed stream should preserve pre-stream voucher persistence without rollback"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it28_empty_required_cumulative_fails_stream_path_deterministically() {
    let rpc = SessionRpcServer::start().await;
    let server = SessionServer::start(SessionServerConfig {
        payee_mode: PayeeMode::Fixed,
        open_receipt_accepted: None,
        sse_voucher_flow: true,
        voucher_head_unsupported: false,
        sse_receipt_accepted: None,
        sse_required_cumulative: None,
        sse_reported_deposit: None,
        invalidating_problem_type_once: None,
        insufficient_balance_once: false,
        error_after_payment_once_status: None,
        response_delay_ms: 0,
    })
    .await;

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let first_output = run_session_request(&temp, &server.url("/resource"));
    assert!(first_output.status.success());

    let second_output = run_session_request(&temp, &server.url("/stream-required-empty"));
    assert!(
        !second_output.status.success(),
        "empty requiredCumulative must fail stream path: {}",
        get_combined_output(&second_output)
    );
    let combined = get_combined_output(&second_output);
    assert!(
        combined.contains("payment-need-voucher.requiredCumulative")
            && combined.contains("must be an integer amount"),
        "failure should clearly describe empty requiredCumulative: {combined}"
    );

    let observed = server.snapshot();
    assert_eq!(
        observed.voucher_head_updates, 0,
        "stream must fail before issuing voucher update transport calls"
    );
    assert_eq!(
        observed.voucher_post_updates, 0,
        "stream must fail before POST fallback is considered"
    );

    let channels = load_channels(&temp);
    assert_eq!(channels.len(), 1);
    assert_eq!(
        channels[0].cumulative_amount,
        SESSION_AMOUNT * 2,
        "failed stream should preserve pre-stream voucher persistence without rollback"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it29_voucher_idempotency_replay_same_or_lower_cumulative_is_successful() {
    let rpc = SessionRpcServer::start().await;
    let server = SessionServer::start(SessionServerConfig {
        payee_mode: PayeeMode::Fixed,
        open_receipt_accepted: None,
        sse_voucher_flow: true,
        voucher_head_unsupported: true,
        sse_receipt_accepted: Some(2_500_000),
        sse_required_cumulative: None,
        sse_reported_deposit: None,
        invalidating_problem_type_once: None,
        insufficient_balance_once: false,
        error_after_payment_once_status: None,
        response_delay_ms: 0,
    })
    .await;

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let first_output = run_session_request(&temp, &server.url("/stream"));
    assert!(first_output.status.success());

    let second_output = run_session_request(&temp, &server.url("/stream-idempotent-replay"));
    assert!(
        second_output.status.success(),
        "delta-too-small idempotent replay should be handled as success: {}",
        get_combined_output(&second_output)
    );
    let second_stdout = String::from_utf8_lossy(&second_output.stdout);
    assert!(
        second_stdout.contains("stream"),
        "stream should continue when voucher replay response is idempotent: {second_stdout}"
    );

    let observed = server.snapshot();
    assert_eq!(
        observed.idempotent_replay_problem_count, 1,
        "server should emit one delta-too-small replay response"
    );
    assert!(
        observed.voucher_post_updates >= 1,
        "idempotent replay path should still exercise voucher POST transport"
    );

    let channels = load_channels(&temp);
    assert_eq!(channels.len(), 1);
    assert!(
        channels[0].cumulative_amount >= 2_000_000,
        "client should preserve monotonic cumulative persistence after idempotent replay"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it30_paid_requests_include_idempotency_key_and_retry_path_stays_stable() {
    let rpc = SessionRpcServer::start().await;
    let server = SessionServer::start(SessionServerConfig {
        payee_mode: PayeeMode::Fixed,
        open_receipt_accepted: None,
        sse_voucher_flow: true,
        voucher_head_unsupported: true,
        sse_receipt_accepted: Some(2_500_000),
        sse_required_cumulative: None,
        sse_reported_deposit: None,
        invalidating_problem_type_once: None,
        insufficient_balance_once: false,
        error_after_payment_once_status: None,
        response_delay_ms: 0,
    })
    .await;

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let first_output = run_session_request(&temp, &server.url("/stream"));
    assert!(first_output.status.success());

    let second_output = run_session_request(&temp, &server.url("/stream-idempotent-replay"));
    assert!(
        second_output.status.success(),
        "duplicate processing response path should remain stable: {}",
        get_combined_output(&second_output)
    );

    let observed = server.snapshot();
    assert_eq!(
        observed.missing_idempotency_key_count, 0,
        "all paid requests should include Idempotency-Key"
    );
    assert!(
        !observed.open_idempotency_keys.is_empty(),
        "open paid requests must include Idempotency-Key"
    );
    assert!(
        !observed.voucher_idempotency_keys.is_empty(),
        "voucher paid requests must include Idempotency-Key"
    );

    let mut seen = std::collections::HashMap::<String, usize>::new();
    for key in observed.voucher_idempotency_keys {
        assert!(!key.trim().is_empty(), "Idempotency-Key must be non-empty");
        *seen.entry(key).or_default() += 1;
    }
    assert!(
        seen.values().any(|count| *count >= 2),
        "retry/fallback voucher transport should reuse the same Idempotency-Key across duplicate processing handling"
    );
}
