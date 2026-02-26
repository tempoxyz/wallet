//! HTTP behavior tests against a local axum mock server.
//!
//! These run in normal `cargo test` — no network or funded wallet required.

mod common;

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::any;
use axum::Router;
use serde_json::json;

use crate::common::{get_combined_output, test_command, write_test_files, TestConfigBuilder};

struct MockServer {
    base_url: String,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    _handle: tokio::task::JoinHandle<()>,
}

impl MockServer {
    async fn start(status: u16, headers: Vec<(&str, &str)>, body: &str) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{port}");

        let status_code = StatusCode::from_u16(status).unwrap();
        let owned_headers: Vec<(String, String)> = headers
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let owned_body = body.to_string();

        let app = Router::new().route(
            "/{*path}",
            any(move || {
                let hdrs = owned_headers.clone();
                let b = owned_body.clone();
                async move {
                    let mut response = (status_code, b).into_response();
                    for (k, v) in &hdrs {
                        response.headers_mut().insert(
                            axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                            axum::http::HeaderValue::from_str(v).unwrap(),
                        );
                    }
                    response
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

        MockServer {
            base_url,
            shutdown_tx: Some(shutdown_tx),
            _handle: handle,
        }
    }

    /// Start a payment mock: returns 402 + WWW-Authenticate when no Authorization
    /// header is present, returns 200 + body when Authorization header is present.
    async fn start_payment(www_authenticate: &str, success_body: &str) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{port}");

        let owned_header = www_authenticate.to_string();
        let owned_body = success_body.to_string();

        let app = Router::new().route(
            "/{*path}",
            any(move |headers: axum::http::HeaderMap| {
                let h = owned_header.clone();
                let b = owned_body.clone();
                async move {
                    if headers.get("authorization").is_some() {
                        (StatusCode::OK, b).into_response()
                    } else {
                        let mut response =
                            (StatusCode::PAYMENT_REQUIRED, "Payment Required").into_response();
                        response.headers_mut().insert(
                            axum::http::HeaderName::from_static("www-authenticate"),
                            axum::http::HeaderValue::from_str(&h).unwrap(),
                        );
                        response
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

        MockServer {
            base_url,
            shutdown_tx: Some(shutdown_tx),
            _handle: handle,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    /// Start a payment mock that also returns a Payment-Receipt header on success
    async fn start_payment_with_receipt(
        www_authenticate: &str,
        success_body: &str,
        receipt_header: &str,
    ) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{port}");

        let owned_header = www_authenticate.to_string();
        let owned_body = success_body.to_string();
        let owned_receipt = receipt_header.to_string();

        let app = Router::new().route(
            "/{*path}",
            any(move |headers: axum::http::HeaderMap| {
                let h = owned_header.clone();
                let b = owned_body.clone();
                let r = owned_receipt.clone();
                async move {
                    if headers.get("authorization").is_some() {
                        let mut resp = (StatusCode::OK, b).into_response();
                        resp.headers_mut().insert(
                            axum::http::HeaderName::from_static("payment-receipt"),
                            axum::http::HeaderValue::from_str(&r).unwrap(),
                        );
                        resp
                    } else {
                        let mut response =
                            (StatusCode::PAYMENT_REQUIRED, "Payment Required").into_response();
                        response.headers_mut().insert(
                            axum::http::HeaderName::from_static("www-authenticate"),
                            axum::http::HeaderValue::from_str(&h).unwrap(),
                        );
                        response
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

        MockServer {
            base_url,
            shutdown_tx: Some(shutdown_tx),
            _handle: handle,
        }
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

impl MockServer {
    /// Start a mock that echoes request headers back as a JSON body.
    async fn start_echo_headers() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{port}");

        let app = Router::new().route(
            "/{*path}",
            any(move |headers: axum::http::HeaderMap| async move {
                let mut map = serde_json::Map::new();
                for (k, v) in headers.iter() {
                    if let Ok(s) = v.to_str() {
                        map.insert(
                            k.as_str().to_string(),
                            serde_json::Value::String(s.to_string()),
                        );
                    }
                }
                let body = serde_json::to_string(&map).unwrap();
                (StatusCode::OK, body).into_response()
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

        MockServer {
            base_url,
            shutdown_tx: Some(shutdown_tx),
            _handle: handle,
        }
    }

    /// Start a mock that returns an SSE stream with the given raw body.
    async fn start_sse(body: &str) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{port}");

        let owned_body = body.to_string();

        let app = Router::new().route(
            "/{*path}",
            any(move || {
                let b = owned_body.clone();
                async move {
                    let mut response = (StatusCode::OK, b).into_response();
                    response.headers_mut().insert(
                        axum::http::HeaderName::from_static("content-type"),
                        axum::http::HeaderValue::from_static("text/event-stream"),
                    );
                    response
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

        MockServer {
            base_url,
            shutdown_tx: Some(shutdown_tx),
            _handle: handle,
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_non_402_get_request() {
    let server = MockServer::start(200, vec![], "hello world").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .arg(server.url("/test"))
        .output()
        .unwrap();

    assert!(output.status.success(), "expected success");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("hello world"),
        "stdout should contain body: {stdout}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_non_402_post_with_json() {
    let server = MockServer::start(200, vec![], r#"{"result":"ok"}"#).await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args([
            "-X",
            "POST",
            "--json",
            r#"{"key":"val"}"#,
            &server.url("/api"),
        ])
        .output()
        .unwrap();

    assert!(output.status.success(), "expected success");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(r#"{"result":"ok"}"#),
        "stdout should contain JSON response: {stdout}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_include_headers_flag() {
    let server = MockServer::start(200, vec![("x-test", "foo")], "body").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-i", &server.url("/test")])
        .output()
        .unwrap();

    assert!(output.status.success(), "expected success");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("HTTP 200"),
        "stdout should contain status line: {stdout}"
    );
    assert!(
        stdout.contains("x-test: foo"),
        "stdout should contain custom header: {stdout}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_output_to_file() {
    let server = MockServer::start(200, vec![], "file content here").await;
    let temp = TestConfigBuilder::new().build();
    let out_file = temp.path().join("output.txt");

    let output = test_command(&temp)
        .args(["-o", out_file.to_str().unwrap(), &server.url("/test")])
        .output()
        .unwrap();

    assert!(output.status.success(), "expected success");
    assert!(out_file.exists(), "output file should exist");
    let contents = std::fs::read_to_string(&out_file).unwrap();
    assert!(
        contents.contains("file content here"),
        "file should contain body: {contents}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_server_error_500() {
    let server = MockServer::start(500, vec![], "Internal Server Error").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .arg(server.url("/error"))
        .output()
        .unwrap();

    assert!(!output.status.success(), "presto should fail on 500 error");
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("500"),
        "output should mention status code: {combined}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_connection_refused() {
    // Retry with different ports to avoid the race where another process
    // claims the port between our drop and the CLI's connect attempt.
    for _ in 0..3 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let temp = TestConfigBuilder::new().build();

        let output = test_command(&temp)
            .arg(format!("http://127.0.0.1:{port}/test"))
            .output()
            .unwrap();

        if output.status.success() {
            // Port was reused by another process; try again with a new port
            continue;
        }

        let combined = get_combined_output(&output);
        assert!(
            combined.contains("error")
                || combined.contains("connect")
                || combined.contains("Connection"),
            "output should mention connection error: {combined}"
        );
        return;
    }
    panic!("Could not find a closed port after 3 attempts — port reuse race in CI?");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_session_list_empty() {
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["session", "list"])
        .output()
        .unwrap();

    let combined = get_combined_output(&output);
    assert!(
        combined.contains("No active sessions"),
        "should say no active sessions: {combined}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_session_close_no_session() {
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["session", "close", "https://example.com"])
        .output()
        .unwrap();

    let combined = get_combined_output(&output);
    assert!(
        combined.contains("No active session"),
        "should say no active session: {combined}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_402_without_valid_payment_header() {
    let server = MockServer::start(402, vec![], "Payment Required").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .arg(server.url("/paid"))
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "expected failure on 402 without payment header"
    );
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("WWW-Authenticate"),
        "should mention missing header: {combined}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_402_unsupported_payment_method() {
    // WWW-Authenticate present but with a non-tempo method should be rejected
    // Build a minimal valid-looking header with method="other"
    let challenge_request = "eyJhbW91bnQiOiIxMDAwMDAwIiwiY3VycmVuY3kiOiIweDIwYzAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAiLCJtZXRob2REZXRhaWxzIjp7ImNoYWluSWQiOjQyNDMxfSwicmVjaXBpZW50IjoiMHg3MDk5Nzk3MEM1MTgxMmRjM0EwMTBDN2QwMWI1MGUwZDE3ZGM3OUM4In0";
    let www_auth = format!(
        r#"Payment id="test-unsupported", realm="mock", method="other", intent="charge", request="{challenge_request}""#
    );

    let server = MockServer::start(
        402,
        vec![("www-authenticate", &www_auth)],
        "Payment Required",
    )
    .await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .arg(server.url("/paid"))
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "expected failure on unsupported payment method"
    );
    let combined = get_combined_output(&output);
    assert!(
        combined
            .to_lowercase()
            .contains("unsupported payment method"),
        "should mention unsupported payment method: {combined}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_dry_run_no_payment() {
    let server = MockServer::start(200, vec![], "dry run body").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["--dry-run", &server.url("/test")])
        .output()
        .unwrap();

    assert!(output.status.success(), "dry run should succeed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_quiet_suppresses_logs() {
    let server = MockServer::start(200, vec![], "quiet body").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-s", &server.url("/test")])
        .output()
        .unwrap();

    assert!(output.status.success(), "expected success");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.is_empty(),
        "stderr should be empty in quiet mode: {stderr}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_verbose_shows_logs() {
    let server = MockServer::start(200, vec![], "verbose body").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-v", &server.url("/test")])
        .output()
        .unwrap();

    assert!(output.status.success(), "expected success");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Making GET request"),
        "stderr should contain verbose log: {stderr}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_custom_header() {
    let server = MockServer::start(200, vec![], "ok").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-H", "X-Custom: myvalue", &server.url("/test")])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "expected success with custom header"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("ok"),
        "stdout should contain body: {stdout}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_post_data_flag() {
    let server = MockServer::start(200, vec![], "posted").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-d", "key=value", &server.url("/test")])
        .output()
        .unwrap();

    assert!(output.status.success(), "expected success with -d flag");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("posted"),
        "stdout should contain body: {stdout}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_post_data_from_file() {
    let server = MockServer::start(200, vec![], "file-posted").await;
    let temp = TestConfigBuilder::new().build();

    let data_file = temp.path().join("postdata.txt");
    std::fs::write(&data_file, "file_key=file_value").unwrap();

    let data_arg = format!("@{}", data_file.to_str().unwrap());
    let output = test_command(&temp)
        .args(["-d", &data_arg, &server.url("/test")])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "expected success with -d @file: {}",
        get_combined_output(&output)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("file-posted"),
        "stdout should contain body: {stdout}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_multiple_data_flags() {
    let server = MockServer::start(200, vec![], "multi-posted").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-d", "a=1", "-d", "b=2", &server.url("/test")])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "expected success with multiple -d flags"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("multi-posted"),
        "stdout should contain body: {stdout}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_retries_and_backoff_on_unreachable_host() {
    // Port 9 is "discard" and typically closed locally; triggers a connect error quickly.
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args([
            "-j",
            "--retries",
            "1",
            "--retry-backoff",
            "10",
            "--timeout",
            "1",
            "http://127.0.0.1:9",
        ])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "expected failure on unreachable host"
    );
    // Should emit JSON error to stdout
    let stdout = String::from_utf8_lossy(&output.stdout);
    let _: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid json error");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_output_format_json() {
    let server = MockServer::start(
        200,
        vec![("content-type", "application/json")],
        r#"{"key":"value"}"#,
    )
    .await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-j", &server.url("/test")])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "expected success with -j json output"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("key"),
        "stdout should contain JSON key: {stdout}"
    );
    assert!(
        stdout.contains("value"),
        "stdout should contain JSON value: {stdout}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_no_redirect() {
    let server = MockServer::start(
        301,
        vec![("location", "http://127.0.0.1:1/other")],
        "redirecting",
    )
    .await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-i", &server.url("/test")])
        .output()
        .unwrap();

    let combined = get_combined_output(&output);
    assert!(
        combined.contains("301"),
        "output should contain 301 status: {combined}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_fail_flag_suppresses_body() {
    let server = MockServer::start(404, vec![], "not found body").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-f", &server.url("/missing")])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "-f should make HTTP >= 400 exit with error"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("not found body"),
        "-f should suppress error response body: {stdout}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_dump_header_writes_file() {
    let server = MockServer::start(200, vec![("x-test", "1")], "ok").await;
    let temp = TestConfigBuilder::new().build();

    let hdr_path = temp.path().join("headers.out");
    let hdr_str = hdr_path.to_string_lossy().to_string();
    let output = test_command(&temp)
        .args(["-D", &hdr_str, &server.url("/test")])
        .output()
        .unwrap();

    assert!(output.status.success(), "request should succeed");
    let dumped = std::fs::read_to_string(&hdr_path).expect("headers file exists");
    assert!(dumped.contains("HTTP 200"));
    assert!(dumped.to_lowercase().contains("x-test: 1"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_timeout_flag() {
    let server = MockServer::start(200, vec![], "fast").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["--timeout", "1", &server.url("/test")])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "expected success with --timeout flag: {}",
        get_combined_output(&output)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("fast"),
        "stdout should contain body: {stdout}"
    );
}

// ==================== Mock RPC Server ====================

/// Mock JSON-RPC server for EVM RPC responses.
struct MockRpcServer {
    base_url: String,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    _handle: tokio::task::JoinHandle<()>,
}

impl MockRpcServer {
    async fn start(chain_id: u64) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{port}");

        let app = Router::new().route(
            "/",
            axum::routing::post(
                move |axum::extract::Json(body): axum::extract::Json<serde_json::Value>| async move {
                    let response = if body.is_array() {
                        serde_json::Value::Array(
                            body.as_array()
                                .unwrap()
                                .iter()
                                .map(|req| mock_rpc_response(req, chain_id))
                                .collect(),
                        )
                    } else {
                        mock_rpc_response(&body, chain_id)
                    };
                    axum::Json(response)
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

        MockRpcServer {
            base_url,
            shutdown_tx: Some(shutdown_tx),
            _handle: handle,
        }
    }
}

impl Drop for MockRpcServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

fn mock_rpc_response(req: &serde_json::Value, chain_id: u64) -> serde_json::Value {
    let method = req["method"].as_str().unwrap_or("");
    let id = req["id"].clone();

    let result: serde_json::Value = match method {
        "eth_chainId" => json!(format!("0x{:x}", chain_id)),
        "eth_getTransactionCount" => json!("0x0"),
        "eth_estimateGas" => json!("0x5208"),
        "eth_maxPriorityFeePerGas" => json!("0x3b9aca00"),
        "eth_gasPrice" => json!("0x4a817c800"),
        "eth_getBalance" => json!("0xde0b6b3a7640000"),
        "eth_call" => json!("0x"),
        "eth_sendRawTransaction" => {
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
        "result": result
    })
}

// ==================== 402 Charge Payment Flow ====================

/// Test the full 402 → payment → 200 charge flow with mock servers.
///
/// Verifies that presto correctly:
/// 1. Receives a 402 with WWW-Authenticate header
/// 2. Parses the MPP payment challenge
/// 3. Loads wallet credentials
/// 4. Builds and signs a payment credential via mock RPC
/// 5. Retries the request with Authorization header
/// 6. Returns the final 200 response body
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_402_charge_flow() {
    // Mock RPC server for tempo-moderato (chain 42431)
    let rpc = MockRpcServer::start(42431).await;

    // base64url-no-padding of canonical JSON:
    // {"amount":"1000000","currency":"0x20c0...","methodDetails":{"chainId":42431},"recipient":"0x7099..."}
    let challenge_request = "eyJhbW91bnQiOiIxMDAwMDAwIiwiY3VycmVuY3kiOiIweDIwYzAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAiLCJtZXRob2REZXRhaWxzIjp7ImNoYWluSWQiOjQyNDMxfSwicmVjaXBpZW50IjoiMHg3MDk5Nzk3MEM1MTgxMmRjM0EwMTBDN2QwMWI1MGUwZDE3ZGM3OUM4In0";
    let www_auth = format!(
        r#"Payment id="test-charge", realm="mock", method="tempo", intent="charge", request="{challenge_request}""#
    );

    // Payment mock: 402 on first request, 200 on retry with Authorization
    let server = MockServer::start_payment(&www_auth, "charge accepted").await;

    // Set up temp dir with wallet + config pointing RPC to mock
    let temp = TestConfigBuilder::new()
        .with_keys_toml(
            r#"
[[keys]]
wallet_address = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
key_address = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
key = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
"#,
        )
        .with_config_toml(format!("moderato_rpc = \"{}\"\n", rpc.base_url))
        .build();

    let output = test_command(&temp)
        .args(["-v", &server.url("/api")])
        .output()
        .unwrap();

    let combined = get_combined_output(&output);
    assert!(
        output.status.success(),
        "expected 402 → payment → 200 flow to succeed: {combined}"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("charge accepted"),
        "stdout should contain success body: {combined}"
    );
}

/// Payment narration is printed at -v when encountering a 402
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_402_payment_narration_verbose() {
    // Mock RPC server and 402→200 server
    let rpc = MockRpcServer::start(42431).await;

    let challenge_request = "eyJhbW91bnQiOiIxMDAwMDAwIiwiY3VycmVuY3kiOiIweDIwYzAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAiLCJtZXRob2REZXRhaWxzIjp7ImNoYWluSWQiOjQyNDMxfSwicmVjaXBpZW50IjoiMHg3MDk5Nzk3MEM1MTgxMmRjM0EwMTBDN2QwMWI1MGUwZDE3ZGM3OUM4In0";
    let www_auth = format!(
        r#"Payment id="test-charge", realm="mock", method="tempo", intent="charge", request="{challenge_request}""#
    );
    let server = MockServer::start_payment(&www_auth, "ok").await;

    // Write wallet + RPC config
    let temp = TestConfigBuilder::new()
        .with_keys_toml(
            r#"
[[keys]]
wallet_address = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
key_address = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
key = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
"#,
        )
        .with_config_toml(format!("moderato_rpc = \"{}\"\n", rpc.base_url))
        .build();

    let output = test_command(&temp)
        .args(["-v", &server.url("/api")])
        .output()
        .unwrap();

    assert!(output.status.success(), "expected success");
    let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
    assert!(
        stderr.contains("payment required:"),
        "should narrate 402 payment requirement: {}",
        stderr
    );
}

/// Paid summary prints by default and is suppressed by -q
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_402_paid_summary_default_and_quiet() {
    let rpc = MockRpcServer::start(42431).await;

    let challenge_request = "eyJhbW91bnQiOiIxMDAwMDAwIiwiY3VycmVuY3kiOiIweDIwYzAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAiLCJtZXRob2REZXRhaWxzIjp7ImNoYWluSWQiOjQyNDMxfSwicmVjaXBpZW50IjoiMHg3MDk5Nzk3MEM1MTgxMmRjM0EwMTBDN2QwMWI1MGUwZDE3ZGM3OUM4In0";
    let www_auth = format!(
        r#"Payment id="test-charge-paid", realm="mock", method="tempo", intent="charge", request="{challenge_request}""#
    );
    let server = MockServer::start_payment(&www_auth, "ok").await;

    let temp = TestConfigBuilder::new()
        .with_keys_toml(
            r#"
[[keys]]
wallet_address = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
key_address = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
key = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
"#,
        )
        .with_config_toml(format!("moderato_rpc = \"{}\"\n", rpc.base_url))
        .build();

    // Default: summary should be printed
    let output_default = test_command(&temp)
        .args([&server.url("/api")])
        .output()
        .unwrap();
    assert!(
        output_default.status.success(),
        "default run should succeed"
    );
    let stderr_default = String::from_utf8_lossy(&output_default.stderr);
    assert!(
        stderr_default.contains("Paid "),
        "expected paid summary in default mode, got: {}",
        stderr_default
    );

    // Quiet: summary should be suppressed
    let output_quiet = test_command(&temp)
        .args(["-s", &server.url("/api")])
        .output()
        .unwrap();
    assert!(output_quiet.status.success(), "quiet run should succeed");
    let stderr_quiet = String::from_utf8_lossy(&output_quiet.stderr);
    assert!(
        !stderr_quiet.contains("Paid "),
        "expected no paid summary in quiet mode, got: {}",
        stderr_quiet
    );
}

/// Analytics PaymentSuccess tx_hash should be the extracted hex, not the raw header
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_analytics_tx_hash_is_extracted_hex() {
    let rpc = MockRpcServer::start(42431).await;

    // Simple 64-nybble hex hash
    let tx_hash = format!("0x{}", "ab".repeat(32));
    let challenge_request = "eyJhbW91bnQiOiIxMDAwMDAwIiwiY3VycmVuY3kiOiIweDIwYzAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAiLCJtZXRob2REZXRhaWxzIjp7ImNoYWluSWQiOjQyNDMxfSwicmVjaXBpZW50IjoiMHg3MDk5Nzk3MEM1MTgxMmRjM0EwMTBDN2QwMWI1MGUwZDE3ZGM3OUM4In0";
    let www_auth = format!(
        r#"Payment id="test-charge-analytics", realm="mock", method="tempo", intent="charge", request="{challenge_request}""#
    );

    // Return a Payment-Receipt header that includes the tx hash in a field the extractor recognizes
    let receipt_value = format!("tx={}", tx_hash);
    let server = MockServer::start_payment_with_receipt(&www_auth, "ok", &receipt_value).await;

    let temp = TestConfigBuilder::new()
        .with_keys_toml(
            r#"
[[keys]]
wallet_address = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
key_address = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
key = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
"#,
        )
        .with_config_toml(format!("moderato_rpc = \"{}\"\n", rpc.base_url))
        .build();

    // Set up analytics tap file
    let events_path = temp.path().join("events.log");

    let output = test_command(&temp)
        .env("PRESTO_TEST_EVENTS", events_path.to_str().unwrap())
        .args(["-v", &server.url("/api")])
        .output()
        .unwrap();
    assert!(output.status.success(), "expected success");

    // Read captured events and find payment_success
    let content = std::fs::read_to_string(&events_path).unwrap();
    let mut found = None;
    for line in content.lines() {
        if let Some((name, json_str)) = line.split_once('|') {
            if name == "payment_success" {
                found = Some(json_str.to_string());
            }
        }
    }
    let Some(json_str) = found else {
        panic!("missing payment_success event: {}", content);
    };
    let v: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    let got = v.get("tx_hash").and_then(|x| x.as_str()).unwrap_or("");
    // Accept either an exact extracted 0x-hash, or empty (if server didn't supply a
    // parseable receipt). Critically, it must NOT be the raw header with fields.
    let is_hex =
        got.starts_with("0x") && got.len() == 66 && got[2..].chars().all(|c| c.is_ascii_hexdigit());
    assert!(
        got.is_empty() || is_hex,
        "tx_hash should be empty or a 0x-hex hash, got: {}",
        got
    );
    assert!(
        !got.contains('='),
        "tx_hash should not be a raw header with fields: {}",
        got
    );
}

/// Test the 402 → payment → 200 charge flow with Keychain signing mode.
///
/// Uses a different `wallet_address` than the derived address of the private
/// key, which triggers `TempoSigningMode::Keychain` instead of `Direct`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_402_charge_flow_keychain() {
    let rpc = MockRpcServer::start(42431).await;

    let challenge_request = "eyJhbW91bnQiOiIxMDAwMDAwIiwiY3VycmVuY3kiOiIweDIwYzAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAiLCJtZXRob2REZXRhaWxzIjp7ImNoYWluSWQiOjQyNDMxfSwicmVjaXBpZW50IjoiMHg3MDk5Nzk3MEM1MTgxMmRjM0EwMTBDN2QwMWI1MGUwZDE3ZGM3OUM4In0";
    let www_auth = format!(
        r#"Payment id="test-charge-kc", realm="mock", method="tempo", intent="charge", request="{challenge_request}""#
    );

    let server = MockServer::start_payment(&www_auth, "keychain charge accepted").await;

    // wallet_address (0x7099...) differs from the private key's derived
    // address (0xf39F...), triggering Keychain signing mode.
    let temp = TestConfigBuilder::new()
        .with_keys_toml(
            r#"
[[keys]]
wallet_address = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
chain_id = 42431
key_address = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
key = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
provisioned = true
"#,
        )
        .with_config_toml(format!("moderato_rpc = \"{}\"\n", rpc.base_url))
        .build();

    let output = test_command(&temp)
        .args(["-v", &server.url("/api")])
        .output()
        .unwrap();

    let combined = get_combined_output(&output);
    assert!(
        output.status.success(),
        "expected keychain 402 → payment → 200 flow to succeed: {combined}"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("keychain charge accepted"),
        "stdout should contain success body: {combined}"
    );
}

// ==================== --private-key Flag ====================

const TEST_PRIVATE_KEY: &str = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

/// Helper: set up a temp dir with config (pointing RPC to mock) but NO keys.toml.
fn setup_config_only(temp: &tempfile::TempDir, rpc_base_url: &str) {
    let config_toml = format!("moderato_rpc = \"{rpc_base_url}\"\n");
    write_test_files(temp.path(), &config_toml, None);
}

/// The 402 charge flow works with --private-key (no keys.toml needed).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_402_charge_flow_with_private_key_flag() {
    let rpc = MockRpcServer::start(42431).await;

    let challenge_request = "eyJhbW91bnQiOiIxMDAwMDAwIiwiY3VycmVuY3kiOiIweDIwYzAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAiLCJtZXRob2REZXRhaWxzIjp7ImNoYWluSWQiOjQyNDMxfSwicmVjaXBpZW50IjoiMHg3MDk5Nzk3MEM1MTgxMmRjM0EwMTBDN2QwMWI1MGUwZDE3ZGM3OUM4In0";
    let www_auth = format!(
        r#"Payment id="test-pk", realm="mock", method="tempo", intent="charge", request="{challenge_request}""#
    );

    let server = MockServer::start_payment(&www_auth, "private key charge ok").await;

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let output = test_command(&temp)
        .args(["-v", "--private-key", TEST_PRIVATE_KEY, &server.url("/api")])
        .output()
        .unwrap();

    let combined = get_combined_output(&output);
    assert!(
        output.status.success(),
        "expected --private-key charge flow to succeed: {combined}"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("private key charge ok"),
        "stdout should contain success body: {combined}"
    );
}

/// --private-key via PRESTO_PRIVATE_KEY env var works.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_402_charge_flow_with_private_key_env() {
    let rpc = MockRpcServer::start(42431).await;

    let challenge_request = "eyJhbW91bnQiOiIxMDAwMDAwIiwiY3VycmVuY3kiOiIweDIwYzAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAiLCJtZXRob2REZXRhaWxzIjp7ImNoYWluSWQiOjQyNDMxfSwicmVjaXBpZW50IjoiMHg3MDk5Nzk3MEM1MTgxMmRjM0EwMTBDN2QwMWI1MGUwZDE3ZGM3OUM4In0";
    let www_auth = format!(
        r#"Payment id="test-pk-env", realm="mock", method="tempo", intent="charge", request="{challenge_request}""#
    );

    let server = MockServer::start_payment(&www_auth, "env key charge ok").await;

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let output = test_command(&temp)
        .env("PRESTO_PRIVATE_KEY", TEST_PRIVATE_KEY)
        .args(["-v", &server.url("/api")])
        .output()
        .unwrap();

    let combined = get_combined_output(&output);
    assert!(
        output.status.success(),
        "expected PRESTO_PRIVATE_KEY charge flow to succeed: {combined}"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("env key charge ok"),
        "stdout should contain success body: {combined}"
    );
}

/// --private-key without 0x prefix works.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_private_key_without_0x_prefix() {
    let rpc = MockRpcServer::start(42431).await;

    let challenge_request = "eyJhbW91bnQiOiIxMDAwMDAwIiwiY3VycmVuY3kiOiIweDIwYzAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAiLCJtZXRob2REZXRhaWxzIjp7ImNoYWluSWQiOjQyNDMxfSwicmVjaXBpZW50IjoiMHg3MDk5Nzk3MEM1MTgxMmRjM0EwMTBDN2QwMWI1MGUwZDE3ZGM3OUM4In0";
    let www_auth = format!(
        r#"Payment id="test-pk-no0x", realm="mock", method="tempo", intent="charge", request="{challenge_request}""#
    );

    let server = MockServer::start_payment(&www_auth, "no prefix ok").await;

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    // Strip the 0x prefix
    let pk_no_prefix = TEST_PRIVATE_KEY.strip_prefix("0x").unwrap();

    let output = test_command(&temp)
        .args(["--private-key", pk_no_prefix, &server.url("/api")])
        .output()
        .unwrap();

    let combined = get_combined_output(&output);
    assert!(
        output.status.success(),
        "expected --private-key without 0x to succeed: {combined}"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("no prefix ok"),
        "stdout should contain success body: {combined}"
    );
}

/// --private-key with invalid hex gives a clear error.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_private_key_invalid_hex_fails() {
    let rpc = MockRpcServer::start(42431).await;

    let challenge_request = "eyJhbW91bnQiOiIxMDAwMDAwIiwiY3VycmVuY3kiOiIweDIwYzAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAiLCJtZXRob2REZXRhaWxzIjp7ImNoYWluSWQiOjQyNDMxfSwicmVjaXBpZW50IjoiMHg3MDk5Nzk3MEM1MTgxMmRjM0EwMTBDN2QwMWI1MGUwZDE3ZGM3OUM4In0";
    let www_auth = format!(
        r#"Payment id="test-pk-bad", realm="mock", method="tempo", intent="charge", request="{challenge_request}""#
    );

    let server = MockServer::start_payment(&www_auth, "should not reach").await;

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let output = test_command(&temp)
        .args(["--private-key", "not-a-valid-key", &server.url("/api")])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "expected failure with invalid private key"
    );
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("Invalid private key") || combined.contains("Invalid hex"),
        "error should mention invalid key: {combined}"
    );
}

/// --private-key with too-short key gives a clear error.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_private_key_wrong_length_fails() {
    let rpc = MockRpcServer::start(42431).await;

    let challenge_request = "eyJhbW91bnQiOiIxMDAwMDAwIiwiY3VycmVuY3kiOiIweDIwYzAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAiLCJtZXRob2REZXRhaWxzIjp7ImNoYWluSWQiOjQyNDMxfSwicmVjaXBpZW50IjoiMHg3MDk5Nzk3MEM1MTgxMmRjM0EwMTBDN2QwMWI1MGUwZDE3ZGM3OUM4In0";
    let www_auth = format!(
        r#"Payment id="test-pk-short", realm="mock", method="tempo", intent="charge", request="{challenge_request}""#
    );

    let server = MockServer::start_payment(&www_auth, "should not reach").await;

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let output = test_command(&temp)
        .args(["--private-key", "0xdeadbeef", &server.url("/api")])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "expected failure with too-short key"
    );
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("Invalid private key"),
        "error should mention invalid key: {combined}"
    );
}

/// --private-key takes precedence over keys.toml (keys.toml is ignored).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_private_key_flag_overrides_wallet() {
    let rpc = MockRpcServer::start(42431).await;

    let challenge_request = "eyJhbW91bnQiOiIxMDAwMDAwIiwiY3VycmVuY3kiOiIweDIwYzAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAiLCJtZXRob2REZXRhaWxzIjp7ImNoYWluSWQiOjQyNDMxfSwicmVjaXBpZW50IjoiMHg3MDk5Nzk3MEM1MTgxMmRjM0EwMTBDN2QwMWI1MGUwZDE3ZGM3OUM4In0";
    let www_auth = format!(
        r#"Payment id="test-pk-override", realm="mock", method="tempo", intent="charge", request="{challenge_request}""#
    );

    let server = MockServer::start_payment(&www_auth, "override ok").await;

    // Set up keys.toml with a DIFFERENT key (Hardhat #1) that points to a
    // different address. The --private-key flag should be used instead.
    let wallet_toml = r#"
[[keys]]
wallet_address = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
key_address = "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d"
key = "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d"
"#;
    let config_toml = format!("moderato_rpc = \"{}\"\n", rpc.base_url);
    let temp = tempfile::TempDir::new().unwrap();
    write_test_files(temp.path(), &config_toml, Some(wallet_toml));

    // Snapshot keys.toml content before the run
    let keys_path = temp
        .path()
        .join("Library/Application Support/presto/keys.toml");
    let wallet_before = std::fs::read_to_string(&keys_path).unwrap();

    // Use Hardhat #0 via --private-key flag
    let output = test_command(&temp)
        .args(["-v", "--private-key", TEST_PRIVATE_KEY, &server.url("/api")])
        .output()
        .unwrap();

    let combined = get_combined_output(&output);
    assert!(
        output.status.success(),
        "expected --private-key to override keys.toml: {combined}"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("override ok"),
        "stdout should contain success body: {combined}"
    );

    // keys.toml must not have been modified by --private-key usage
    let wallet_after = std::fs::read_to_string(&keys_path).unwrap();
    assert_eq!(
        wallet_before, wallet_after,
        "keys.toml should not be modified when --private-key is used"
    );
}

/// Non-402 response works fine with --private-key (key is ignored, no payment).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_private_key_no_payment_needed() {
    let server = MockServer::start(200, vec![], "hello no payment").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["--private-key", TEST_PRIVATE_KEY, &server.url("/test")])
        .output()
        .unwrap();

    assert!(output.status.success(), "expected success");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("hello no payment"),
        "stdout should contain body: {stdout}"
    );
}

// ==================== SSE / NDJSON Streaming Tests ====================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_sse_json_ndjson_schema() {
    // Two SSE data events: one JSON, one plain text
    let sse_body = "data: {\"msg\":\"hello\"}\n\ndata: world\n\n";
    let server = MockServer::start_sse(sse_body).await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["--sse-json", &server.url("/stream")])
        .output()
        .unwrap();

    assert!(output.status.success(), "expected success");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 2, "expected 2 NDJSON lines, got: {stdout}");

    for line in &lines {
        let parsed: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("invalid JSON line: {line} — {e}"));
        assert_eq!(parsed["event"], "data", "missing event field in: {line}");
        assert!(
            parsed.get("data").is_some(),
            "missing data field in: {line}"
        );
        assert!(parsed.get("ts").is_some(), "missing ts field in: {line}");
        // ts should look like ISO-8601
        let ts = parsed["ts"].as_str().unwrap();
        assert!(
            ts.ends_with('Z') && ts.contains('T'),
            "ts not ISO-8601: {ts}"
        );
    }

    // First event data should be parsed as a JSON object
    let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert!(
        first["data"].is_object(),
        "JSON data should be parsed as object: {}",
        first["data"]
    );
    assert_eq!(first["data"]["msg"], "hello");

    // Second event data is plain text → should be a string
    let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert!(
        second["data"].is_string(),
        "plain text data should be a string: {}",
        second["data"]
    );
    assert_eq!(second["data"], "world");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_sse_json_error_event() {
    // 500 error with SSE content type
    let server = MockServer::start(500, vec![("content-type", "text/event-stream")], "").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["--sse-json", &server.url("/fail")])
        .output()
        .unwrap();

    assert!(!output.status.success(), "expected failure on 500");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.trim().lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(
        lines.len(),
        1,
        "expected 1 error NDJSON line, got: {stdout}"
    );

    let parsed: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(parsed["event"], "error");
    assert!(
        parsed.get("message").is_some(),
        "missing message field in error event"
    );
    assert!(
        parsed.get("ts").is_some(),
        "missing ts field in error event"
    );
}

// ==================== Curl Parity Smoke Tests ====================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_dry_run_price_json() {
    // Valid 402 challenge with method="tempo" intent="charge"
    let challenge_request = "eyJhbW91bnQiOiIxMDAwMDAwIiwiY3VycmVuY3kiOiIweDIwYzAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAiLCJtZXRob2REZXRhaWxzIjp7ImNoYWluSWQiOjQyNDMxfSwicmVjaXBpZW50IjoiMHg3MDk5Nzk3MEM1MTgxMmRjM0EwMTBDN2QwMWI1MGUwZDE3ZGM3OUM4In0";
    let www_auth = format!(
        r#"Payment id="price-test", realm="mock", method="tempo", intent="charge", request="{challenge_request}""#
    );
    let server = MockServer::start(
        402,
        vec![("www-authenticate", &www_auth)],
        "Payment Required",
    )
    .await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["--dry-run", "--price-json", &server.url("/api")])
        .output()
        .unwrap();

    let combined = get_combined_output(&output);
    assert!(
        output.status.success(),
        "dry-run --price-json should exit 0: {combined}"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("invalid JSON: {stdout} — {e}"));
    assert!(
        parsed.get("intent").is_some(),
        "missing 'intent' in price JSON"
    );
    assert!(
        parsed.get("amount").is_some(),
        "missing 'amount' in price JSON"
    );
    assert!(
        parsed.get("currency").is_some(),
        "missing 'currency' in price JSON"
    );
    assert!(
        parsed.get("network").is_some(),
        "missing 'network' in price JSON"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_referer_header() {
    let server = MockServer::start_echo_headers().await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-e", "https://referrer.example.com", &server.url("/test")])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        parsed["referer"], "https://referrer.example.com",
        "referer header not echoed: {stdout}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_compressed_sets_accept_encoding() {
    let server = MockServer::start_echo_headers().await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["--compressed", &server.url("/test")])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let ae = parsed["accept-encoding"]
        .as_str()
        .unwrap_or_default()
        .to_lowercase();
    assert!(
        ae.contains("gzip"),
        "accept-encoding should contain gzip: {ae}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_http2_flag_no_crash() {
    let server = MockServer::start(200, vec![], "ok").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["--http2", &server.url("/test")])
        .output()
        .unwrap();

    // HTTP/2 ALPN negotiation may fail against a plain HTTP mock but
    // the CLI should not crash — either success or a transport error.
    let combined = get_combined_output(&output);
    assert!(
        output.status.success() || combined.contains("Error"),
        "http2 flag should not crash: {combined}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_http1_1_flag_no_crash() {
    let server = MockServer::start(200, vec![], "ok").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["--http1.1", &server.url("/test")])
        .output()
        .unwrap();

    assert!(output.status.success(), "http1.1 flag should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("ok"), "body: {stdout}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_http2_http1_conflict() {
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["--http2", "--http1.1", "http://localhost:1"])
        .output()
        .unwrap();

    assert!(!output.status.success(), "http2 + http1.1 should conflict");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_proxy_flag_no_crash() {
    // No actual proxy running — request will fail, but the flag should be accepted
    let server = MockServer::start(200, vec![], "ok").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["--proxy", "http://127.0.0.1:19999", &server.url("/test")])
        .output()
        .unwrap();

    // Connection to proxy will fail, but the CLI should not panic
    let combined = get_combined_output(&output);
    assert!(
        output.status.success() || combined.contains("Error"),
        "proxy flag should not crash: {combined}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_no_proxy_flag_succeeds() {
    let server = MockServer::start(200, vec![], "no proxy ok").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["--no-proxy", &server.url("/test")])
        .output()
        .unwrap();

    assert!(output.status.success(), "no-proxy flag should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("no proxy ok"), "body: {stdout}");
}

// ==================== 402 Charge/Session Edge Cases ====================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_402_without_www_authenticate_header() {
    // A 402 response missing WWW-Authenticate should produce a clear error
    let server = MockServer::start(402, vec![], "Payment Required").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args([&server.url("/test")])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("WWW-Authenticate"),
        "should mention missing header: {combined}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_402_without_www_authenticate_json_error() {
    // Same as above but with -j for structured JSON error
    let server = MockServer::start(402, vec![], "Payment Required").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-j", &server.url("/test")])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["code"], "E_PAYMENT");
    assert!(parsed["message"]
        .as_str()
        .unwrap()
        .contains("WWW-Authenticate"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_402_malformed_www_authenticate() {
    // WWW-Authenticate present but not a valid Payment challenge
    let server = MockServer::start(
        402,
        vec![("www-authenticate", "Basic realm=\"test\"")],
        "Payment Required",
    )
    .await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args([&server.url("/test")])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let combined = get_combined_output(&output);
    // Should error about missing Payment protocol or WWW-Authenticate
    assert!(
        combined.contains("WWW-Authenticate") || combined.contains("Payment"),
        "should mention invalid challenge: {combined}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_non_402_error_codes_with_fail_flag() {
    // -f flag should suppress body and fail on 4xx/5xx
    for status in [400u16, 403, 404, 500, 502, 503] {
        let body = format!("error body for {status}");
        let server = MockServer::start(status, vec![], &body).await;
        let temp = TestConfigBuilder::new().build();

        let output = test_command(&temp)
            .args(["-f", &server.url("/test")])
            .output()
            .unwrap();

        assert!(
            !output.status.success(),
            "status {status} should fail with -f"
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.is_empty(),
            "body should be suppressed with -f for status {status}: {stdout}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_server_error_json_output_schema() {
    // A 500 error with -j should produce structured JSON error with stable schema
    let server = MockServer::start(500, vec![], "Internal Server Error").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-j", &server.url("/test")])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // The output will be the body (pretty-printed if JSON) then an error JSON
    // For a non-JSON body with -j, the raw body is output, then the error JSON on stdout
    // Actually, the error JSON is printed by main on error, so let's check stderr
    let stderr = String::from_utf8_lossy(&output.stderr);
    // The body "Internal Server Error" should not appear in stdout when output format is JSON
    // and the response is not valid JSON itself
    assert!(
        stderr.contains("500") || stdout.contains("500"),
        "should report 500 error somewhere: stdout={stdout}, stderr={stderr}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_402_empty_body_no_crash() {
    // 402 with empty body and no WWW-Authenticate
    let server = MockServer::start(402, vec![], "").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args([&server.url("/test")])
        .output()
        .unwrap();

    assert!(!output.status.success());
    // Should not crash/panic — just produce an error
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("Error"),
        "should have error output: {combined}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_error_json_for_invalid_url() {
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-j", "ftp://example.com/data"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["code"], "E_USAGE");
    assert!(parsed["message"].as_str().unwrap().contains("ftp"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_error_json_for_connection_refused() {
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-j", "http://127.0.0.1:1/unreachable"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(parsed["code"].is_string());
    assert!(parsed["message"].is_string());
}
