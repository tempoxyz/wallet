mod common;

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use axum::{routing::post, Json, Router};
use serde_json::json;
use tempo_test::{mock_rpc_response, TestConfigBuilder, MODERATO_DIRECT_KEYS_TOML};

use common::test_command;

#[allow(dead_code)]
struct MockLoginServer {
    base_url: String,
    poll_count: Arc<Mutex<u32>>,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    _handle: tokio::task::JoinHandle<()>,
}

impl MockLoginServer {
    async fn start_authorized(code: &str) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{}:{}", addr.ip(), addr.port());
        let poll_count = Arc::new(Mutex::new(0u32));

        let device_code = code.to_string();
        let poll_code = code.to_string();
        let poll_state = poll_count.clone();

        let app = Router::new()
            .route(
                "/cli-auth/device-code",
                post(move || {
                    let code = device_code.clone();
                    async move { Json(json!({ "code": code })) }
                }),
            )
            .route(
                "/cli-auth/poll/{code}",
                post(
                    move |axum::extract::Path(path_code): axum::extract::Path<String>| {
                        let expected = poll_code.clone();
                        let poll_state = poll_state.clone();
                        async move {
                            assert_eq!(path_code, expected, "unexpected poll code");
                            let mut count = poll_state.lock().unwrap();
                            let response = if *count == 0 {
                                *count += 1;
                                json!({ "status": "pending" })
                            } else {
                                *count += 1;
                                json!({
                                    "status": "authorized",
                                    "account_address": "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
                                    "key_authorization": null
                                })
                            };
                            Json(response)
                        }
                    },
                ),
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
            poll_count,
            shutdown_tx: Some(shutdown_tx),
            _handle: handle,
        }
    }

    fn auth_url(&self) -> String {
        format!("{}/cli-auth", self.base_url)
    }
}

impl Drop for MockLoginServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

#[allow(dead_code)]
struct BalanceSequenceRpcServer {
    base_url: String,
    balances: Arc<Mutex<VecDeque<String>>>,
    last_value: Arc<Mutex<String>>,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    _handle: tokio::task::JoinHandle<()>,
}

impl BalanceSequenceRpcServer {
    async fn start(raw_balances: Vec<&str>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{}:{}", addr.ip(), addr.port());

        let balances = Arc::new(Mutex::new(
            raw_balances
                .into_iter()
                .map(std::string::ToString::to_string)
                .collect(),
        ));
        let last_value = Arc::new(Mutex::new(String::from("0")));

        let shared_balances = balances.clone();
        let shared_last_value = last_value.clone();

        let app = Router::new().route(
            "/",
            post(move |Json(body): Json<serde_json::Value>| {
                let shared_balances = shared_balances.clone();
                let shared_last_value = shared_last_value.clone();
                async move {
                    if let Some(batch) = body.as_array() {
                        let response = serde_json::Value::Array(
                            batch
                                .iter()
                                .map(|req| {
                                    handle_rpc_request(req, &shared_balances, &shared_last_value)
                                })
                                .collect(),
                        );
                        Json(response)
                    } else {
                        Json(handle_rpc_request(
                            &body,
                            &shared_balances,
                            &shared_last_value,
                        ))
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
            balances,
            last_value,
            shutdown_tx: Some(shutdown_tx),
            _handle: handle,
        }
    }
}

impl Drop for BalanceSequenceRpcServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

fn handle_rpc_request(
    req: &serde_json::Value,
    balances: &Arc<Mutex<VecDeque<String>>>,
    last_value: &Arc<Mutex<String>>,
) -> serde_json::Value {
    if req["method"].as_str() == Some("eth_call") {
        let raw = next_balance(balances, last_value);
        let encoded = encode_raw_balance(&raw);
        return json!({
            "jsonrpc": "2.0",
            "id": req["id"].clone(),
            "result": encoded,
        });
    }

    mock_rpc_response(req, 42431)
}

fn next_balance(
    balances: &Arc<Mutex<VecDeque<String>>>,
    last_value: &Arc<Mutex<String>>,
) -> String {
    let next = {
        let mut queue = balances.lock().unwrap();
        queue.pop_front()
    };

    match next {
        Some(raw) => {
            *last_value.lock().unwrap() = raw.clone();
            raw
        }
        None => last_value.lock().unwrap().clone(),
    }
}

fn encode_raw_balance(raw: &str) -> String {
    let value = raw.parse::<u128>().unwrap();
    let bytes = alloy::primitives::U256::from(value).to_be_bytes::<32>();
    format!("0x{}", hex::encode(bytes))
}

fn moderato_config_toml(rpc_url: &str) -> String {
    format!("[rpc]\n\"tempo-moderato\" = \"{rpc_url}\"\n")
}

fn build_login_temp(rpc_url: &str) -> tempfile::TempDir {
    TestConfigBuilder::new()
        .with_config_toml(moderato_config_toml(rpc_url))
        .build()
}

fn build_fund_temp(rpc_url: &str) -> tempfile::TempDir {
    TestConfigBuilder::new()
        .with_config_toml(moderato_config_toml(rpc_url))
        .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
        .build()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn login_no_browser_prints_remote_safe_handoff_copy_and_completes() {
    let login = MockLoginServer::start_authorized("ANMGE375").await;
    let rpc = BalanceSequenceRpcServer::start(vec!["0"]).await;
    let temp = build_login_temp(&rpc.base_url);

    let output = test_command(&temp)
        .env("TEMPO_AUTH_URL", login.auth_url())
        .args(["-n", "tempo-moderato", "login", "--no-browser"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "login should succeed: {:?}",
        output
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Auth URL:"), "{stderr}");
    assert!(stderr.contains("Verification code:"), "{stderr}");
    assert!(stderr.contains("Open this link on your device"), "{stderr}");
    assert!(
        stderr.contains("If the wallet page shows that same code"),
        "{stderr}"
    );
    assert!(stderr.contains("tap Continue"), "{stderr}");
    assert!(
        stderr.contains("After passkey or wallet creation, return here"),
        "{stderr}"
    );
    assert!(stderr.contains("one more authorization link"), "{stderr}");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Wallet"), "{stdout}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn login_default_flow_keeps_local_copy_and_does_not_print_remote_handoff_text() {
    let login = MockLoginServer::start_authorized("ANMGE375").await;
    let rpc = BalanceSequenceRpcServer::start(vec!["0"]).await;
    let temp = build_login_temp(&rpc.base_url);

    let output = test_command(&temp)
        .env("TEMPO_AUTH_URL", login.auth_url())
        .args(["-n", "tempo-moderato", "login"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "login should succeed: {:?}",
        output
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Auth URL:"), "{stderr}");
    assert!(stderr.contains("Verification code:"), "{stderr}");
    assert!(
        !stderr.contains("Open this link on your device"),
        "unexpected remote-safe handoff text: {stderr}"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Wallet"), "{stdout}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fund_no_browser_prints_remote_safe_handoff_copy_and_detects_balance_change() {
    let rpc = BalanceSequenceRpcServer::start(vec!["0", "1000000"]).await;
    let temp = build_fund_temp(&rpc.base_url);

    let output = test_command(&temp)
        .env(
            "TEMPO_AUTH_URL",
            "https://wallet.moderato.tempo.xyz/cli-auth",
        )
        .args(["-n", "tempo-moderato", "fund", "--no-browser"])
        .output()
        .unwrap();

    assert!(output.status.success(), "fund should succeed: {:?}", output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Fund URL:"), "{stderr}");
    assert!(stderr.contains("Open this link on your device"), "{stderr}");
    assert!(stderr.contains("After funding is complete"), "{stderr}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fund_default_flow_keeps_local_copy_and_does_not_print_remote_handoff_text() {
    let rpc = BalanceSequenceRpcServer::start(vec!["0", "1000000"]).await;
    let temp = build_fund_temp(&rpc.base_url);

    let output = test_command(&temp)
        .env(
            "TEMPO_AUTH_URL",
            "https://wallet.moderato.tempo.xyz/cli-auth",
        )
        .args(["-n", "tempo-moderato", "fund"])
        .output()
        .unwrap();

    assert!(output.status.success(), "fund should succeed: {:?}", output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Fund URL:"), "{stderr}");
    assert!(
        !stderr.contains("Open this link on your device"),
        "unexpected remote-safe handoff text: {stderr}"
    );
}
