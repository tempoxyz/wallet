//! Integration tests for tempo-wallet commands.

mod common;

use std::sync::{Arc, Mutex};

use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, Response, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use common::{
    assert_exit_code, corrupt_local_session_deposit, get_combined_output, seed_local_session,
    test_command, MockServicesServer, TestConfigBuilder, MODERATO_DIRECT_KEYS_TOML,
};
use mpp::{Base64UrlJson, PaymentChallenge};
use serde_json::json;

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

fn parse_events_log(path: &std::path::Path) -> Vec<(String, serde_json::Value)> {
    let content = std::fs::read_to_string(path).unwrap_or_default();
    content
        .lines()
        .filter_map(|line| {
            let (name, json_str) = line.split_once('|')?;
            let value: serde_json::Value = serde_json::from_str(json_str).ok()?;
            Some((name.to_string(), value))
        })
        .collect()
}

#[derive(Debug)]
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

// ==================== whoami ====================

#[test]
fn whoami_no_wallet_shows_not_logged_in() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp).arg("whoami").output().unwrap();

    assert!(
        output.status.success(),
        "whoami should succeed even without wallet"
    );
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("Not logged in") || combined.contains("not logged in"),
        "should mention not logged in: {combined}"
    );
}

#[test]
fn whoami_no_wallet_json_shape() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp).args(["-j", "whoami"]).output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["ready"], false, "should not be ready: {parsed}");
}

#[test]
fn whoami_with_wallet_json_has_wallet_field() {
    let temp = TestConfigBuilder::new()
        .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
        .build();

    let output = test_command(&temp).args(["-j", "whoami"]).output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(
        parsed["wallet"].is_string(),
        "should have wallet field: {parsed}"
    );
    assert!(
        parsed["wallet"].as_str().unwrap().starts_with("0x"),
        "wallet should be an address: {parsed}"
    );
}

#[test]
fn whoami_with_wallet_toon_shape() {
    let temp = TestConfigBuilder::new()
        .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
        .build();

    let output = test_command(&temp).args(["-t", "whoami"]).output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = toon_format::decode_default(stdout.trim()).unwrap();
    assert!(
        parsed["wallet"].is_string(),
        "TOON should have wallet: {parsed}"
    );
}

#[test]
fn whoami_emits_keystore_degraded_event_for_malformed_keys_file() {
    let temp = TestConfigBuilder::new().build();
    let keys_path = temp.path().join(".tempo/wallet/keys.toml");
    std::fs::write(&keys_path, "this-is-not-valid-toml = [").unwrap();

    let events_path = temp.path().join("events_keystore_degraded.log");
    let output = test_command(&temp)
        .env("TEMPO_TEST_EVENTS", events_path.to_str().unwrap())
        .arg("whoami")
        .output()
        .unwrap();

    assert!(output.status.success());
    let events = parse_events_log(&events_path);
    let payload = events
        .iter()
        .find(|(name, _)| name == "keystore load degraded")
        .map_or_else(
            || panic!("missing keystore load degraded event: {events:?}"),
            |(_, payload)| payload,
        );

    assert!(
        payload["strict_parse_failures"].as_u64().unwrap_or(0) >= 1,
        "expected strict_parse_failures >= 1, got: {payload}"
    );
}

// ==================== logout ====================

#[test]
fn logout_no_wallet_succeeds() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["logout", "--yes"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("Not logged in") || combined.contains("not logged in"),
        "should mention not logged in: {combined}"
    );
}

#[test]
fn logout_no_wallet_json_shape() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["-j", "logout", "--yes"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["logged_in"], false);
    assert_eq!(parsed["disconnected"], false);
}

#[test]
fn logout_without_yes_in_non_interactive_mode_requires_confirmation_flag() {
    use std::process::Stdio;

    let passkey_keys = r#"
[[keys]]
wallet_type = "passkey"
wallet_address = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
key_address = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
key = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
chain_id = 42431
"#;
    let temp = TestConfigBuilder::new()
        .with_keys_toml(passkey_keys)
        .build();
    let mut child = test_command(&temp)
        .arg("logout")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn logout command");

    drop(child.stdin.take());
    let output = child
        .wait_with_output()
        .expect("failed to wait for logout command");

    assert_exit_code(
        &output,
        2,
        "non-interactive logout without --yes should exit with E_USAGE",
    );
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("Use --yes for non-interactive mode"),
        "expected non-interactive confirmation guidance: {combined}"
    );
}

// ==================== keys ====================

#[test]
fn keys_empty() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp).arg("keys").output().unwrap();

    assert!(output.status.success());
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("No keys") || combined.contains("0 key"),
        "should mention no keys: {combined}"
    );
}

#[test]
fn keys_json_shape() {
    let temp = TestConfigBuilder::new()
        .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
        .build();

    let output = test_command(&temp).args(["-j", "keys"]).output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(parsed["keys"].is_array());
    assert!(parsed["total"].as_u64().unwrap() >= 1);
    let key = &parsed["keys"][0];
    assert!(key["address"].is_string());
    assert!(key["key"].is_string(), "JSON should include private key");
}

#[test]
fn keys_toon_shape() {
    let temp = TestConfigBuilder::new()
        .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
        .build();

    let output = test_command(&temp).args(["-t", "keys"]).output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = toon_format::decode_default(stdout.trim()).unwrap();
    assert!(parsed["keys"].is_array());
    assert!(parsed["total"].as_u64().unwrap() >= 1);
}

#[test]
fn mixed_case_keys_are_canonicalized_in_output() {
    let mixed_case_keys = r#"
[[keys]]
wallet_address = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
key_address = "0xF39fD6E51Aad88f6f4ce6AB8827279cfFFb92266"
key = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
chain_id = 42431
"#;
    let temp = TestConfigBuilder::new()
        .with_keys_toml(mixed_case_keys)
        .build();

    let whoami = test_command(&temp).args(["-j", "whoami"]).output().unwrap();
    assert!(whoami.status.success());
    let whoami_json: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&whoami.stdout).trim()).unwrap();
    assert_eq!(
        whoami_json["wallet"].as_str(),
        Some("0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266")
    );

    let keys = test_command(&temp).args(["-j", "keys"]).output().unwrap();
    assert!(keys.status.success());
    let keys_json: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&keys.stdout).trim()).unwrap();
    assert_eq!(
        keys_json["keys"][0]["wallet_address"].as_str(),
        Some("0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266")
    );
    assert_eq!(
        keys_json["keys"][0]["address"].as_str(),
        Some("0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266")
    );
}

// ==================== sessions ====================

#[test]
fn sessions_list_it10_empty_returns_expected_json_shape() {
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
fn sessions_list_it09_seeded_channel_returns_expected_json_shape() {
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
async fn sessions_list_it11_state_all_includes_orphaned_channels() {
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
fn sessions_list_it12_emits_degraded_event_for_malformed_session_row() {
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
async fn sessions_close_it06a_cooperative_credential_shape_and_did_source() {
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
async fn sessions_close_it06b_request_close_transitions_to_pending_and_persists_countdown() {
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
async fn sessions_close_it06c_withdraw_after_grace_elapsed() {
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
async fn sessions_close_it06d_pending_before_grace_elapsed_submits_no_tx() {
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
async fn sessions_close_it13_channel_id_exercises_onchain_close_path() {
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
async fn sessions_close_it14_all_closes_multiple_local_sessions() {
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
async fn sessions_sync_it15_origin_reconciles_closing_state_and_grace_ready_at() {
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

// ==================== services ====================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn services_category_filter() {
    let mock = MockServicesServer::start().await;
    let temp = TestConfigBuilder::new().build();

    // Filter by existing category
    let output = test_command(&temp)
        .env("TEMPO_SERVICES_URL", &mock.services_url)
        .args(["-j", "services", "--search", "ai"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(parsed.as_array().is_some_and(|a| !a.is_empty()));

    // Filter by non-existent category
    let output = test_command(&temp)
        .env("TEMPO_SERVICES_URL", &mock.services_url)
        .args(["-j", "services", "--search", "nonexistent"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(parsed.as_array().is_some_and(std::vec::Vec::is_empty));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn services_search_filter() {
    let mock = MockServicesServer::start().await;
    let temp = TestConfigBuilder::new().build();

    // Search for "openai" (matches the mock service)
    let output = test_command(&temp)
        .env("TEMPO_SERVICES_URL", &mock.services_url)
        .args(["-j", "services", "--search", "openai"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(parsed.as_array().is_some_and(|a| !a.is_empty()));

    // Search for something that doesn't match
    let output = test_command(&temp)
        .env("TEMPO_SERVICES_URL", &mock.services_url)
        .args(["-j", "services", "--search", "zzz_no_match"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(parsed.as_array().is_some_and(std::vec::Vec::is_empty));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn services_info_not_found() {
    let mock = MockServicesServer::start().await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .env("TEMPO_SERVICES_URL", &mock.services_url)
        .args(["services", "nonexistent_service"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("not found"),
        "should mention not found: {combined}"
    );
}

#[test]
fn services_invalid_url_is_classified_as_usage_error() {
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .env("TEMPO_SERVICES_URL", "not-a-valid-url")
        .args(["services", "list"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert_exit_code(
        &output,
        2,
        "invalid TEMPO_SERVICES_URL should map to InvalidUsage",
    );

    let combined = get_combined_output(&output);
    assert!(
        combined.contains("invalid service directory URL"),
        "should report invalid service URL: {combined}"
    );
}

// ==================== mpp-sign ====================

/// Valid charge challenge for Tempo mainnet (chainId 4217).
const VALID_CHARGE_CHALLENGE: &str = r#"Payment id="test", realm="test", method="tempo", intent="charge", request="eyJhbW91bnQiOiIxMDAwIiwiY3VycmVuY3kiOiIweDIwYzAwMDAwMDAwMDAwMDAwMDAwMDAwMGI5NTM3ZDExYzYwZThiNTAiLCJtZXRob2REZXRhaWxzIjp7ImNoYWluSWQiOjQyMTd9fQ""#;

#[test]
fn sign_help_shows_flags() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["mpp-sign", "--help"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let combined = get_combined_output(&output);
    assert!(combined.contains("--challenge"), "should show --challenge");
    assert!(combined.contains("--dry-run"), "should show --dry-run");
}

#[test]
fn sign_dry_run_valid_challenge_succeeds() {
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args([
            "mpp-sign",
            "--dry-run",
            "--challenge",
            VALID_CHARGE_CHALLENGE,
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Challenge is valid"),
        "should confirm valid challenge: {stderr}"
    );
}

#[test]
fn sign_dry_run_json_emits_structured() {
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args([
            "-j",
            "mpp-sign",
            "--dry-run",
            "--challenge",
            VALID_CHARGE_CHALLENGE,
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["valid"], true);
    assert_eq!(parsed["method"], "tempo");
}

#[test]
fn sign_dry_run_invalid_challenge_fails() {
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args([
            "mpp-sign",
            "--dry-run",
            "--challenge",
            "not a valid challenge",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert_exit_code(&output, 4, "invalid challenge should exit with E_PAYMENT");
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("Failed to parse WWW-Authenticate challenge")
            || combined.contains("Invalid challenge"),
        "should include challenge parse failure context: {combined}"
    );
}

#[test]
fn sign_dry_run_unsupported_method() {
    let temp = TestConfigBuilder::new().build();
    let challenge = r#"Payment id="x", realm="x", method="stripe", intent="charge", request="e30""#;

    let output = test_command(&temp)
        .args(["mpp-sign", "--dry-run", "--challenge", challenge])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Unsupported"),
        "should mention unsupported: {stderr}"
    );
}

#[test]
fn sign_dry_run_missing_chain_id() {
    let temp = TestConfigBuilder::new().build();
    // request = base64url({"amount":"1000","currency":"0x00"}) — no methodDetails/chainId
    let challenge = r#"Payment id="x", realm="x", method="tempo", intent="charge", request="eyJhbW91bnQiOiIxMDAwIiwiY3VycmVuY3kiOiIweDAwIn0""#;

    let output = test_command(&temp)
        .args(["mpp-sign", "--dry-run", "--challenge", challenge])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert_exit_code(
        &output,
        4,
        "challenge missing chainId should exit with E_PAYMENT",
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("chainId"),
        "should mention chainId: {stderr}"
    );
    assert!(
        stderr.contains("Malformed payment request: missing chainId"),
        "should preserve missing chainId wording: {stderr}"
    );
}

#[test]
fn sign_dry_run_network_mismatch_reports_stable_message() {
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args([
            "--network",
            "tempo-moderato",
            "mpp-sign",
            "--dry-run",
            "--challenge",
            VALID_CHARGE_CHALLENGE,
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert_exit_code(
        &output,
        4,
        "network mismatch in challenge should exit with E_PAYMENT",
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("challenge network 'tempo' does not match --network 'tempo-moderato'"),
        "should preserve network mismatch wording: {stderr}"
    );
}

#[test]
fn sign_no_wallet_configured() {
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["mpp-sign", "--challenge", VALID_CHARGE_CHALLENGE])
        .output()
        .unwrap();

    assert!(!output.status.success());
}

#[test]
fn sign_empty_stdin_fails() {
    use std::process::Stdio;

    let temp = TestConfigBuilder::new().build();
    let mut child = test_command(&temp)
        .arg("mpp-sign")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn");
    drop(child.stdin.take()); // close stdin immediately
    let output = child.wait_with_output().expect("Failed to wait");
    assert!(!output.status.success());
}

#[test]
fn sign_dry_run_reads_from_stdin() {
    use std::{io::Write, process::Stdio};

    let temp = TestConfigBuilder::new().build();
    let mut child = test_command(&temp)
        .args(["mpp-sign", "--dry-run"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(VALID_CHARGE_CHALLENGE.as_bytes())
        .unwrap();
    drop(child.stdin.take());
    let output = child.wait_with_output().expect("Failed to wait");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "sign via stdin failed: {stderr}");
    assert!(
        stderr.contains("Challenge is valid"),
        "should confirm valid: {stderr}"
    );
}

// ==================== version ====================

#[test]
fn version_flag_outputs_version() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp).arg("--version").output().unwrap();

    assert!(output.status.success());
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("tempo wallet"),
        "should show version: {combined}"
    );
}

// ==================== transfer ====================

#[test]
fn transfer_help_shows_flags() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["transfer", "--help"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let combined = get_combined_output(&output);
    assert!(combined.contains("<TO>"), "should show TO positional arg");
    assert!(combined.contains("--dry-run"), "should show --dry-run flag");
    assert!(
        combined.contains("--fee-token"),
        "should show --fee-token flag"
    );
    assert!(
        combined.contains("Token contract address"),
        "should describe token as contract address: {combined}"
    );
}

#[test]
fn transfer_no_wallet_fails() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args([
            "transfer",
            "1.00",
            "0x20c0000000000000000000000b9537d11c60e8b50",
            "0x70997970C51812dc3A010C7d01b50e0d17dc79C8",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("No wallet") || combined.contains("login"),
        "should mention no wallet or login: {combined}"
    );
}

#[test]
fn transfer_no_wallet_json_fails() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args([
            "-j",
            "transfer",
            "1.00",
            "0x20c0000000000000000000000b9537d11c60e8b50",
            "0x70997970C51812dc3A010C7d01b50e0d17dc79C8",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
}

#[test]
fn transfer_missing_recipient_fails() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args([
            "transfer",
            "1.00",
            "0x20c0000000000000000000000b9537d11c60e8b50",
        ])
        .output()
        .unwrap();

    assert_exit_code(&output, 2, "missing recipient should exit with E_USAGE");
}

#[test]
fn transfer_missing_amount_fails() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp).args(["transfer"]).output().unwrap();

    assert_exit_code(&output, 2, "missing amount should exit with E_USAGE");
}

#[test]
fn transfer_missing_token_fails() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["transfer", "1.00"])
        .output()
        .unwrap();

    assert_exit_code(&output, 2, "missing token should exit with E_USAGE");
}

#[test]
fn transfer_invalid_token_address_fails() {
    let temp = TestConfigBuilder::new()
        .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
        .build();

    let output = test_command(&temp)
        .args([
            "-n",
            "tempo-moderato",
            "transfer",
            "1.00",
            "not-an-address",
            "0x70997970C51812dc3A010C7d01b50e0d17dc79C8",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("token address"),
        "should mention token address error: {combined}"
    );
}

#[test]
fn transfer_invalid_recipient_address_fails() {
    let temp = TestConfigBuilder::new()
        .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
        .build();

    // Use a non-0x string for recipient to trigger the validate_hex_input error
    // before any on-chain calls happen
    let output = test_command(&temp)
        .args([
            "-n",
            "tempo-moderato",
            "transfer",
            "1.00",
            "0x0000000000000000000000000000000000000001",
            "not-an-address",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("recipient address"),
        "should mention recipient address error: {combined}"
    );
}

// ==================== unknown subcommand ====================

#[test]
fn unknown_subcommand_fails() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp).arg("nonexistent").output().unwrap();

    assert_exit_code(&output, 2, "unknown subcommand should exit with E_USAGE");
}
