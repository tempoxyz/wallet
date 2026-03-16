//! Integration tests for tempo-request commands.
//!
//! These run in normal `cargo test` — no network or funded wallet required.

mod common;

use crate::common::{
    assert_exit_code, charge_www_authenticate_with_realm, get_combined_output, setup_config_only,
    test_command, write_test_files, MockRpcServer, MockServer, PaymentTestHarness,
    TestConfigBuilder, HARDHAT_PRIVATE_KEY, MODERATO_CHARGE_CHALLENGE,
};

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

    assert_exit_code(&output, 3, "500 error should exit with E_NETWORK");
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

        assert_exit_code(&output, 3, "connection refused should exit with E_NETWORK");
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
async fn test_402_without_valid_payment_header() {
    let server = MockServer::start(402, vec![], "Payment Required").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .arg(server.url("/paid"))
        .output()
        .unwrap();

    assert_exit_code(
        &output,
        4,
        "402 without payment header should exit with E_PAYMENT",
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
    let www_auth = format!(
        r#"Payment id="test-unsupported", realm="mock", method="other", intent="charge", request="{MODERATO_CHARGE_CHALLENGE}""#
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

    assert_exit_code(
        &output,
        4,
        "unsupported payment method should exit with E_PAYMENT",
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

    assert_exit_code(&output, 3, "unreachable host should exit with E_NETWORK");
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

// ==================== 402 Charge Payment Flow ====================

/// Test the full 402 → payment → 200 charge flow with mock servers.
///
/// Verifies that tempo-request correctly:
/// 1. Receives a 402 with WWW-Authenticate header
/// 2. Parses the MPP payment challenge
/// 3. Loads wallet keys
/// 4. Builds and signs a payment credential via mock RPC
/// 5. Retries the request with Authorization header
/// 6. Returns the final 200 response body
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_402_charge_flow() {
    let h = PaymentTestHarness::charge_with_body("charge accepted").await;

    let output = test_command(&h.temp)
        .args(["-v", &h.url("/api")])
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

/// Missing receipts remain warning-only in default mode.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_402_charge_flow_default_mode_allows_missing_receipt() {
    let h = PaymentTestHarness::charge_with_body("default receipt policy ok").await;

    let output = test_command(&h.temp)
        .args([&h.url("/api")])
        .output()
        .unwrap();

    let combined = get_combined_output(&output);
    assert!(
        output.status.success(),
        "default mode should not fail on missing receipt: {combined}"
    );
}

/// Strict receipt mode fails successful paid responses that omit Payment-Receipt.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_402_charge_flow_strict_receipts_requires_header() {
    let h = PaymentTestHarness::charge_with_body("strict receipt policy should fail").await;

    let output = test_command(&h.temp)
        .args(["--strict-receipts", &h.url("/api")])
        .output()
        .unwrap();

    assert_exit_code(
        &output,
        4,
        "strict receipts should fail missing Payment-Receipt with E_PAYMENT",
    );
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("Missing required Payment-Receipt"),
        "strict receipt failure should mention missing Payment-Receipt: {combined}"
    );
}

/// Payment narration is printed at -v when encountering a 402
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_402_payment_narration_verbose() {
    let h = PaymentTestHarness::charge().await;

    let output = test_command(&h.temp)
        .args(["-v", &h.url("/api")])
        .output()
        .unwrap();

    assert!(output.status.success(), "expected success");
    let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
    assert!(
        stderr.contains("payment required:"),
        "should narrate 402 payment requirement: {stderr}"
    );
}

/// Paid summary prints with -v and is suppressed by default / -q
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_402_paid_summary_verbose_and_quiet() {
    let h = PaymentTestHarness::charge_with_id("test-charge-paid", "ok").await;

    // Default (no -v): summary should NOT be printed
    let output_default = test_command(&h.temp)
        .args([&h.url("/api")])
        .output()
        .unwrap();
    assert!(
        output_default.status.success(),
        "default run should succeed"
    );
    let stderr_default = String::from_utf8_lossy(&output_default.stderr);
    assert!(
        !stderr_default.contains("Paid "),
        "expected no paid summary in default mode, got: {stderr_default}"
    );

    // Verbose: summary should be printed
    let output_verbose = test_command(&h.temp)
        .args(["-v", &h.url("/api")])
        .output()
        .unwrap();
    assert!(
        output_verbose.status.success(),
        "verbose run should succeed"
    );
    let stderr_verbose = String::from_utf8_lossy(&output_verbose.stderr);
    assert!(
        stderr_verbose.contains("Paid "),
        "expected paid summary in verbose mode, got: {stderr_verbose}"
    );

    // Quiet: summary should be suppressed
    let output_quiet = test_command(&h.temp)
        .args(["-s", &h.url("/api")])
        .output()
        .unwrap();
    assert!(output_quiet.status.success(), "quiet run should succeed");
    let stderr_quiet = String::from_utf8_lossy(&output_quiet.stderr);
    assert!(
        !stderr_quiet.contains("Paid "),
        "expected no paid summary in quiet mode, got: {stderr_quiet}"
    );
}

/// Analytics `PaymentSuccess` `tx_hash` should be the extracted hex, not the raw header
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_analytics_tx_hash_is_extracted_hex() {
    // Simple 64-nybble hex hash
    let tx_hash = format!("0x{}", "ab".repeat(32));
    let receipt_value = format!("tx={tx_hash}");
    let h = PaymentTestHarness::charge_with_receipt("ok", &receipt_value).await;

    // Set up analytics tap file
    let events_path = h.temp.path().join("events.log");

    let output = test_command(&h.temp)
        .env("TEMPO_TEST_EVENTS", events_path.to_str().unwrap())
        .args(["-v", &h.url("/api")])
        .output()
        .unwrap();
    assert!(output.status.success(), "expected success");

    // Read captured events and find payment succeeded
    let content = std::fs::read_to_string(&events_path).unwrap();
    let mut found = None;
    for line in content.lines() {
        if let Some((name, json_str)) = line.split_once('|') {
            if name == "payment succeeded" {
                found = Some(json_str.to_string());
            }
        }
    }
    let Some(json_str) = found else {
        panic!("missing payment succeeded event: {content}");
    };
    let v: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    let got = v.get("tx_hash").and_then(|x| x.as_str()).unwrap_or("");
    // Accept either an exact extracted 0x-hash, or empty (if server didn't supply a
    // parseable receipt). Critically, it must NOT be the raw header with fields.
    let is_hex =
        got.starts_with("0x") && got.len() == 66 && got[2..].chars().all(|c| c.is_ascii_hexdigit());
    assert!(
        got.is_empty() || is_hex,
        "tx_hash should be empty or a 0x-hex hash, got: {got}"
    );
    assert!(
        !got.contains('='),
        "tx_hash should not be a raw header with fields: {got}"
    );
}

/// Test the 402 → payment → 200 charge flow with Keychain signing mode.
///
/// Uses a different `wallet_address` than the derived address of the private
/// key, which triggers `TempoSigningMode::Keychain` instead of `Direct`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_402_charge_flow_keychain() {
    let h = PaymentTestHarness::charge_keychain("keychain charge accepted").await;

    let output = test_command(&h.temp)
        .args(["-v", &h.url("/api")])
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

/// The 402 charge flow works with --private-key (no keys.toml needed).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_402_charge_flow_with_private_key_flag() {
    let rpc = MockRpcServer::start(42431).await;
    let server = MockServer::start_payment_deferred("private key charge ok").await;
    let www_auth = charge_www_authenticate_with_realm("test-pk", &server.base_url);
    server.set_www_authenticate(&www_auth);

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let output = test_command(&temp)
        .args([
            "-v",
            "--private-key",
            HARDHAT_PRIVATE_KEY,
            &server.url("/api"),
        ])
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

/// --private-key via `TEMPO_PRIVATE_KEY` env var works.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_402_charge_flow_with_private_key_env() {
    let rpc = MockRpcServer::start(42431).await;
    let server = MockServer::start_payment_deferred("env key charge ok").await;
    let www_auth = charge_www_authenticate_with_realm("test-pk-env", &server.base_url);
    server.set_www_authenticate(&www_auth);

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let output = test_command(&temp)
        .env("TEMPO_PRIVATE_KEY", HARDHAT_PRIVATE_KEY)
        .args(["-v", &server.url("/api")])
        .output()
        .unwrap();

    let combined = get_combined_output(&output);
    assert!(
        output.status.success(),
        "expected TEMPO_PRIVATE_KEY charge flow to succeed: {combined}"
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
    let server = MockServer::start_payment_deferred("no prefix ok").await;
    let www_auth = charge_www_authenticate_with_realm("test-pk-no0x", &server.base_url);
    server.set_www_authenticate(&www_auth);

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    // Strip the 0x prefix
    let pk_no_prefix = HARDHAT_PRIVATE_KEY.strip_prefix("0x").unwrap();

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
    let server = MockServer::start_payment_deferred("should not reach").await;
    let www_auth = charge_www_authenticate_with_realm("test-pk-bad", &server.base_url);
    server.set_www_authenticate(&www_auth);

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let output = test_command(&temp)
        .args(["--private-key", "not-a-valid-key", &server.url("/api")])
        .output()
        .unwrap();

    assert_exit_code(&output, 2, "invalid private key should exit with E_USAGE");
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
    let server = MockServer::start_payment_deferred("should not reach").await;
    let www_auth = charge_www_authenticate_with_realm("test-pk-short", &server.base_url);
    server.set_www_authenticate(&www_auth);

    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let output = test_command(&temp)
        .args(["--private-key", "0xdeadbeef", &server.url("/api")])
        .output()
        .unwrap();

    assert_exit_code(&output, 2, "too-short private key should exit with E_USAGE");
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
    let server = MockServer::start_payment_deferred("override ok").await;
    let www_auth = charge_www_authenticate_with_realm("test-pk-override", &server.base_url);
    server.set_www_authenticate(&www_auth);

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
    let keys_path = temp.path().join(".tempo/wallet/keys.toml");
    let wallet_before = std::fs::read_to_string(&keys_path).unwrap();

    // Use Hardhat #0 via --private-key flag
    let output = test_command(&temp)
        .args([
            "-v",
            "--private-key",
            HARDHAT_PRIVATE_KEY,
            &server.url("/api"),
        ])
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
        .args(["--private-key", HARDHAT_PRIVATE_KEY, &server.url("/test")])
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

    assert_exit_code(&output, 3, "SSE 500 error should exit with E_NETWORK");
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

    assert_exit_code(
        &output,
        2,
        "http2 + http1.1 conflict should exit with E_USAGE",
    );
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
async fn test_402_without_www_authenticate_json_error() {
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
async fn test_server_error_json_output_schema() {
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
async fn test_402_empty_body_no_crash() {
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
async fn test_error_json_for_invalid_url() {
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
async fn test_error_json_for_invalid_http_method() {
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
async fn test_error_json_for_connection_refused() {
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
async fn test_offline_flag_fails_fast() {
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
async fn test_offline_flag_json_error() {
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
async fn test_offline_flag_no_socket_opened() {
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

// ==================== Analytics Events Sequencing & Redaction ====================

/// Helper to parse the `TEMPO_TEST_EVENTS` file into a list of (`event_name`, `props_json`).
fn parse_events_log(path: &std::path::Path) -> Vec<(String, serde_json::Value)> {
    let content = std::fs::read_to_string(path).unwrap_or_default();
    content
        .lines()
        .filter_map(|line| {
            let (name, json_str) = line.split_once('|')?;
            let v: serde_json::Value = serde_json::from_str(json_str).ok()?;
            Some((name.to_string(), v))
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_analytics_event_sequence_success() {
    let server = MockServer::start(200, vec![], "ok").await;
    let temp = TestConfigBuilder::new().build();
    let events_path = temp.path().join("events_seq_success.log");

    let output = test_command(&temp)
        .env("TEMPO_TEST_EVENTS", events_path.to_str().unwrap())
        .args([&server.url("/api/data")])
        .output()
        .unwrap();
    assert!(output.status.success());

    let events = parse_events_log(&events_path);
    let names: Vec<&str> = events.iter().map(|(n, _)| n.as_str()).collect();

    // Must contain the core sequence in order
    assert!(
        names.contains(&"command succeeded"),
        "missing command succeeded: {names:?}"
    );
    assert!(
        names.contains(&"query started"),
        "missing query started: {names:?}"
    );
    assert!(
        names.contains(&"query succeeded"),
        "missing query succeeded: {names:?}"
    );

    // Verify ordering: query started before query succeeded
    let pos = |name: &str| names.iter().position(|n| *n == name);
    assert!(pos("query started") < pos("query succeeded"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_analytics_event_sequence_failure() {
    let temp = TestConfigBuilder::new().build();
    let events_path = temp.path().join("events_seq_failure.log");

    // Connect to a port nothing is listening on
    let output = test_command(&temp)
        .env("TEMPO_TEST_EVENTS", events_path.to_str().unwrap())
        .args(["http://127.0.0.1:1/unreachable"])
        .output()
        .unwrap();
    assert_exit_code(&output, 3, "connection failure should exit with E_NETWORK");

    let events = parse_events_log(&events_path);
    let names: Vec<&str> = events.iter().map(|(n, _)| n.as_str()).collect();

    assert!(
        names.contains(&"query started"),
        "missing query started: {names:?}"
    );
    assert!(
        names.contains(&"query failed"),
        "missing query failed: {names:?}"
    );
    assert!(pos_of(&names, "query started") < pos_of(&names, "query failed"));

    // Verify query failed has a sanitized error (not empty, bounded)
    let failure_props = events
        .iter()
        .find(|(n, _)| n == "query failed")
        .map(|(_, v)| v)
        .unwrap();
    let err = failure_props["error"].as_str().unwrap();
    assert!(!err.is_empty(), "error should not be empty");
    assert!(err.len() <= 203, "error should be truncated"); // 200 + "…" (3 bytes)
}

fn pos_of(names: &[&str], target: &str) -> usize {
    names
        .iter()
        .position(|n| *n == target)
        .unwrap_or_else(|| panic!("event {target} not found in {names:?}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_analytics_url_query_params_redacted() {
    let server = MockServer::start(200, vec![], "ok").await;
    let temp = TestConfigBuilder::new().build();
    let events_path = temp.path().join("events_url_redact.log");

    // URL with sensitive query parameters
    let url_with_secrets = format!(
        "{}/api/data?api_key=sk_live_secret123&token=bearer_xyz",
        server.base_url
    );

    let output = test_command(&temp)
        .env("TEMPO_TEST_EVENTS", events_path.to_str().unwrap())
        .args([&url_with_secrets])
        .output()
        .unwrap();
    assert!(output.status.success());

    let events = parse_events_log(&events_path);

    // Check all events that contain a "url" field
    for (name, props) in &events {
        if let Some(url_val) = props.get("url").and_then(|v| v.as_str()) {
            assert!(
                !url_val.contains("sk_live_secret123"),
                "event '{name}' leaks api_key in url: {url_val}"
            );
            assert!(
                !url_val.contains("bearer_xyz"),
                "event '{name}' leaks token in url: {url_val}"
            );
            assert!(
                !url_val.contains('?'),
                "event '{name}' has query params in url: {url_val}"
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_analytics_bearer_token_not_leaked() {
    let server = MockServer::start(200, vec![], "ok").await;
    let temp = TestConfigBuilder::new().build();
    let events_path = temp.path().join("events_bearer.log");

    let output = test_command(&temp)
        .env("TEMPO_TEST_EVENTS", events_path.to_str().unwrap())
        .args(["--bearer", "super_secret_token_12345", &server.url("/api")])
        .output()
        .unwrap();
    assert!(output.status.success());

    // Read the raw file content and verify the bearer token is nowhere in it
    let raw = std::fs::read_to_string(&events_path).unwrap();
    assert!(
        !raw.contains("super_secret_token_12345"),
        "bearer token leaked into analytics events: {raw}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_analytics_basic_auth_not_leaked() {
    let server = MockServer::start(200, vec![], "ok").await;
    let temp = TestConfigBuilder::new().build();
    let events_path = temp.path().join("events_basic.log");

    let output = test_command(&temp)
        .env("TEMPO_TEST_EVENTS", events_path.to_str().unwrap())
        .args(["-u", "admin:s3cretP@ss", &server.url("/api")])
        .output()
        .unwrap();
    assert!(output.status.success());

    let raw = std::fs::read_to_string(&events_path).unwrap();
    assert!(
        !raw.contains("s3cretP@ss"),
        "basic auth password leaked into analytics: {raw}"
    );
    assert!(
        !raw.contains("admin:"),
        "basic auth username leaked into analytics: {raw}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_analytics_custom_auth_header_not_leaked() {
    let server = MockServer::start(200, vec![], "ok").await;
    let temp = TestConfigBuilder::new().build();
    let events_path = temp.path().join("events_custom_auth.log");

    let output = test_command(&temp)
        .env("TEMPO_TEST_EVENTS", events_path.to_str().unwrap())
        .args([
            "-H",
            "Authorization: Bearer my_private_jwt_token",
            &server.url("/api"),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());

    let raw = std::fs::read_to_string(&events_path).unwrap();
    assert!(
        !raw.contains("my_private_jwt_token"),
        "custom auth header leaked into analytics: {raw}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_analytics_private_key_env_not_leaked() {
    let server = MockServer::start(200, vec![], "ok").await;
    let temp = TestConfigBuilder::new().build();
    let events_path = temp.path().join("events_pk.log");

    // TEMPO_PRIVATE_KEY is used for payment signing, not for the HTTP request itself,
    // but verify it never appears in analytics output
    let output = test_command(&temp)
        .env("TEMPO_TEST_EVENTS", events_path.to_str().unwrap())
        .env(
            "TEMPO_PRIVATE_KEY",
            "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
        )
        .args([&server.url("/api")])
        .output()
        .unwrap();
    assert!(output.status.success());

    let raw = std::fs::read_to_string(&events_path).unwrap();
    assert!(
        !raw.contains("deadbeefdeadbeef"),
        "private key leaked into analytics: {raw}"
    );
}

// ==================== Verbose Log Redaction ====================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_verbose_log_redacts_url_query_params() {
    let server = MockServer::start(200, vec![], "ok").await;
    let temp = TestConfigBuilder::new().build();

    let url_with_secret = format!(
        "{}/api?api_key=sk_live_XYZZY&token=t0p_secret",
        server.base_url
    );

    let output = test_command(&temp)
        .args(["-v", &url_with_secret])
        .output()
        .unwrap();
    assert!(output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("sk_live_XYZZY"),
        "api_key leaked in verbose log: {stderr}"
    );
    assert!(
        !stderr.contains("t0p_secret"),
        "token leaked in verbose log: {stderr}"
    );
    // The path should still be present
    assert!(
        stderr.contains("/api"),
        "verbose log should show the path: {stderr}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_verbose_log_redacts_bearer_in_stderr() {
    let server = MockServer::start(200, vec![], "ok").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-v", "--bearer", "my_super_secret_jwt", &server.url("/api")])
        .output()
        .unwrap();
    assert!(output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("my_super_secret_jwt"),
        "bearer token leaked in verbose stderr: {stderr}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_verbose_log_redacts_basic_auth_in_stderr() {
    let server = MockServer::start(200, vec![], "ok").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-v", "-u", "admin:hunter2", &server.url("/api")])
        .output()
        .unwrap();
    assert!(output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("hunter2"),
        "basic auth password leaked in verbose stderr: {stderr}"
    );
}

// ==================== TOON Input/Output ====================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_toon_output_pretty_prints_json_response() {
    let server = MockServer::start(200, vec![], r#"{"name":"Alice","age":30}"#).await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-t", &server.url("/test")])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.starts_with('{'),
        "TOON output should not start with '{{': {stdout}"
    );
    assert!(stdout.contains("name"), "should contain 'name': {stdout}");
    assert!(stdout.contains("Alice"), "should contain 'Alice': {stdout}");
    assert!(stdout.contains("age"), "should contain 'age': {stdout}");
    assert!(stdout.contains("30"), "should contain '30': {stdout}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_toon_output_non_json_response_passthrough() {
    let server = MockServer::start(200, vec![], "hello world").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-t", &server.url("/test")])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("hello world"),
        "non-JSON body should pass through: {stdout}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_toon_input_sets_content_type_json() {
    let server = MockServer::start_echo_headers().await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["--toon", "name: Alice", &server.url("/test")])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let ct = parsed["content-type"].as_str().unwrap();
    assert!(
        ct.contains("application/json"),
        "content-type should be application/json: {ct}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_toon_input_invalid_data_errors() {
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["--toon", "[invalid{toon", "http://127.0.0.1:1/test"])
        .output()
        .unwrap();

    assert_exit_code(&output, 2, "invalid TOON input should exit with E_USAGE");
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("TOON") || combined.contains("decode"),
        "should mention TOON parse error: {combined}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_toon_and_json_input_conflict() {
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["--json", "{}", "--toon", "x: 1", "http://127.0.0.1:1/test"])
        .output()
        .unwrap();

    assert_exit_code(
        &output,
        2,
        "--json + --toon conflict should exit with E_USAGE",
    );
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("cannot be used with")
            || combined.contains("conflict")
            || combined.contains("--json"),
        "should mention conflict: {combined}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_head_flag_sends_head_method() {
    let server = MockServer::start_echo_request().await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-I", &server.url("/test")])
        .output()
        .unwrap();

    assert!(output.status.success(), "HEAD request should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    // -I implies include headers, so stdout starts with HTTP status line
    assert!(
        stdout.contains("HTTP 200"),
        "HEAD should show headers: {stdout}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_stream_flag_outputs_body() {
    let server = MockServer::start(200, vec![], "streamed body content").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["--stream", &server.url("/test")])
        .output()
        .unwrap();

    assert!(output.status.success(), "stream should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("streamed body content"),
        "stream should output body: {stdout}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_sse_passthrough_outputs_raw_events() {
    let sse_body = "data: hello\n\ndata: world\n\n";
    let server = MockServer::start_sse(sse_body).await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["--sse", &server.url("/stream")])
        .output()
        .unwrap();

    assert!(output.status.success(), "SSE passthrough should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    // --sse outputs raw SSE text (unlike --sse-json which converts to NDJSON)
    assert!(
        stdout.contains("data: hello") || stdout.contains("hello"),
        "SSE passthrough should contain event data: {stdout}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_user_agent_flag_sends_custom_agent() {
    let server = MockServer::start_echo_headers().await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-A", "MyCustomAgent/1.0", &server.url("/test")])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        parsed["user-agent"], "MyCustomAgent/1.0",
        "user-agent not set correctly: {stdout}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_bearer_flag_sends_authorization_header() {
    let server = MockServer::start_echo_headers().await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["--bearer", "test_token_abc", &server.url("/test")])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        parsed["authorization"], "Bearer test_token_abc",
        "bearer header not echoed: {stdout}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_remote_name_saves_to_derived_filename() {
    let server = MockServer::start(200, vec![], "remote content").await;
    let temp = TestConfigBuilder::new().build();

    // Run from the temp directory so -O writes there
    let output = test_command(&temp)
        .current_dir(temp.path())
        .args(["-O", &server.url("/path/download.txt")])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "remote-name should succeed: {}",
        get_combined_output(&output)
    );
    let saved = temp.path().join("download.txt");
    assert!(saved.exists(), "file should be saved as download.txt");
    let contents = std::fs::read_to_string(&saved).unwrap();
    assert!(
        contents.contains("remote content"),
        "saved file should contain body: {contents}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_data_urlencode_sends_encoded_body() {
    let server = MockServer::start_echo_request().await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["--data-urlencode", "msg=hello world", &server.url("/test")])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let body = parsed["body"].as_str().unwrap();
    assert!(
        body.contains("msg=hello%20world"),
        "body should contain URL-encoded data: {body}"
    );
    let ct = parsed["headers"]["content-type"].as_str().unwrap_or("");
    assert!(
        ct.contains("application/x-www-form-urlencoded"),
        "content-type should be form-urlencoded: {ct}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_get_flag_appends_data_to_query() {
    let server = MockServer::start_echo_request().await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-G", "-d", "key=value", &server.url("/test")])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(parsed["method"], "GET", "-G should force GET method");
    let query = parsed["query"].as_str().unwrap();
    assert!(
        query.contains("key=value"),
        "query string should contain data: {query}"
    );
    // Body should be empty when using -G
    let body = parsed["body"].as_str().unwrap();
    assert!(body.is_empty(), "body should be empty with -G: {body}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_get_flag_with_data_urlencode() {
    let server = MockServer::start_echo_request().await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args([
            "-G",
            "--data-urlencode",
            "q=hello world",
            &server.url("/search"),
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(parsed["method"], "GET");
    let query = parsed["query"].as_str().unwrap();
    assert!(
        query.contains("q=hello%20world"),
        "query should contain encoded data: {query}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_write_meta_creates_json_file() {
    let server = MockServer::start(200, vec![], "meta test body").await;
    let temp = TestConfigBuilder::new().build();

    let meta_path = temp.path().join("meta.json");
    let output = test_command(&temp)
        .args([
            "--write-meta",
            meta_path.to_str().unwrap(),
            &server.url("/test"),
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "write-meta should succeed: {}",
        get_combined_output(&output)
    );
    assert!(meta_path.exists(), "meta file should exist");
    let meta_content = std::fs::read_to_string(&meta_path).unwrap();
    let meta: serde_json::Value = serde_json::from_str(&meta_content).unwrap();
    assert_eq!(meta["status"], 200);
    assert!(meta["url"].is_string());
    assert!(meta["elapsed_ms"].is_number());
    assert!(meta["bytes"].is_number());
    assert!(meta["headers"].is_object());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_max_redirs_limits_redirects() {
    // Server that always redirects to itself
    let server = MockServer::start(
        302,
        vec![("location", "http://127.0.0.1:1/redirect")],
        "redirecting",
    )
    .await;
    let temp = TestConfigBuilder::new().build();

    // Follow redirects with max 0 → should fail immediately
    let output = test_command(&temp)
        .args(["-L", "--max-redirs", "0", &server.url("/start")])
        .output()
        .unwrap();

    // With max-redirs=0, the redirect loop hits 0 and the CLI gets a redirect response
    // but stops following. This may succeed (returning 302) or error depending on implementation.
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("302") || combined.contains("redirect") || !output.status.success(),
        "should hit redirect limit: {combined}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_retry_http_retries_on_specified_codes() {
    // Server returns 503 — with --retry-http 503, the CLI should retry
    let server = MockServer::start(503, vec![], "Service Unavailable").await;
    let temp = TestConfigBuilder::new().build();

    let start = std::time::Instant::now();
    let output = test_command(&temp)
        .args([
            "--retries",
            "1",
            "--retry-backoff",
            "10",
            "--retry-http",
            "503",
            &server.url("/test"),
        ])
        .output()
        .unwrap();
    let elapsed = start.elapsed();

    assert!(!output.status.success(), "should fail after retries");
    // Should have waited at least the backoff time (retried once)
    assert!(
        elapsed.as_millis() >= 10,
        "should have retried with backoff: elapsed={elapsed:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_network_mismatch_preserves_router_wording_and_exit_class() {
    let server = MockServer::start_payment_deferred("Payment Required").await;
    let www_auth = charge_www_authenticate_with_realm("test-network-mismatch", &server.base_url);
    server.set_www_authenticate(&www_auth);
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .env("TEMPO_PRIVATE_KEY", HARDHAT_PRIVATE_KEY)
        .args(["--network", "tempo", &server.url("/paid")])
        .output()
        .unwrap();

    assert_exit_code(
        &output,
        4,
        "challenge network mismatch should exit with E_PAYMENT",
    );
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("Server requested network 'tempo-moderato' but --network is 'tempo'"),
        "should preserve router mismatch wording: {combined}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_stream_with_include_headers() {
    let server = MockServer::start(200, vec![("x-stream", "yes")], "stream+headers").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["--stream", "-i", &server.url("/test")])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("HTTP 200"),
        "should include status line: {stdout}"
    );
    assert!(
        stdout.contains("x-stream: yes"),
        "should include custom header: {stdout}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_sse_json_with_write_meta() {
    let sse_body = "data: test\n\n";
    let server = MockServer::start_sse(sse_body).await;
    let temp = TestConfigBuilder::new().build();

    let meta_path = temp.path().join("sse_meta.json");
    let output = test_command(&temp)
        .args([
            "--sse-json",
            "--write-meta",
            meta_path.to_str().unwrap(),
            &server.url("/stream"),
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(meta_path.exists(), "meta file should be created for SSE");
    let meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&meta_path).unwrap()).unwrap();
    assert_eq!(meta["status"], 200);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_data_urlencode_from_file() {
    let server = MockServer::start_echo_request().await;
    let temp = TestConfigBuilder::new().build();

    let data_file = temp.path().join("encode_data.txt");
    std::fs::write(&data_file, "hello world & special=chars").unwrap();

    let arg = format!("field=@{}", data_file.to_str().unwrap());
    let output = test_command(&temp)
        .args(["--data-urlencode", &arg, &server.url("/test")])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let body = parsed["body"].as_str().unwrap();
    // The file content should be URL-encoded
    assert!(
        body.contains("field="),
        "body should contain the field name: {body}"
    );
    assert!(!body.contains(' '), "spaces should be URL-encoded: {body}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_connect_timeout_flag() {
    let server = MockServer::start(200, vec![], "ok").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["--connect-timeout", "5", &server.url("/test")])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "connect-timeout should succeed for reachable server"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("ok"), "body: {stdout}");
}

/// Verbose logs must never contain the raw private key material.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_private_key_not_leaked_in_verbose_logs() {
    let rpc = MockRpcServer::start(42431).await;
    let server = MockServer::start_payment_deferred("pk leak test ok").await;
    let www_auth = charge_www_authenticate_with_realm("test-pk-leak", &server.base_url);
    server.set_www_authenticate(&www_auth);
    let temp = tempfile::TempDir::new().unwrap();
    setup_config_only(&temp, &rpc.base_url);

    let output = test_command(&temp)
        .args([
            "-v",
            "--private-key",
            HARDHAT_PRIVATE_KEY,
            &server.url("/api"),
        ])
        .output()
        .unwrap();

    let combined = get_combined_output(&output);
    assert!(
        output.status.success(),
        "expected charge flow to succeed: {combined}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"),
        "stderr must not contain the raw private key (without 0x prefix)"
    );
    assert!(
        !stderr.contains(HARDHAT_PRIVATE_KEY),
        "stderr must not contain the full private key"
    );
}

/// Writing response body to a file with `-o` preserves content exactly.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_binary_response_written_to_file() {
    let body = "binary-like-data-\t\n-special-chars-end";
    let server = MockServer::start(200, vec![], body).await;
    let temp = TestConfigBuilder::new().build();
    let out_file = temp.path().join("output.bin");

    let output = test_command(&temp)
        .args(["-o", out_file.to_str().unwrap(), &server.url("/download")])
        .output()
        .unwrap();

    assert!(output.status.success(), "expected success");
    assert!(out_file.exists(), "output file should be created");
    let contents = std::fs::read_to_string(&out_file).unwrap();
    assert_eq!(
        contents, body,
        "file content should match response body exactly"
    );
}

/// `-o` to a path inside a non-existent directory should fail.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_output_file_in_nonexistent_directory_fails() {
    let server = MockServer::start(200, vec![], "should not write").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args([
            "-o",
            "/nonexistent_dir_xyz/output.txt",
            &server.url("/test"),
        ])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "writing to non-existent directory should fail"
    );
}

/// `-o` path traversal should be rejected.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_output_file_path_traversal_rejected() {
    let server = MockServer::start(200, vec![], "should not write").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-o", "../escape.txt", &server.url("/test")])
        .output()
        .unwrap();

    assert!(!output.status.success(), "path traversal should fail");
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("path traversal") || combined.contains("Invalid output path"),
        "error should mention invalid output path: {combined}"
    );
}

/// Verbose payment flow must not leak private key material in stderr.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_payment_credential_not_leaked_in_verbose_logs() {
    let h = PaymentTestHarness::charge_with_body("auth redact ok").await;

    let output = test_command(&h.temp)
        .args(["-v", &h.url("/api")])
        .output()
        .unwrap();

    let combined = get_combined_output(&output);
    assert!(
        output.status.success(),
        "expected payment flow to succeed: {combined}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"),
        "stderr must not contain wallet private key material"
    );
}

/// An empty URL argument should fail with `E_USAGE` (exit code 2).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_empty_url_fails_with_usage_error() {
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp).arg("").output().unwrap();

    assert_exit_code(&output, 2, "empty URL should exit with E_USAGE");
}

/// `--json` and `--toon` input format flags conflict.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_json_toon_format_conflict() {
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["--json", "{}", "--toon", "x: 1", "http://127.0.0.1:1/test"])
        .output()
        .unwrap();

    assert_exit_code(
        &output,
        2,
        "--json + --toon format conflict should exit with E_USAGE",
    );
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("cannot be used with")
            || combined.contains("conflict")
            || combined.contains("--json"),
        "should mention conflict: {combined}"
    );
}

/// A response with a very large header value (1000+ chars) is handled without crash.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_large_header_value_handled() {
    let large_value: String = "X".repeat(2000);
    let server = MockServer::start(200, vec![("x-large-header", &large_value)], "ok").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-i", &server.url("/test")])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "large header value should not crash"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(&large_value),
        "stdout should contain the large header value"
    );
}

// ── Realm validation ────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_realm_bare_fqdn_accepted() {
    // Realm set as bare host:port (no scheme) — should match.
    let rpc = MockRpcServer::start(42431).await;
    let server = MockServer::start_payment_deferred("ok").await;
    let port = server
        .base_url
        .strip_prefix("http://")
        .unwrap()
        .split(':')
        .next_back()
        .unwrap();
    let realm = format!("127.0.0.1:{port}");
    let www_auth = format!(
        r#"Payment id="test-bare", realm="{realm}", method="tempo", intent="charge", request="{MODERATO_CHARGE_CHALLENGE}""#
    );
    server.set_www_authenticate(&www_auth);
    let temp = TestConfigBuilder::new()
        .with_keys_toml(tempo_test::fixture::MODERATO_DIRECT_KEYS_TOML)
        .with_config_toml(format!(
            "[rpc]\n\"tempo-moderato\" = \"{}\"\n",
            rpc.base_url
        ))
        .build();

    let output = test_command(&temp)
        .arg(server.url("/paid"))
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "bare FQDN realm should be accepted: {}",
        get_combined_output(&output)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_realm_with_scheme_prefix_accepted() {
    // Realm set as "http://host:port" — should also match.
    let rpc = MockRpcServer::start(42431).await;
    let server = MockServer::start_payment_deferred("ok").await;
    let realm_with_scheme = &server.base_url; // "http://127.0.0.1:PORT"
    let www_auth = format!(
        r#"Payment id="test-scheme", realm="{realm_with_scheme}", method="tempo", intent="charge", request="{MODERATO_CHARGE_CHALLENGE}""#
    );
    server.set_www_authenticate(&www_auth);
    let temp = TestConfigBuilder::new()
        .with_keys_toml(tempo_test::fixture::MODERATO_DIRECT_KEYS_TOML)
        .with_config_toml(format!(
            "[rpc]\n\"tempo-moderato\" = \"{}\"\n",
            rpc.base_url
        ))
        .build();

    let output = test_command(&temp)
        .arg(server.url("/paid"))
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "realm with scheme prefix should be accepted: {}",
        get_combined_output(&output)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_realm_mismatch_rejected() {
    // Realm set to a completely different host — should be rejected.
    let server = MockServer::start_payment_deferred("ok").await;
    let www_auth = format!(
        r#"Payment id="test-mismatch", realm="evil.example.com", method="tempo", intent="charge", request="{MODERATO_CHARGE_CHALLENGE}""#
    );
    server.set_www_authenticate(&www_auth);
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .env("TEMPO_PRIVATE_KEY", HARDHAT_PRIVATE_KEY)
        .arg(server.url("/paid"))
        .output()
        .unwrap();

    assert_exit_code(&output, 4, "mismatched realm should exit with E_PAYMENT");
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("does not match request host"),
        "should report realm mismatch: {combined}"
    );
}
