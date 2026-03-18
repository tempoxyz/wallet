//! Analytics and redaction tests split from commands.rs.

use super::*;

// ==================== Analytics Events Sequencing & Redaction ====================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn analytics_event_sequence_success() {
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
async fn analytics_event_sequence_failure() {
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
async fn analytics_url_query_params_redacted() {
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
async fn analytics_bearer_token_not_leaked() {
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
async fn analytics_basic_auth_not_leaked() {
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
async fn analytics_custom_auth_header_not_leaked() {
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
async fn analytics_private_key_env_not_leaked() {
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
async fn verbose_log_redacts_url_query_params() {
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
async fn verbose_log_redacts_bearer_in_stderr() {
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
async fn verbose_log_redacts_basic_auth_in_stderr() {
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
