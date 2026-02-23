//! Live end-to-end tests against real MPP endpoints.
//!
//! All tests are `#[ignore]` — they require `PRESTO_LIVE_TESTS=1` and a funded wallet.
//! Run with: `cargo test --test live -- --ignored --nocapture`

mod common;

use serial_test::serial;

use crate::common::{delete_sessions_db, get_combined_output, setup_live_test, test_command};

const ENDPOINT: &str = "https://openrouter.mpp.tempo.xyz/v1/chat/completions";
const REQUEST_BODY: &str =
    r#"{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"say hi"}]}"#;

/// Extract channel ID from session list output.
///
/// Looks for a hex address (0x...) on any line containing "Channel" (case-insensitive),
/// making this resilient to formatting changes in the output.
fn extract_channel_id(output: &str) -> Option<String> {
    for line in output.lines() {
        let lower = line.to_lowercase();
        if lower.contains("channel") {
            // Find a hex address (0x followed by hex chars)
            if let Some(start) = line.find("0x") {
                let hex_str: String = line[start..]
                    .chars()
                    .take_while(|c| c.is_ascii_hexdigit() || *c == 'x')
                    .collect();
                if hex_str.len() > 2 {
                    return Some(hex_str);
                }
            }
        }
    }
    None
}

/// Best-effort cleanup: close all sessions.
fn cleanup_sessions(temp: &tempfile::TempDir) {
    let _ = test_command(temp)
        .args(["session", "close", "--all"])
        .output();
}

#[tokio::test]
#[ignore]
#[serial]
async fn test_session_one_per_origin() {
    let Some(temp) = setup_live_test() else {
        return;
    };

    // First request
    let output = test_command(&temp)
        .args(["-X", "POST", "--json", REQUEST_BODY, ENDPOINT])
        .output()
        .unwrap();
    assert!(output.status.success(), "first request failed");

    let list_out = test_command(&temp)
        .args(["session", "list"])
        .output()
        .unwrap();
    let combined = get_combined_output(&list_out);
    assert!(
        combined.contains("1 session(s) total"),
        "expected 1 session after first request: {combined}"
    );

    // Second request (same origin)
    let output = test_command(&temp)
        .args(["-X", "POST", "--json", REQUEST_BODY, ENDPOINT])
        .output()
        .unwrap();
    assert!(output.status.success(), "second request failed");

    let list_out = test_command(&temp)
        .args(["session", "list"])
        .output()
        .unwrap();
    let combined = get_combined_output(&list_out);
    assert!(
        combined.contains("1 session(s) total"),
        "expected still 1 session after second request: {combined}"
    );

    cleanup_sessions(&temp);
}

#[tokio::test]
#[ignore]
#[serial]
async fn test_session_close() {
    let Some(temp) = setup_live_test() else {
        return;
    };

    // Open a channel
    let output = test_command(&temp)
        .args(["-X", "POST", "--json", REQUEST_BODY, ENDPOINT])
        .output()
        .unwrap();
    assert!(output.status.success(), "request failed");

    let list_out = test_command(&temp)
        .args(["session", "list"])
        .output()
        .unwrap();
    let combined = get_combined_output(&list_out);
    assert!(
        combined.contains("1 session(s) total"),
        "expected session to exist: {combined}"
    );

    // Close by URL
    let close_out = test_command(&temp)
        .args(["session", "close", ENDPOINT])
        .output()
        .unwrap();
    let combined = get_combined_output(&close_out);
    assert!(
        combined.contains("Session closed"),
        "expected 'Session closed': {combined}"
    );

    // Verify it's gone
    let list_out = test_command(&temp)
        .args(["session", "list"])
        .output()
        .unwrap();
    let combined = get_combined_output(&list_out);
    assert!(
        combined.contains("No active sessions"),
        "expected no sessions after close: {combined}"
    );
}

#[tokio::test]
#[ignore]
#[serial]
async fn test_session_recover() {
    let Some(temp) = setup_live_test() else {
        return;
    };

    // Open a channel
    let output = test_command(&temp)
        .args(["-X", "POST", "--json", REQUEST_BODY, ENDPOINT])
        .output()
        .unwrap();
    assert!(output.status.success(), "request failed");

    // Capture channel ID
    let list_out = test_command(&temp)
        .args(["session", "list"])
        .output()
        .unwrap();
    let list_combined = get_combined_output(&list_out);
    assert!(
        list_combined.contains("1 session(s) total"),
        "expected session: {list_combined}"
    );
    let channel_before =
        extract_channel_id(&list_combined).expect("could not extract channel ID before recovery");

    // Delete sessions DB to simulate data loss
    delete_sessions_db(&temp);

    let list_out = test_command(&temp)
        .args(["session", "list"])
        .output()
        .unwrap();
    let combined = get_combined_output(&list_out);
    assert!(
        combined.contains("No active sessions"),
        "expected no sessions after DB delete: {combined}"
    );

    // Recover from on-chain state
    let recover_out = test_command(&temp)
        .args(["session", "recover", ENDPOINT])
        .output()
        .unwrap();
    let recover_combined = get_combined_output(&recover_out);
    assert!(
        recover_combined.contains("Session recovered from on-chain state"),
        "expected recovery message: {recover_combined}"
    );

    // Verify recovered channel matches
    let channel_after =
        extract_channel_id(&recover_combined).expect("could not extract channel ID after recovery");
    assert_eq!(
        channel_before, channel_after,
        "recovered channel should match original"
    );

    // Verify session list shows it
    let list_out = test_command(&temp)
        .args(["session", "list"])
        .output()
        .unwrap();
    let combined = get_combined_output(&list_out);
    assert!(
        combined.contains("1 session(s) total"),
        "expected session after recovery: {combined}"
    );

    cleanup_sessions(&temp);
}

#[tokio::test]
#[ignore]
#[serial]
async fn test_session_auto_recover() {
    let Some(temp) = setup_live_test() else {
        return;
    };

    // Open a channel
    let output = test_command(&temp)
        .args(["-X", "POST", "--json", REQUEST_BODY, ENDPOINT])
        .output()
        .unwrap();
    assert!(output.status.success(), "request failed");

    // Capture channel ID
    let list_out = test_command(&temp)
        .args(["session", "list"])
        .output()
        .unwrap();
    let list_combined = get_combined_output(&list_out);
    let channel_before =
        extract_channel_id(&list_combined).expect("could not extract channel ID before deletion");

    // Delete sessions DB
    delete_sessions_db(&temp);

    let list_out = test_command(&temp)
        .args(["session", "list"])
        .output()
        .unwrap();
    let combined = get_combined_output(&list_out);
    assert!(
        combined.contains("No active sessions"),
        "expected no sessions after DB delete: {combined}"
    );

    // Make another request with -v (should auto-recover)
    let request_out = test_command(&temp)
        .args(["-v", "-X", "POST", "--json", REQUEST_BODY, ENDPOINT])
        .output()
        .unwrap();
    assert!(request_out.status.success(), "auto-recover request failed");

    let stderr = String::from_utf8_lossy(&request_out.stderr);
    assert!(
        stderr.contains("Recovered channel from on-chain state"),
        "expected auto-recovery message in stderr: {stderr}"
    );

    // Verify session list shows 1 session with matching channel
    let list_out = test_command(&temp)
        .args(["session", "list"])
        .output()
        .unwrap();
    let list_combined = get_combined_output(&list_out);
    assert!(
        list_combined.contains("1 session(s) total"),
        "expected 1 session after auto-recovery: {list_combined}"
    );

    let channel_after = extract_channel_id(&list_combined)
        .expect("could not extract channel ID after auto-recovery");
    assert_eq!(
        channel_before, channel_after,
        "auto-recovered channel should match original"
    );

    cleanup_sessions(&temp);
}
