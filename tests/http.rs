//! HTTP behavior tests against a local axum mock server.
//!
//! These run in normal `cargo test` — no network or funded wallet required.

mod common;

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::any;
use axum::Router;

use crate::common::{get_combined_output, test_command, TestConfigBuilder};

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

        // Give the server a moment to start accepting connections
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        MockServer {
            base_url,
            shutdown_tx: Some(shutdown_tx),
            _handle: handle,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
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
    // Bind and immediately drop the listener to get a port with nothing listening
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .arg(format!("http://127.0.0.1:{port}/test"))
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "expected failure on connection refused"
    );
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("error")
            || combined.contains("connect")
            || combined.contains("Connection"),
        "output should mention connection error: {combined}"
    );
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
        .args(["-q", &server.url("/test")])
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
