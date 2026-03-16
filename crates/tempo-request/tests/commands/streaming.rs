//! SSE/streaming output tests split from commands.rs.

use super::*;

// ==================== SSE / NDJSON Streaming Tests ====================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sse_json_ndjson_schema() {
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
async fn sse_json_error_event() {
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
