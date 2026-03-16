use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sessions_close_cooperative_credential_shape_and_did_source() {
    let rpc = CloseRpcServer::start(RpcCloseConfig {
        close_requested_at: 0,
        finalized: false,
        grace_period: 900,
        orphaned_log_channel_id: None,
    })
    .await;
    let close_server = CooperativeCloseServer::start().await;
    let temp = TestConfigBuilder::new()
        .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
        .with_config_toml(format!(
            "[rpc]\n\"tempo-moderato\" = \"{}\"\n",
            rpc.base_url
        ))
        .build();

    seed_session_for_close(
        &temp,
        &close_server.base_url,
        &close_server.close_url(),
        4242,
    );

    let output = test_command(&temp)
        .args([
            "-j",
            "-n",
            "tempo-moderato",
            "sessions",
            "close",
            &close_server.base_url,
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "close should succeed: {}",
        get_combined_output(&output)
    );

    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("json output should parse");
    assert_eq!(parsed["closed"], 1);
    assert_eq!(parsed["pending"], 0);
    assert_eq!(parsed["failed"], 0);

    let observed = close_server.snapshot();
    assert_eq!(
        observed.prefetch_count, 1,
        "close flow should prefetch challenge once"
    );
    assert_eq!(
        observed.authorized_count, 1,
        "close flow should submit exactly one close credential"
    );
    assert_eq!(
        observed.close_channel_id.as_deref(),
        Some(SEEDED_CHANNEL_ID)
    );
    assert_eq!(observed.close_cumulative_amount.as_deref(), Some("4242"));
    assert!(
        observed
            .close_signature
            .as_deref()
            .is_some_and(|signature| signature.starts_with("0x")),
        "close signature should be present and hex encoded: {observed:?}"
    );
    assert_eq!(
        observed.credential_source.as_deref(),
        Some("did:pkh:eip155:42431:0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266"),
        "close credential source should be DID derived from payer"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sessions_close_request_close_transitions_to_pending_and_persists_countdown() {
    let rpc = CloseRpcServer::start(RpcCloseConfig {
        close_requested_at: 0,
        finalized: false,
        grace_period: 900,
        orphaned_log_channel_id: None,
    })
    .await;
    let temp = TestConfigBuilder::new()
        .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
        .with_config_toml(format!(
            "[rpc]\n\"tempo-moderato\" = \"{}\"\n",
            rpc.base_url
        ))
        .build();
    seed_session_for_close(
        &temp,
        "https://close.example",
        "http://127.0.0.1:1/unreachable",
        777,
    );

    let output = test_command(&temp)
        .args([
            "-j",
            "-n",
            "tempo-moderato",
            "sessions",
            "close",
            "https://close.example",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "close should succeed with pending outcome: {}",
        get_combined_output(&output)
    );

    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("json output should parse");
    assert_eq!(parsed["closed"], 0);
    assert_eq!(parsed["pending"], 1);
    assert_eq!(parsed["failed"], 0);
    assert_eq!(parsed["results"][0]["status"], "pending");
    assert_eq!(parsed["results"][0]["remaining_secs"], 900);

    let close_state = read_close_state(&temp).expect("pending close should keep local row");
    assert_eq!(close_state.state, "closing");
    assert!(close_state.close_requested_at > 0);
    assert_eq!(
        close_state.grace_ready_at,
        close_state.close_requested_at + 900,
        "pending close should persist grace countdown"
    );

    let observed = rpc.snapshot();
    assert_eq!(
        observed.send_raw_count, 1,
        "requestClose branch should submit exactly one tx"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sessions_close_withdraw_after_grace_elapsed() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let rpc = CloseRpcServer::start(RpcCloseConfig {
        close_requested_at: now.saturating_sub(1_000),
        finalized: false,
        grace_period: 900,
        orphaned_log_channel_id: None,
    })
    .await;
    let temp = TestConfigBuilder::new()
        .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
        .with_config_toml(format!(
            "[rpc]\n\"tempo-moderato\" = \"{}\"\n",
            rpc.base_url
        ))
        .build();
    seed_session_for_close(
        &temp,
        "https://close.example",
        "http://127.0.0.1:1/unreachable",
        777,
    );

    let output = test_command(&temp)
        .args([
            "-j",
            "-n",
            "tempo-moderato",
            "sessions",
            "close",
            "https://close.example",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "close should succeed with closed outcome: {}",
        get_combined_output(&output)
    );

    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("json output should parse");
    assert_eq!(parsed["closed"], 1);
    assert_eq!(parsed["pending"], 0);
    assert_eq!(parsed["failed"], 0);
    assert_eq!(parsed["results"][0]["status"], "closed");

    assert!(
        read_close_state(&temp).is_none(),
        "closed outcome should remove local session row"
    );
    let observed = rpc.snapshot();
    assert_eq!(
        observed.send_raw_count, 1,
        "withdraw branch should submit exactly one tx"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sessions_close_pending_before_grace_elapsed_submits_no_tx() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let close_requested_at = now.saturating_sub(100);
    let rpc = CloseRpcServer::start(RpcCloseConfig {
        close_requested_at,
        finalized: false,
        grace_period: 900,
        orphaned_log_channel_id: None,
    })
    .await;
    let temp = TestConfigBuilder::new()
        .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
        .with_config_toml(format!(
            "[rpc]\n\"tempo-moderato\" = \"{}\"\n",
            rpc.base_url
        ))
        .build();
    seed_session_for_close(
        &temp,
        "https://close.example",
        "http://127.0.0.1:1/unreachable",
        777,
    );

    let output = test_command(&temp)
        .args([
            "-j",
            "-n",
            "tempo-moderato",
            "sessions",
            "close",
            "https://close.example",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "close should succeed with pending outcome: {}",
        get_combined_output(&output)
    );

    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("json output should parse");
    assert_eq!(parsed["closed"], 0);
    assert_eq!(parsed["pending"], 1);
    assert_eq!(parsed["failed"], 0);
    assert_eq!(parsed["results"][0]["status"], "pending");
    let remaining_secs = parsed["results"][0]["remaining_secs"]
        .as_u64()
        .expect("pending close should include remaining_secs");
    assert!(
        (799..=800).contains(&remaining_secs),
        "remaining seconds should reflect pending grace window: {remaining_secs}"
    );

    let close_state = read_close_state(&temp).expect("pending close should keep local row");
    assert_eq!(close_state.state, "closing");
    assert_eq!(close_state.close_requested_at, close_requested_at);
    assert_eq!(close_state.grace_ready_at, close_requested_at + 900);

    let observed = rpc.snapshot();
    assert_eq!(
        observed.send_raw_count, 0,
        "pending branch must not submit a close tx before grace elapses"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sessions_close_channel_id_exercises_onchain_close_path() {
    let rpc = CloseRpcServer::start(RpcCloseConfig {
        close_requested_at: 0,
        finalized: false,
        grace_period: 900,
        orphaned_log_channel_id: None,
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
        .args([
            "-j",
            "-n",
            "tempo-moderato",
            "sessions",
            "close",
            ORPHANED_CHANNEL_ID,
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "close by channel ID should succeed: {}",
        get_combined_output(&output)
    );

    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("json output should parse");
    assert_eq!(parsed["closed"], 0);
    assert_eq!(parsed["pending"], 1);
    assert_eq!(parsed["failed"], 0);
    assert_eq!(parsed["results"][0]["channel_id"], ORPHANED_CHANNEL_ID);
    assert_eq!(parsed["results"][0]["status"], "pending");

    let observed = rpc.snapshot();
    assert_eq!(
        observed.send_raw_count, 1,
        "on-chain channel-ID close should submit exactly one requestClose tx"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sessions_close_all_closes_multiple_local_sessions() {
    let rpc = CloseRpcServer::start(RpcCloseConfig {
        close_requested_at: 0,
        finalized: false,
        grace_period: 900,
        orphaned_log_channel_id: None,
    })
    .await;
    let close_server = CooperativeCloseServer::start().await;
    let temp = TestConfigBuilder::new()
        .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
        .with_config_toml(format!(
            "[rpc]\n\"tempo-moderato\" = \"{}\"\n",
            rpc.base_url
        ))
        .build();

    seed_session_for_close(
        &temp,
        &close_server.base_url,
        &close_server.close_url(),
        4242,
    );
    insert_session_for_close(
        &temp,
        SECOND_CHANNEL_ID,
        "https://close-two.example",
        &close_server.close_url(),
        7777,
    );

    let output = test_command(&temp)
        .args(["-j", "-n", "tempo-moderato", "sessions", "close", "--all"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "close --all should succeed: {}",
        get_combined_output(&output)
    );

    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("json output should parse");
    assert_eq!(parsed["closed"], 2);
    assert_eq!(parsed["pending"], 0);
    assert_eq!(parsed["failed"], 0);
    assert_eq!(
        session_row_count(&temp),
        0,
        "closed sessions should be removed locally"
    );

    let observed = close_server.snapshot();
    assert_eq!(observed.prefetch_count, 2);
    assert_eq!(observed.authorized_count, 2);
    assert!(
        observed
            .close_channel_ids
            .contains(&SEEDED_CHANNEL_ID.to_string()),
        "first channel should be closed cooperatively: {observed:?}"
    );
    assert!(
        observed
            .close_channel_ids
            .contains(&SECOND_CHANNEL_ID.to_string()),
        "second channel should be closed cooperatively: {observed:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sessions_close_origin_closes_all_matching_channels() {
    let rpc = CloseRpcServer::start(RpcCloseConfig {
        close_requested_at: 0,
        finalized: false,
        grace_period: 900,
        orphaned_log_channel_id: None,
    })
    .await;
    let close_server = CooperativeCloseServer::start().await;
    let temp = TestConfigBuilder::new()
        .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
        .with_config_toml(format!(
            "[rpc]\n\"tempo-moderato\" = \"{}\"\n",
            rpc.base_url
        ))
        .build();

    seed_session_for_close(
        &temp,
        "https://close-origin.example",
        &close_server.close_url(),
        4242,
    );
    insert_session_for_close(
        &temp,
        SECOND_CHANNEL_ID,
        "https://close-origin.example",
        &close_server.close_url(),
        7777,
    );

    let output = test_command(&temp)
        .args([
            "-j",
            "-n",
            "tempo-moderato",
            "sessions",
            "close",
            "https://close-origin.example",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "close by origin should close all matching channels: {}",
        get_combined_output(&output)
    );

    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("json output should parse");
    assert_eq!(parsed["closed"], 2);
    assert_eq!(parsed["pending"], 0);
    assert_eq!(parsed["failed"], 0);
    assert_eq!(
        session_row_count(&temp),
        0,
        "all matching channels should be removed locally after close"
    );

    let observed = close_server.snapshot();
    assert_eq!(observed.prefetch_count, 2);
    assert_eq!(observed.authorized_count, 2);
}

#[test]
fn sessions_close_dry_run_without_target_requires_target_mode() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["sessions", "close", "--dry-run"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Specify a URL, channel ID"));
}

#[test]
fn sessions_close_requires_login() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["sessions", "close", "https://close.example"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("No wallet configured"));
    assert!(stderr.contains("tempo wallet login"));
}

#[test]
fn sessions_close_cooperative_rejects_incompatible_modes() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["sessions", "close", "--cooperative", "--all"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--cooperative cannot be combined"));
}
