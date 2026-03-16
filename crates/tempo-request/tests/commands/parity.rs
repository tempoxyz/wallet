//! Curl parity smoke tests split from commands.rs.

use super::*;

// ==================== Curl Parity Smoke Tests ====================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn referer_header() {
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
async fn compressed_sets_accept_encoding() {
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
async fn http2_flag_no_crash() {
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
async fn http1_1_flag_no_crash() {
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
async fn http2_http1_conflict() {
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
async fn proxy_flag_no_crash() {
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
async fn no_proxy_flag_succeeds() {
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
