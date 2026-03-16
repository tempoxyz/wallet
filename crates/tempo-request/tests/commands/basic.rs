//! Basic request command behavior tests split from commands.rs.

use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn non_402_get_request() {
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
async fn non_402_post_with_json() {
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
async fn include_headers_flag() {
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
async fn output_to_file() {
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
async fn server_error_500() {
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
async fn connection_refused() {
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
async fn status_402_without_valid_payment_header() {
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
async fn status_402_unsupported_payment_method() {
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
async fn dry_run_no_payment() {
    let server = MockServer::start(200, vec![], "dry run body").await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["--dry-run", &server.url("/test")])
        .output()
        .unwrap();

    assert!(output.status.success(), "dry run should succeed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn quiet_suppresses_logs() {
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
async fn verbose_shows_logs() {
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
async fn custom_header() {
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
async fn post_data_flag() {
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
async fn post_data_from_file() {
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
async fn multiple_data_flags() {
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
async fn retries_and_backoff_on_unreachable_host() {
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
async fn output_format_json() {
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
async fn no_redirect() {
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
async fn dump_header_writes_file() {
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
async fn timeout_flag() {
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
