use super::*;

#[test]
fn sessions_list_empty_returns_expected_json_shape() {
    let temp = TestConfigBuilder::new()
        .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
        .build();
    let output = test_command(&temp)
        .args(["-j", "sessions", "list"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(parsed["sessions"].is_array());
    assert!(parsed["sessions"]
        .as_array()
        .is_some_and(std::vec::Vec::is_empty));
    assert_eq!(parsed["total"], 0);
}

#[test]
fn sessions_list_requires_login() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["sessions", "list"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("No wallet configured"));
    assert!(stderr.contains("tempo wallet login"));
}

#[test]
fn sessions_list_seeded_channel_returns_expected_json_shape() {
    let temp = TestConfigBuilder::new()
        .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
        .build();
    seed_local_session(&temp, "https://api.example.com");

    let output = test_command(&temp)
        .args(["-j", "sessions", "list"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["total"], 1);
    assert!(parsed["sessions"].is_array());
    let session = &parsed["sessions"][0];
    assert!(session["channel_id"].is_string());
    assert_eq!(session["origin"], "https://api.example.com");
    assert!(session["deposit"].is_string());
    assert!(session["spent"].is_string());
    assert_eq!(session["status"], "active");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sessions_list_all_includes_orphaned_channels() {
    let rpc = CloseRpcServer::start(RpcCloseConfig {
        close_requested_at: 0,
        finalized: false,
        grace_period: 900,
        orphaned_log_channel_id: Some(ORPHANED_CHANNEL_ID),
    })
    .await;
    let temp = TestConfigBuilder::new()
        .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
        .with_config_toml(format!(
            "[rpc]\n\"tempo-moderato\" = \"{}\"\n",
            rpc.base_url
        ))
        .build();

    let output = test_command(&temp)
        .args(["-j", "-n", "tempo-moderato", "sessions", "list", "--all"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let sessions = parsed["sessions"]
        .as_array()
        .expect("sessions should be array");
    let orphaned = sessions
        .iter()
        .find(|item| item["channel_id"] == ORPHANED_CHANNEL_ID)
        .expect("expected orphaned channel in --all output");
    assert_eq!(orphaned["status"], "orphaned");
    assert!(
        orphaned.get("origin").is_none(),
        "orphaned channel should not include local origin: {orphaned}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sessions_list_orphaned_persists_discovered_channels() {
    let rpc = CloseRpcServer::start(RpcCloseConfig {
        close_requested_at: 0,
        finalized: false,
        grace_period: 900,
        orphaned_log_channel_id: Some(ORPHANED_CHANNEL_ID),
    })
    .await;
    let temp = TestConfigBuilder::new()
        .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
        .with_config_toml(format!(
            "[rpc]\n\"tempo-moderato\" = \"{}\"\n",
            rpc.base_url
        ))
        .build();

    let discover = test_command(&temp)
        .args([
            "-j",
            "-n",
            "tempo-moderato",
            "sessions",
            "list",
            "--orphaned",
        ])
        .output()
        .unwrap();
    assert!(discover.status.success());

    let list_again = test_command(&temp)
        .args(["-j", "-n", "tempo-moderato", "sessions", "list"])
        .output()
        .unwrap();
    assert!(list_again.status.success());

    let parsed: serde_json::Value =
        serde_json::from_slice(&list_again.stdout).expect("json output should parse");
    let sessions = parsed["sessions"]
        .as_array()
        .expect("sessions should be array");
    let orphaned = sessions
        .iter()
        .find(|item| item["channel_id"] == ORPHANED_CHANNEL_ID)
        .expect("expected persisted orphaned channel in subsequent list output");
    assert_eq!(orphaned["status"], "orphaned");
}

#[test]
fn sessions_list_emits_degraded_event_for_malformed_session_row() {
    let temp = TestConfigBuilder::new()
        .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
        .build();
    seed_local_session(&temp, "https://api.example.com");
    corrupt_local_session_deposit(&temp, "https://api.example.com", "not-a-number");

    let events_path = temp.path().join("events_session_degraded.log");
    let output = test_command(&temp)
        .env("TEMPO_TEST_EVENTS", events_path.to_str().unwrap())
        .args(["-j", "sessions", "list"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let events = parse_events_log(&events_path);
    let payload = events
        .iter()
        .find(|(name, _)| name == "session store degraded")
        .map_or_else(
            || panic!("missing session store degraded event: {events:?}"),
            |(_, payload)| payload,
        );

    assert!(
        payload["malformed_list_drops"].as_u64().unwrap_or(0) >= 1,
        "expected malformed_list_drops >= 1, got: {payload}"
    );
}
