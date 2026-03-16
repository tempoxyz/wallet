//! Session command integration scenarios and harnesses split from commands.rs.

use std::sync::{Arc, Mutex};

use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, Response, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use mpp::{Base64UrlJson, PaymentChallenge};
use serde_json::json;

use super::*;
use crate::common::{corrupt_local_session_deposit, seed_local_session};

const MODERATO_ESCROW: &str = "0x542831e3e4ace07559b7c8787395f4fb99f70787";
const MODERATO_TOKEN: &str = "0x20c0000000000000000000000000000000000000";
const CHANNEL_OPENED_TOPIC: &str =
    "0xcd6e60364f8ee4c2b0d62afc07a1fb04fd267ce94693f93f8f85daaa099b5c94";
const SEEDED_CHANNEL_ID: &str =
    "0x0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20";
const SECOND_CHANNEL_ID: &str =
    "0x02030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f2021";
const ORPHANED_CHANNEL_ID: &str =
    "0x030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122";
const SEEDED_PAYER: &str = "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266";

struct StoredCloseState {
    state: String,
    close_requested_at: u64,
    grace_ready_at: u64,
}

#[derive(Debug, Clone, Default)]
struct CooperativeObservations {
    prefetch_count: usize,
    authorized_count: usize,
    close_channel_id: Option<String>,
    close_channel_ids: Vec<String>,
    close_cumulative_amount: Option<String>,
    close_signature: Option<String>,
    credential_source: Option<String>,
}

#[derive(Clone)]
struct CooperativeCloseState {
    realm: String,
    observations: Arc<Mutex<CooperativeObservations>>,
}

struct CooperativeCloseServer {
    base_url: String,
    observations: Arc<Mutex<CooperativeObservations>>,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    _handle: tokio::task::JoinHandle<()>,
}

impl CooperativeCloseServer {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{}:{}", addr.ip(), addr.port());
        let realm = format!("{}:{}", addr.ip(), addr.port());

        let observations = Arc::new(Mutex::new(CooperativeObservations::default()));
        let state = CooperativeCloseState {
            realm,
            observations: observations.clone(),
        };

        let app = Router::new()
            .route("/close", post(cooperative_close_handler))
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

    fn close_url(&self) -> String {
        format!("{}/close", self.base_url)
    }

    fn snapshot(&self) -> CooperativeObservations {
        self.observations.lock().unwrap().clone()
    }
}

impl Drop for CooperativeCloseServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

async fn cooperative_close_handler(
    State(state): State<CooperativeCloseState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let Some(auth_value) = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
    else {
        let request = Base64UrlJson::from_value(&json!({})).unwrap();
        let challenge =
            PaymentChallenge::new("it-close", &state.realm, "tempo", "session", request);
        let www_authenticate = mpp::format_www_authenticate(&challenge).unwrap();
        let mut observations = state.observations.lock().unwrap();
        observations.prefetch_count += 1;
        return Response::builder()
            .status(StatusCode::PAYMENT_REQUIRED)
            .header("www-authenticate", www_authenticate)
            .body(Body::empty())
            .unwrap();
    };

    let parsed = mpp::parse_authorization(auth_value).unwrap();
    let payload: mpp::protocol::methods::tempo::session::SessionCredentialPayload =
        parsed.payload_as().unwrap();
    let mut observations = state.observations.lock().unwrap();
    observations.authorized_count += 1;
    observations.credential_source = parsed.source;
    if let mpp::protocol::methods::tempo::session::SessionCredentialPayload::Close {
        channel_id,
        cumulative_amount,
        signature,
    } = payload
    {
        observations.close_channel_id = Some(channel_id.clone());
        observations.close_channel_ids.push(channel_id);
        observations.close_cumulative_amount = Some(cumulative_amount);
        observations.close_signature = Some(signature);
    }

    Response::builder()
        .status(StatusCode::OK)
        .body(Body::from("closed"))
        .unwrap()
}

#[derive(Debug, Clone, Copy)]
struct RpcCloseConfig {
    close_requested_at: u64,
    finalized: bool,
    grace_period: u64,
    orphaned_log_channel_id: Option<&'static str>,
}

#[derive(Debug, Clone, Default)]
struct RpcCloseObservations {
    send_raw_count: usize,
}

struct CloseRpcServer {
    base_url: String,
    observations: Arc<Mutex<RpcCloseObservations>>,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    _handle: tokio::task::JoinHandle<()>,
}

impl CloseRpcServer {
    async fn start(config: RpcCloseConfig) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{}:{}", addr.ip(), addr.port());

        let observations = Arc::new(Mutex::new(RpcCloseObservations::default()));
        let app = Router::new().route(
            "/",
            post({
                let observations = observations.clone();
                move |Json(body): Json<serde_json::Value>| {
                    let observations = observations.clone();
                    async move {
                        let response = if let Some(batch) = body.as_array() {
                            serde_json::Value::Array(
                                batch
                                    .iter()
                                    .map(|request| {
                                        rpc_close_response(request, &config, &observations)
                                    })
                                    .collect(),
                            )
                        } else {
                            rpc_close_response(&body, &config, &observations)
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

    fn snapshot(&self) -> RpcCloseObservations {
        self.observations.lock().unwrap().clone()
    }
}

impl Drop for CloseRpcServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

fn rpc_close_response(
    request: &serde_json::Value,
    config: &RpcCloseConfig,
    observations: &Arc<Mutex<RpcCloseObservations>>,
) -> serde_json::Value {
    let method = request["method"].as_str().unwrap_or("");
    let id = request["id"].clone();
    let result = match method {
        "eth_chainId" => json!("0xa5bf"), // 42431
        "eth_getTransactionCount" => json!("0x0"),
        "eth_blockNumber" => json!("0x100000"),
        "eth_estimateGas" => json!("0x5208"),
        "eth_maxPriorityFeePerGas" => json!("0x3b9aca00"),
        "eth_gasPrice" => json!("0x4a817c800"),
        "eth_getBalance" => json!("0xde0b6b3a7640000"),
        "eth_getLogs" => {
            if let Some(channel_id) = config.orphaned_log_channel_id {
                json!([channel_opened_log(channel_id)])
            } else {
                json!([])
            }
        }
        "eth_call" => {
            let call_input = request["params"]
                .get(0)
                .and_then(|tx| tx.get("input").or_else(|| tx.get("data")))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("0x");
            if call_input.len() <= 10 {
                json!(format!(
                    "0x{}",
                    hex::encode(encode_u64_word(config.grace_period))
                ))
            } else {
                json!(encode_get_channel_return_data(
                    config.close_requested_at,
                    config.finalized
                ))
            }
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
                "uncles": []
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

fn encode_get_channel_return_data(close_requested_at: u64, finalized: bool) -> String {
    let payer: alloy::primitives::Address = SEEDED_PAYER.parse().unwrap();
    let payee: alloy::primitives::Address = "0x0000000000000000000000000000000000000002"
        .parse()
        .unwrap();
    let token: alloy::primitives::Address = MODERATO_TOKEN.parse().unwrap();
    let authorized_signer: alloy::primitives::Address = SEEDED_PAYER.parse().unwrap();

    let mut encoded = Vec::with_capacity(32 * 8);
    encoded.extend(encode_address_word(payer));
    encoded.extend(encode_address_word(payee));
    encoded.extend(encode_address_word(token));
    encoded.extend(encode_address_word(authorized_signer));
    encoded.extend(encode_u128_word(10_000_000));
    encoded.extend(encode_u128_word(0));
    encoded.extend(encode_u64_word(close_requested_at));
    encoded.extend(encode_bool_word(finalized));
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

fn topic_from_address(address: alloy::primitives::Address) -> String {
    format!("0x{}", hex::encode(encode_address_word(address)))
}

fn channel_opened_log(channel_id: &str) -> serde_json::Value {
    let payer: alloy::primitives::Address = SEEDED_PAYER.parse().unwrap();
    let payee: alloy::primitives::Address = "0x0000000000000000000000000000000000000002"
        .parse()
        .unwrap();
    let token: alloy::primitives::Address = MODERATO_TOKEN.parse().unwrap();

    let mut data = Vec::with_capacity(96);
    data.extend(encode_address_word(token));
    data.extend(encode_u128_word(10_000_000));
    data.extend([0u8; 32]);

    json!({
        "removed": false,
        "address": MODERATO_ESCROW,
        "data": format!("0x{}", hex::encode(data)),
        "topics": [
            CHANNEL_OPENED_TOPIC,
            channel_id,
            topic_from_address(payer),
            topic_from_address(payee),
        ],
        "blockNumber": "0x100000",
        "transactionHash": "0x0000000000000000000000000000000000000000000000000000000000000002",
        "transactionIndex": "0x0",
        "blockHash": "0x0000000000000000000000000000000000000000000000000000000000000003",
        "logIndex": "0x0"
    })
}

fn seed_session_for_close(
    temp: &tempfile::TempDir,
    origin: &str,
    request_url: &str,
    cumulative_amount: u128,
) {
    seed_local_session(temp, origin);
    let challenge_request = Base64UrlJson::from_value(&json!({})).unwrap();
    let challenge = PaymentChallenge::new(
        "it-close",
        "close.test",
        "tempo",
        "session",
        challenge_request,
    );
    let challenge_echo = serde_json::to_string(&challenge.to_echo()).unwrap();
    let db_path = temp.path().join(".tempo/wallet/channels.db");
    let conn = rusqlite::Connection::open(db_path).unwrap();
    conn.execute(
        "UPDATE channels
         SET request_url = ?1,
             chain_id = 42431,
             escrow_contract = ?2,
             token = ?3,
             payer = ?4,
             authorized_signer = ?4,
             cumulative_amount = ?5,
             challenge_echo = ?6,
             state = 'active',
             close_requested_at = 0,
             grace_ready_at = 0
         WHERE channel_id = ?7",
        rusqlite::params![
            request_url,
            MODERATO_ESCROW,
            MODERATO_TOKEN,
            SEEDED_PAYER,
            cumulative_amount.to_string(),
            challenge_echo,
            SEEDED_CHANNEL_ID,
        ],
    )
    .unwrap();
}

fn insert_session_for_close(
    temp: &tempfile::TempDir,
    channel_id: &str,
    origin: &str,
    request_url: &str,
    cumulative_amount: u128,
) {
    let challenge_request = Base64UrlJson::from_value(&json!({})).unwrap();
    let challenge = PaymentChallenge::new(
        "it-close",
        "close.test",
        "tempo",
        "session",
        challenge_request,
    );
    let challenge_echo = serde_json::to_string(&challenge.to_echo()).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let db_path = temp.path().join(".tempo/wallet/channels.db");
    let conn = rusqlite::Connection::open(db_path).unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO channels (
            channel_id,
            version,
            origin,
            request_url,
            chain_id,
            escrow_contract,
            token,
            payee,
            payer,
            authorized_signer,
            salt,
            deposit,
            cumulative_amount,
            challenge_echo,
            state,
            close_requested_at,
            grace_ready_at,
            created_at,
            last_used_at
        ) VALUES (?1, 1, ?2, ?3, 42431, ?4, ?5, ?6, ?7, ?7, ?8, ?9, ?10, ?11, 'active', 0, 0, ?12, ?12)",
        rusqlite::params![
            channel_id,
            origin,
            request_url,
            MODERATO_ESCROW,
            MODERATO_TOKEN,
            "0x0000000000000000000000000000000000000002",
            SEEDED_PAYER,
            "0x00",
            "1000000",
            cumulative_amount.to_string(),
            challenge_echo,
            now,
        ],
    )
    .unwrap();
}

fn read_close_state(temp: &tempfile::TempDir) -> Option<StoredCloseState> {
    let db_path = temp.path().join(".tempo/wallet/channels.db");
    let conn = rusqlite::Connection::open(db_path).unwrap();
    conn.query_row(
        "SELECT state, close_requested_at, grace_ready_at
         FROM channels
         WHERE channel_id = ?1",
        [SEEDED_CHANNEL_ID],
        |row| {
            Ok(StoredCloseState {
                state: row.get(0)?,
                close_requested_at: row.get::<_, u64>(1)?,
                grace_ready_at: row.get::<_, u64>(2)?,
            })
        },
    )
    .ok()
}

fn session_row_count(temp: &tempfile::TempDir) -> u64 {
    let db_path = temp.path().join(".tempo/wallet/channels.db");
    let conn = rusqlite::Connection::open(db_path).unwrap();
    conn.query_row("SELECT COUNT(*) FROM channels", [], |row| {
        row.get::<_, u64>(0)
    })
    .unwrap()
}

// ==================== sessions ====================

#[test]
fn sessions_list_empty_returns_expected_json_shape() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["-j", "sessions", "list"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(parsed["sessions"].is_array());
    assert!(parsed["sessions"]
        .as_array()
        .is_some_and(std::vec::Vec::is_empty));
    assert_eq!(parsed["total"], 0);
}

#[test]
fn sessions_list_seeded_channel_returns_expected_json_shape() {
    let temp = TestConfigBuilder::new().build();
    seed_local_session(&temp, "https://api.example.com");

    let output = test_command(&temp)
        .args(["-j", "sessions", "list"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["total"], 1);
    assert!(parsed["sessions"].is_array());
    let session = &parsed["sessions"][0];
    assert!(session["channel_id"].is_string());
    assert_eq!(session["origin"], "https://api.example.com");
    assert!(session["deposit"].is_string());
    assert!(session["spent"].is_string());
    assert_eq!(session["status"], "active");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sessions_list_state_all_includes_orphaned_channels() {
    let rpc = CloseRpcServer::start(RpcCloseConfig {
        close_requested_at: 0,
        finalized: false,
        grace_period: 900,
        orphaned_log_channel_id: Some(ORPHANED_CHANNEL_ID),
    })
    .await;
    let temp = TestConfigBuilder::new()
        .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
        .with_config_toml(format!(
            "[rpc]\n\"tempo-moderato\" = \"{}\"\n",
            rpc.base_url
        ))
        .build();

    let output = test_command(&temp)
        .args([
            "-j",
            "-n",
            "tempo-moderato",
            "sessions",
            "list",
            "--state",
            "all",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let sessions = parsed["sessions"]
        .as_array()
        .expect("sessions should be array");
    let orphaned = sessions
        .iter()
        .find(|item| item["channel_id"] == ORPHANED_CHANNEL_ID)
        .expect("expected orphaned channel in --state all output");
    assert_eq!(orphaned["status"], "orphaned");
    assert!(
        orphaned.get("origin").is_none(),
        "orphaned channel should not include local origin: {orphaned}"
    );
}

#[test]
fn sessions_sync_empty_json() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["-j", "sessions", "sync"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(parsed["sessions"].is_array());
    assert_eq!(parsed["total"], 0);
}

#[test]
fn sessions_list_emits_degraded_event_for_malformed_session_row() {
    let temp = TestConfigBuilder::new().build();
    seed_local_session(&temp, "https://api.example.com");
    corrupt_local_session_deposit(&temp, "https://api.example.com", "not-a-number");

    let events_path = temp.path().join("events_session_degraded.log");
    let output = test_command(&temp)
        .env("TEMPO_TEST_EVENTS", events_path.to_str().unwrap())
        .args(["-j", "sessions", "list"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let events = parse_events_log(&events_path);
    let payload = events
        .iter()
        .find(|(name, _)| name == "session store degraded")
        .map_or_else(
            || panic!("missing session store degraded event: {events:?}"),
            |(_, payload)| payload,
        );

    assert!(
        payload["malformed_list_drops"].as_u64().unwrap_or(0) >= 1,
        "expected malformed_list_drops >= 1, got: {payload}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sessions_close_cooperative_credential_shape_and_did_source() {
    let rpc = CloseRpcServer::start(RpcCloseConfig {
        close_requested_at: 0,
        finalized: false,
        grace_period: 900,
        orphaned_log_channel_id: None,
    })
    .await;
    let close_server = CooperativeCloseServer::start().await;
    let temp = TestConfigBuilder::new()
        .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
        .with_config_toml(format!(
            "[rpc]\n\"tempo-moderato\" = \"{}\"\n",
            rpc.base_url
        ))
        .build();

    seed_session_for_close(
        &temp,
        &close_server.base_url,
        &close_server.close_url(),
        4242,
    );

    let output = test_command(&temp)
        .args([
            "-j",
            "-n",
            "tempo-moderato",
            "sessions",
            "close",
            &close_server.base_url,
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "close should succeed: {}",
        get_combined_output(&output)
    );

    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("json output should parse");
    assert_eq!(parsed["closed"], 1);
    assert_eq!(parsed["pending"], 0);
    assert_eq!(parsed["failed"], 0);

    let observed = close_server.snapshot();
    assert_eq!(
        observed.prefetch_count, 1,
        "close flow should prefetch challenge once"
    );
    assert_eq!(
        observed.authorized_count, 1,
        "close flow should submit exactly one close credential"
    );
    assert_eq!(
        observed.close_channel_id.as_deref(),
        Some(SEEDED_CHANNEL_ID)
    );
    assert_eq!(observed.close_cumulative_amount.as_deref(), Some("4242"));
    assert!(
        observed
            .close_signature
            .as_deref()
            .is_some_and(|signature| signature.starts_with("0x")),
        "close signature should be present and hex encoded: {observed:?}"
    );
    assert_eq!(
        observed.credential_source.as_deref(),
        Some("did:pkh:eip155:42431:0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266"),
        "close credential source should be DID derived from payer"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sessions_close_request_close_transitions_to_pending_and_persists_countdown() {
    let rpc = CloseRpcServer::start(RpcCloseConfig {
        close_requested_at: 0,
        finalized: false,
        grace_period: 900,
        orphaned_log_channel_id: None,
    })
    .await;
    let temp = TestConfigBuilder::new()
        .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
        .with_config_toml(format!(
            "[rpc]\n\"tempo-moderato\" = \"{}\"\n",
            rpc.base_url
        ))
        .build();
    seed_session_for_close(
        &temp,
        "https://close.example",
        "http://127.0.0.1:1/unreachable",
        777,
    );

    let output = test_command(&temp)
        .args([
            "-j",
            "-n",
            "tempo-moderato",
            "sessions",
            "close",
            "https://close.example",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "close should succeed with pending outcome: {}",
        get_combined_output(&output)
    );

    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("json output should parse");
    assert_eq!(parsed["closed"], 0);
    assert_eq!(parsed["pending"], 1);
    assert_eq!(parsed["failed"], 0);
    assert_eq!(parsed["results"][0]["status"], "pending");
    assert_eq!(parsed["results"][0]["remaining_secs"], 900);

    let close_state = read_close_state(&temp).expect("pending close should keep local row");
    assert_eq!(close_state.state, "closing");
    assert!(close_state.close_requested_at > 0);
    assert_eq!(
        close_state.grace_ready_at,
        close_state.close_requested_at + 900,
        "pending close should persist grace countdown"
    );

    let observed = rpc.snapshot();
    assert_eq!(
        observed.send_raw_count, 1,
        "requestClose branch should submit exactly one tx"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sessions_close_withdraw_after_grace_elapsed() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let rpc = CloseRpcServer::start(RpcCloseConfig {
        close_requested_at: now.saturating_sub(1_000),
        finalized: false,
        grace_period: 900,
        orphaned_log_channel_id: None,
    })
    .await;
    let temp = TestConfigBuilder::new()
        .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
        .with_config_toml(format!(
            "[rpc]\n\"tempo-moderato\" = \"{}\"\n",
            rpc.base_url
        ))
        .build();
    seed_session_for_close(
        &temp,
        "https://close.example",
        "http://127.0.0.1:1/unreachable",
        777,
    );

    let output = test_command(&temp)
        .args([
            "-j",
            "-n",
            "tempo-moderato",
            "sessions",
            "close",
            "https://close.example",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "close should succeed with closed outcome: {}",
        get_combined_output(&output)
    );

    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("json output should parse");
    assert_eq!(parsed["closed"], 1);
    assert_eq!(parsed["pending"], 0);
    assert_eq!(parsed["failed"], 0);
    assert_eq!(parsed["results"][0]["status"], "closed");

    assert!(
        read_close_state(&temp).is_none(),
        "closed outcome should remove local session row"
    );
    let observed = rpc.snapshot();
    assert_eq!(
        observed.send_raw_count, 1,
        "withdraw branch should submit exactly one tx"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sessions_close_pending_before_grace_elapsed_submits_no_tx() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let close_requested_at = now.saturating_sub(100);
    let rpc = CloseRpcServer::start(RpcCloseConfig {
        close_requested_at,
        finalized: false,
        grace_period: 900,
        orphaned_log_channel_id: None,
    })
    .await;
    let temp = TestConfigBuilder::new()
        .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
        .with_config_toml(format!(
            "[rpc]\n\"tempo-moderato\" = \"{}\"\n",
            rpc.base_url
        ))
        .build();
    seed_session_for_close(
        &temp,
        "https://close.example",
        "http://127.0.0.1:1/unreachable",
        777,
    );

    let output = test_command(&temp)
        .args([
            "-j",
            "-n",
            "tempo-moderato",
            "sessions",
            "close",
            "https://close.example",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "close should succeed with pending outcome: {}",
        get_combined_output(&output)
    );

    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("json output should parse");
    assert_eq!(parsed["closed"], 0);
    assert_eq!(parsed["pending"], 1);
    assert_eq!(parsed["failed"], 0);
    assert_eq!(parsed["results"][0]["status"], "pending");
    let remaining_secs = parsed["results"][0]["remaining_secs"]
        .as_u64()
        .expect("pending close should include remaining_secs");
    assert!(
        (799..=800).contains(&remaining_secs),
        "remaining seconds should reflect pending grace window: {remaining_secs}"
    );

    let close_state = read_close_state(&temp).expect("pending close should keep local row");
    assert_eq!(close_state.state, "closing");
    assert_eq!(close_state.close_requested_at, close_requested_at);
    assert_eq!(close_state.grace_ready_at, close_requested_at + 900);

    let observed = rpc.snapshot();
    assert_eq!(
        observed.send_raw_count, 0,
        "pending branch must not submit a close tx before grace elapses"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sessions_close_channel_id_exercises_onchain_close_path() {
    let rpc = CloseRpcServer::start(RpcCloseConfig {
        close_requested_at: 0,
        finalized: false,
        grace_period: 900,
        orphaned_log_channel_id: None,
    })
    .await;
    let temp = TestConfigBuilder::new()
        .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
        .with_config_toml(format!(
            "[rpc]\n\"tempo-moderato\" = \"{}\"\n",
            rpc.base_url
        ))
        .build();

    let output = test_command(&temp)
        .args([
            "-j",
            "-n",
            "tempo-moderato",
            "sessions",
            "close",
            ORPHANED_CHANNEL_ID,
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "close by channel ID should succeed: {}",
        get_combined_output(&output)
    );

    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("json output should parse");
    assert_eq!(parsed["closed"], 0);
    assert_eq!(parsed["pending"], 1);
    assert_eq!(parsed["failed"], 0);
    assert_eq!(parsed["results"][0]["channel_id"], ORPHANED_CHANNEL_ID);
    assert_eq!(parsed["results"][0]["status"], "pending");

    let observed = rpc.snapshot();
    assert_eq!(
        observed.send_raw_count, 1,
        "on-chain channel-ID close should submit exactly one requestClose tx"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sessions_close_all_closes_multiple_local_sessions() {
    let rpc = CloseRpcServer::start(RpcCloseConfig {
        close_requested_at: 0,
        finalized: false,
        grace_period: 900,
        orphaned_log_channel_id: None,
    })
    .await;
    let close_server = CooperativeCloseServer::start().await;
    let temp = TestConfigBuilder::new()
        .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
        .with_config_toml(format!(
            "[rpc]\n\"tempo-moderato\" = \"{}\"\n",
            rpc.base_url
        ))
        .build();

    seed_session_for_close(
        &temp,
        &close_server.base_url,
        &close_server.close_url(),
        4242,
    );
    insert_session_for_close(
        &temp,
        SECOND_CHANNEL_ID,
        "https://close-two.example",
        &close_server.close_url(),
        7777,
    );

    let output = test_command(&temp)
        .args(["-j", "-n", "tempo-moderato", "sessions", "close", "--all"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "close --all should succeed: {}",
        get_combined_output(&output)
    );

    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("json output should parse");
    assert_eq!(parsed["closed"], 2);
    assert_eq!(parsed["pending"], 0);
    assert_eq!(parsed["failed"], 0);
    assert_eq!(
        session_row_count(&temp),
        0,
        "closed sessions should be removed locally"
    );

    let observed = close_server.snapshot();
    assert_eq!(observed.prefetch_count, 2);
    assert_eq!(observed.authorized_count, 2);
    assert!(
        observed
            .close_channel_ids
            .contains(&SEEDED_CHANNEL_ID.to_string()),
        "first channel should be closed cooperatively: {observed:?}"
    );
    assert!(
        observed
            .close_channel_ids
            .contains(&SECOND_CHANNEL_ID.to_string()),
        "second channel should be closed cooperatively: {observed:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sessions_sync_origin_reconciles_closing_state_and_grace_ready_at() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let close_requested_at = now.saturating_sub(60);
    let rpc = CloseRpcServer::start(RpcCloseConfig {
        close_requested_at,
        finalized: false,
        grace_period: 900,
        orphaned_log_channel_id: None,
    })
    .await;
    let temp = TestConfigBuilder::new()
        .with_config_toml(format!(
            "[rpc]\n\"tempo-moderato\" = \"{}\"\n",
            rpc.base_url
        ))
        .build();
    seed_session_for_close(
        &temp,
        "https://sync.example",
        "http://127.0.0.1:1/unreachable",
        777,
    );

    let output = test_command(&temp)
        .args([
            "-j",
            "-n",
            "tempo-moderato",
            "sessions",
            "sync",
            "--origin",
            "https://sync.example",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "sync --origin should succeed: {}",
        get_combined_output(&output)
    );

    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("json output should parse");
    assert_eq!(parsed["recovered"], true);
    assert_eq!(parsed["status"], "closing");
    assert!(
        parsed["remaining_secs"]
            .as_u64()
            .is_some_and(|remaining| remaining > 0 && remaining <= 900),
        "sync should report remaining close grace period: {parsed}"
    );

    let close_state =
        read_close_state(&temp).expect("sync should keep local row and update close state");
    assert_eq!(close_state.state, "closing");
    assert_eq!(close_state.close_requested_at, close_requested_at);
    assert_eq!(close_state.grace_ready_at, close_requested_at + 900);
}
