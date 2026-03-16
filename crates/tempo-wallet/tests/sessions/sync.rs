use super::*;

#[test]
fn sessions_sync_empty_returns_expected_json_shape() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["-j", "sessions", "sync"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(parsed["sessions"].is_array());
    assert_eq!(parsed["total"], 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sessions_sync_origin_reconciles_closing_state_and_grace_ready_at() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let close_requested_at = now.saturating_sub(60);
    let rpc = CloseRpcServer::start(RpcCloseConfig {
        close_requested_at,
        finalized: false,
        grace_period: 900,
        orphaned_log_channel_id: None,
    })
    .await;
    let temp = TestConfigBuilder::new()
        .with_config_toml(format!(
            "[rpc]\n\"tempo-moderato\" = \"{}\"\n",
            rpc.base_url
        ))
        .build();
    seed_session_for_close(
        &temp,
        "https://sync.example",
        "http://127.0.0.1:1/unreachable",
        777,
    );

    let output = test_command(&temp)
        .args([
            "-j",
            "-n",
            "tempo-moderato",
            "sessions",
            "sync",
            "--origin",
            "https://sync.example",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "sync --origin should succeed: {}",
        get_combined_output(&output)
    );

    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("json output should parse");
    assert_eq!(parsed["recovered"], true);
    assert_eq!(parsed["status"], "closing");
    assert!(
        parsed["remaining_secs"]
            .as_u64()
            .is_some_and(|remaining| remaining > 0 && remaining <= 900),
        "sync should report remaining close grace period: {parsed}"
    );

    let close_state =
        read_close_state(&temp).expect("sync should keep local row and update close state");
    assert_eq!(close_state.state, "closing");
    assert_eq!(close_state.close_requested_at, close_requested_at);
    assert_eq!(close_state.grace_ready_at, close_requested_at + 900);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sessions_sync_origin_updates_all_matching_channels() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let close_requested_at = now.saturating_sub(60);
    let rpc = CloseRpcServer::start(RpcCloseConfig {
        close_requested_at,
        finalized: false,
        grace_period: 900,
        orphaned_log_channel_id: None,
    })
    .await;
    let temp = TestConfigBuilder::new()
        .with_config_toml(format!(
            "[rpc]\n\"tempo-moderato\" = \"{}\"\n",
            rpc.base_url
        ))
        .build();
    seed_session_for_close(
        &temp,
        "https://sync-many.example",
        "http://127.0.0.1:1/unreachable",
        777,
    );
    insert_session_for_close(
        &temp,
        SECOND_CHANNEL_ID,
        "https://sync-many.example",
        "http://127.0.0.1:1/unreachable",
        888,
    );

    let output = test_command(&temp)
        .args([
            "-j",
            "-n",
            "tempo-moderato",
            "sessions",
            "sync",
            "--origin",
            "https://sync-many.example",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "sync --origin should process all matching channels: {}",
        get_combined_output(&output)
    );

    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("json output should parse");
    assert_eq!(parsed["processed"], 2);
    assert_eq!(parsed["recovered_count"], 2);
    assert_eq!(parsed["removed_count"], 0);
    assert!(
        parsed["results"].is_array() && parsed["results"].as_array().unwrap().len() == 2,
        "sync response should include one per-channel result: {parsed}"
    );

    let first = read_close_state(&temp).expect("first channel should remain present");
    assert_eq!(first.state, "closing");
    assert_eq!(first.close_requested_at, close_requested_at);
    assert_eq!(first.grace_ready_at, close_requested_at + 900);

    let second =
        read_close_state_for(&temp, SECOND_CHANNEL_ID).expect("second channel should remain present");
    assert_eq!(second.state, "closing");
    assert_eq!(second.close_requested_at, close_requested_at);
    assert_eq!(second.grace_ready_at, close_requested_at + 900);
}
