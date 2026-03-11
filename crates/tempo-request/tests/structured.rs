//! Snapshot-like structure tests for JSON and TOON outputs.

use axum::routing::get;
use axum::{Json, Router};
use serde_json::Value;

mod common;
use common::{assert_exit_code, test_command, MockServer, TestConfigBuilder};

fn run_both(
    temp: &tempfile::TempDir,
    args: &[&str],
) -> (std::process::Output, Value, std::process::Output, Value) {
    tempo_test::run_structured_both(test_command, temp, args)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn query_json_and_toon_body_shape() {
    let app = Router::new().route(
        "/json",
        get(|| async { Json(serde_json::json!({"ok": true, "count": 2})) }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });

    let temp = TestConfigBuilder::new().build();
    let url = format!("http://{addr}/json");
    let (json_out, json, toon_out, toon) = run_both(&temp, &[&url]);
    tempo_test::assert_clean_stderr(&json_out);
    tempo_test::assert_clean_stderr(&toon_out);
    assert_eq!(json["ok"], true);
    assert_eq!(json["count"], 2);
    assert_eq!(toon["ok"], true);
    assert_eq!(toon["count"], 2);

    let _ = shutdown_tx.send(());
    let _ = server.await;
}

// ── Phase 4: Structured output gap tests ────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn toon_error_402_without_www_authenticate() {
    let server = MockServer::start(402, vec![], "Payment Required").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-t", &server.url("/paid")])
        .output()
        .unwrap();

    assert_exit_code(
        &output,
        4,
        "402 without WWW-Authenticate should exit with E_PAYMENT",
    );
    let parsed = tempo_test::parse_toon_stdout(&output);
    assert_eq!(parsed["code"], "E_PAYMENT");
    assert!(
        parsed["message"]
            .as_str()
            .unwrap()
            .contains("WWW-Authenticate"),
        "message should mention WWW-Authenticate: {}",
        parsed["message"]
    );
}

#[test]
fn toon_error_invalid_url() {
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-t", "ftp://example.com/data"])
        .output()
        .unwrap();

    assert_exit_code(&output, 2, "invalid URL scheme should exit with E_USAGE");
    let parsed = tempo_test::parse_toon_stdout(&output);
    assert_eq!(parsed["code"], "E_USAGE");
    assert!(
        parsed["message"].as_str().unwrap().contains("ftp"),
        "message should mention ftp: {}",
        parsed["message"]
    );
}

#[test]
fn toon_error_connection_refused() {
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-t", "http://127.0.0.1:1/unreachable"])
        .output()
        .unwrap();

    assert_exit_code(&output, 1, "connection refused should exit with E_GENERAL");
    let parsed = tempo_test::parse_toon_stdout(&output);
    assert!(
        parsed["code"].is_string(),
        "code should be a string: {parsed}"
    );
    assert!(
        parsed["message"].is_string(),
        "message should be a string: {parsed}"
    );
}

#[test]
fn toon_error_offline() {
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-t", "--offline", "http://example.com/api"])
        .output()
        .unwrap();

    assert_exit_code(&output, 3, "offline mode should exit with E_NETWORK");
    let parsed = tempo_test::parse_toon_stdout(&output);
    assert_eq!(parsed["code"], "E_NETWORK");
    assert!(
        parsed["message"].as_str().unwrap().contains("--offline"),
        "message should mention --offline: {}",
        parsed["message"]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn toon_error_server_500() {
    let server = MockServer::start(500, vec![], "Internal Server Error").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-t", &server.url("/error")])
        .output()
        .unwrap();

    assert_exit_code(&output, 3, "500 error should exit with E_NETWORK");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("500"),
        "output should mention 500: {stdout}"
    );
}

#[test]
fn version_json_output() {
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-j", "--version"])
        .output()
        .unwrap();

    assert!(output.status.success(), "version should succeed");
    let parsed: Value = serde_json::from_slice(&output.stdout).expect("valid JSON");
    assert!(parsed["version"].is_string(), "version should be a string");
    assert!(
        parsed["git_commit"].is_string(),
        "git_commit should be a string"
    );
    assert!(
        parsed["build_date"].is_string(),
        "build_date should be a string"
    );
    assert!(parsed["profile"].is_string(), "profile should be a string");
}

#[test]
fn version_toon_output() {
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-t", "--version"])
        .output()
        .unwrap();

    assert!(output.status.success(), "version should succeed");
    let parsed = tempo_test::parse_toon_stdout(&output);
    assert!(parsed["version"].is_string(), "version should be a string");
    assert!(
        parsed["git_commit"].is_string(),
        "git_commit should be a string"
    );
    assert!(
        parsed["build_date"].is_string(),
        "build_date should be a string"
    );
    assert!(parsed["profile"].is_string(), "profile should be a string");
}

#[test]
fn describe_outputs_schema() {
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp).arg("--describe").output().unwrap();

    assert!(output.status.success(), "--describe should succeed");
    let parsed: Value = serde_json::from_slice(&output.stdout).expect("valid JSON");
    assert!(parsed["name"].is_string(), "name should be a string");
    assert!(parsed.get("args").is_some(), "args field should be present");
}
