//! Shared harness for session integration scenarios.

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
use tempo_common::keys::parse_private_key_signer;

use crate::common::test_command;
use tempo_test::MODERATO_PRIVATE_KEY;

pub(crate) const MODERATO_ESCROW: &str = "0x542831e3e4ace07559b7c8787395f4fb99f70787";
pub(crate) const MODERATO_TOKEN: &str = "0x20c0000000000000000000000000000000000000";
pub(crate) const PAYEE_A: &str = "0x1111111111111111111111111111111111111111";
pub(crate) const PAYEE_B: &str = "0x2222222222222222222222222222222222222222";
pub(crate) const SESSION_AMOUNT: u128 = 1_000_000;

fn challenge_amount_for_path(path: &str) -> u128 {
    if path.contains("amount-3x") {
        SESSION_AMOUNT.saturating_mul(3)
    } else if path.contains("amount-2x") {
        SESSION_AMOUNT.saturating_mul(2)
    } else {
        SESSION_AMOUNT
    }
}

#[derive(Debug, Clone)]
pub(crate) struct StoredChannel {
    pub(crate) channel_id: String,
    pub(crate) payee: String,
    pub(crate) state: String,
    pub(crate) deposit: u128,
    pub(crate) cumulative_amount: u128,
}

#[derive(Debug, Default)]
/// Captured interactions from the mock payment server used by assertions.
pub(crate) struct SessionObservations {
    pub(crate) open_count: usize,
    pub(crate) voucher_count: usize,
    pub(crate) top_up_count: usize,
    pub(crate) top_up_actions: Vec<String>,
    pub(crate) credential_sources: Vec<String>,
    pub(crate) open_payload_keys: Vec<Vec<String>>,
    pub(crate) voucher_payload_keys: Vec<Vec<String>>,
    pub(crate) invalidating_problem_count: usize,
    pub(crate) insufficient_balance_problem_count: usize,
    pub(crate) idempotent_replay_problem_count: usize,
    pub(crate) missing_idempotency_key_count: usize,
    pub(crate) open_idempotency_keys: Vec<String>,
    pub(crate) voucher_idempotency_keys: Vec<String>,
    pub(crate) top_up_idempotency_keys: Vec<String>,
    pub(crate) open_transactions: Vec<String>,
    pub(crate) voucher_cumulative: Vec<u128>,
    pub(crate) voucher_paths: Vec<String>,
    pub(crate) top_up_paths: Vec<String>,
    pub(crate) voucher_head_updates: usize,
    pub(crate) voucher_head_statuses: Vec<u16>,
    pub(crate) voucher_post_updates: usize,
    pub(crate) unauth_head_count: usize,
    pub(crate) unauth_non_head_count: usize,
    pub(crate) top_up_challenge_not_found_problem_count: usize,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum PayeeMode {
    Fixed,
    ByPath,
}

#[derive(Debug)]
pub(crate) struct SessionServerConfig {
    pub(crate) payee_mode: PayeeMode,
    pub(crate) open_receipt_accepted: Option<u128>,
    pub(crate) sse_voucher_flow: bool,
    pub(crate) voucher_head_unsupported: bool,
    pub(crate) sse_receipt_accepted: Option<u128>,
    pub(crate) sse_required_cumulative: Option<u128>,
    pub(crate) sse_reported_deposit: Option<u128>,
    pub(crate) invalidating_problem_type_once: Option<&'static str>,
    pub(crate) insufficient_balance_once: bool,
    pub(crate) error_after_payment_once_status: Option<u16>,
    pub(crate) response_delay_ms: u64,
}

#[derive(Clone)]
struct SessionServerState {
    realm: String,
    config: Arc<SessionServerConfig>,
    observations: Arc<Mutex<SessionObservations>>,
}

pub(crate) struct SessionServer {
    pub(crate) base_url: String,
    observations: Arc<Mutex<SessionObservations>>,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    _handle: tokio::task::JoinHandle<()>,
}

pub(crate) struct SessionRpcServer {
    pub(crate) base_url: String,
    observations: Arc<Mutex<SessionRpcObservations>>,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    _handle: tokio::task::JoinHandle<()>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SessionRpcObservations {
    pub(crate) eth_call_count: usize,
    pub(crate) send_raw_count: usize,
}

impl SessionServer {
    pub(crate) async fn start(config: SessionServerConfig) -> Self {
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

    pub(crate) fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    pub(crate) fn snapshot(&self) -> SessionObservations {
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
    pub(crate) async fn start() -> Self {
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

    pub(crate) fn snapshot(&self) -> SessionRpcObservations {
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
    let payer = parse_private_key_signer(MODERATO_PRIVATE_KEY)
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
    // This single handler emulates the full session-intent lifecycle and path-specific edge cases.
    let path = request.uri().path().to_string();
    let method = request.method().clone();
    let challenge_amount = challenge_amount_for_path(&path);
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
            "amount": challenge_amount.to_string(),
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

pub(crate) fn run_session_request(temp: &tempfile::TempDir, url: &str) -> std::process::Output {
    run_session_request_with_env(temp, url, &[])
}

pub(crate) fn run_session_request_with_env(
    temp: &tempfile::TempDir,
    url: &str,
    envs: &[(&str, &str)],
) -> std::process::Output {
    // Use the real CLI binary to keep this harness as close as possible to production behavior.
    test_command(temp)
        .envs(envs.iter().copied())
        .args([
            "--private-key",
            MODERATO_PRIVATE_KEY,
            "--network",
            "tempo-moderato",
            url,
        ])
        .output()
        .unwrap()
}

pub(crate) fn spawn_session_request(temp: &tempfile::TempDir, url: &str) -> std::process::Child {
    spawn_session_request_with_env(temp, url, &[])
}

pub(crate) fn spawn_session_request_with_env(
    temp: &tempfile::TempDir,
    url: &str,
    envs: &[(&str, &str)],
) -> std::process::Child {
    test_command(temp)
        .envs(envs.iter().copied())
        .args([
            "--private-key",
            MODERATO_PRIVATE_KEY,
            "--network",
            "tempo-moderato",
            url,
        ])
        .spawn()
        .unwrap()
}

pub(crate) fn load_channels(temp: &tempfile::TempDir) -> Vec<StoredChannel> {
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

pub(crate) fn set_all_channel_state(temp: &tempfile::TempDir, state: &str) {
    let db_path = temp.path().join(".tempo/wallet/channels.db");
    let conn = Connection::open(db_path).unwrap();
    conn.execute("UPDATE channels SET state = ?1", [state])
        .unwrap();
}

pub(crate) fn wait_for_channel_cumulative(
    temp: &tempfile::TempDir,
    expected: u128,
    timeout: std::time::Duration,
) -> bool {
    // Poll storage because receipt/event ordering tests race with process and stream completion.
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

pub(crate) fn run_two_concurrent_session_requests(
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

pub(crate) fn assert_payload_has_spec_fields(keys: &[String], context: &str, fields: &[&str]) {
    for field in fields {
        assert!(
            keys.iter().any(|key| key == field),
            "{context} should include spec field '{field}', observed keys={keys:?}"
        );
    }
}
