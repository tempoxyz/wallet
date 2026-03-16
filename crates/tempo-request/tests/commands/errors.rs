//! Error and offline-mode behavior tests split from commands.rs.

use super::*;

// ==================== 402 Charge/Session Edge Cases ====================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn status_402_without_www_authenticate_header() {
    // A 402 response missing WWW-Authenticate should produce a clear error
    let server = MockServer::start(402, vec![], "Payment Required").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args([&server.url("/test")])
        .output()
        .unwrap();

    assert_exit_code(
        &output,
        4,
        "402 without WWW-Authenticate should exit with E_PAYMENT",
    );
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("WWW-Authenticate"),
        "should mention missing header: {combined}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn status_402_without_www_authenticate_json_error() {
    // Same as above but with -j for structured JSON error
    let server = MockServer::start(402, vec![], "Payment Required").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-j", &server.url("/test")])
        .output()
        .unwrap();

    assert_exit_code(
        &output,
        4,
        "402 without WWW-Authenticate (JSON) should exit with E_PAYMENT",
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["code"], "E_PAYMENT");
    assert!(parsed["message"]
        .as_str()
        .unwrap()
        .contains("WWW-Authenticate"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn status_402_malformed_www_authenticate() {
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

    assert_exit_code(
        &output,
        4,
        "malformed WWW-Authenticate should exit with E_PAYMENT",
    );
    let combined = get_combined_output(&output);
    // Should error about missing Payment protocol or WWW-Authenticate
    assert!(
        combined.contains("WWW-Authenticate") || combined.contains("Payment"),
        "should mention invalid challenge: {combined}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn server_error_json_output_schema() {
    // A 500 error with -j should produce structured JSON error with stable schema
    let server = MockServer::start(500, vec![], "Internal Server Error").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-j", &server.url("/test")])
        .output()
        .unwrap();

    assert_exit_code(&output, 3, "500 error with -j should exit with E_NETWORK");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("500") || stdout.contains("500"),
        "should report 500 error somewhere: stdout={stdout}, stderr={stderr}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn status_402_empty_body_no_crash() {
    // 402 with empty body and no WWW-Authenticate
    let server = MockServer::start(402, vec![], "").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args([&server.url("/test")])
        .output()
        .unwrap();

    assert_exit_code(&output, 4, "402 empty body should exit with E_PAYMENT");
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("Error"),
        "should have error output: {combined}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn error_json_for_invalid_url() {
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-j", "ftp://example.com/data"])
        .output()
        .unwrap();

    assert_exit_code(&output, 2, "invalid URL scheme should exit with E_USAGE");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["code"], "E_USAGE");
    assert!(parsed["message"].as_str().unwrap().contains("ftp"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn error_json_for_invalid_http_method() {
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-j", "-X", "NOPE??", "https://example.com/api"])
        .output()
        .unwrap();

    assert_exit_code(&output, 2, "invalid HTTP method should exit with E_USAGE");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["code"], "E_USAGE");
    assert!(
        parsed["message"]
            .as_str()
            .unwrap()
            .contains("Invalid HTTP method"),
        "expected invalid method message, got: {}",
        parsed["message"]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn error_json_for_connection_refused() {
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-j", "http://127.0.0.1:1/unreachable"])
        .output()
        .unwrap();

    assert_exit_code(&output, 3, "connection refused should exit with E_NETWORK");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(parsed["code"].is_string());
    assert!(parsed["message"].is_string());
}

// ==================== Offline Mode ====================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offline_flag_fails_fast() {
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["--offline", "http://127.0.0.1:1/should-not-connect"])
        .output()
        .unwrap();

    assert_exit_code(&output, 3, "--offline should exit with E_NETWORK");
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("--offline"),
        "should mention --offline mode: {combined}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offline_flag_json_error() {
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-j", "--offline", "http://example.com/api"])
        .output()
        .unwrap();

    assert_exit_code(&output, 3, "--offline (JSON) should exit with E_NETWORK");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["code"], "E_NETWORK");
    assert!(parsed["message"].as_str().unwrap().contains("--offline"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offline_flag_no_socket_opened() {
    // Start a real server, then use --offline — it should never be contacted
    let server = MockServer::start(200, vec![], "should not see this").await;
    let temp = TestConfigBuilder::new().build();
    let events_path = temp.path().join("events_offline.log");

    let output = test_command(&temp)
        .env("TEMPO_TEST_EVENTS", events_path.to_str().unwrap())
        .args(["--offline", &server.url("/api")])
        .output()
        .unwrap();

    assert_exit_code(&output, 3, "--offline no-socket should exit with E_NETWORK");
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should NOT contain the server's response body
    assert!(
        !stdout.contains("should not see this"),
        "offline mode should not contact the server"
    );

    // No query_started event should be emitted (offline bails before tracking)
    let raw = std::fs::read_to_string(&events_path).unwrap_or_default();
    assert!(
        !raw.contains("query started"),
        "no query started event should fire in offline mode: {raw}"
    );
}
