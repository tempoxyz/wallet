//! Integration tests for tempo-wallet commands.

mod common;

use common::{
    assert_exit_code, get_combined_output, seed_local_session, test_command, MockServicesServer,
    TestConfigBuilder, MODERATO_DIRECT_KEYS_TOML,
};

// ==================== whoami ====================

#[test]
fn whoami_no_wallet_shows_not_logged_in() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp).arg("whoami").output().unwrap();

    assert!(
        output.status.success(),
        "whoami should succeed even without wallet"
    );
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("Not logged in") || combined.contains("not logged in"),
        "should mention not logged in: {combined}"
    );
}

#[test]
fn whoami_no_wallet_json_shape() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp).args(["-j", "whoami"]).output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["ready"], false, "should not be ready: {parsed}");
}

#[test]
fn whoami_with_wallet_json_has_wallet_field() {
    let temp = TestConfigBuilder::new()
        .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
        .build();

    let output = test_command(&temp).args(["-j", "whoami"]).output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(
        parsed["wallet"].is_string(),
        "should have wallet field: {parsed}"
    );
    assert!(
        parsed["wallet"].as_str().unwrap().starts_with("0x"),
        "wallet should be an address: {parsed}"
    );
}

#[test]
fn whoami_with_wallet_toon_shape() {
    let temp = TestConfigBuilder::new()
        .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
        .build();

    let output = test_command(&temp).args(["-t", "whoami"]).output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = toon_format::decode_default(stdout.trim()).unwrap();
    assert!(
        parsed["wallet"].is_string(),
        "TOON should have wallet: {parsed}"
    );
}

// ==================== list ====================

#[test]
fn list_empty_shows_no_wallets() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp).arg("list").output().unwrap();

    assert!(output.status.success());
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("No wallets") || combined.contains("0 wallet"),
        "should mention no wallets: {combined}"
    );
}

#[test]
fn list_empty_json_shape() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp).args(["-j", "list"]).output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(parsed["wallets"].is_array());
    assert_eq!(parsed["total"], 0);
}

#[test]
fn list_with_wallet_json_shape() {
    let temp = TestConfigBuilder::new()
        .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
        .build();

    let output = test_command(&temp).args(["-j", "list"]).output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(parsed["wallets"].is_array());
    assert!(parsed["total"].as_u64().unwrap() >= 1);
    let wallet = &parsed["wallets"][0];
    assert!(wallet["address"].is_string());
    assert!(wallet["wallet_type"].is_string());
}

#[test]
fn list_with_wallet_toon_shape() {
    let temp = TestConfigBuilder::new()
        .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
        .build();

    let output = test_command(&temp).args(["-t", "list"]).output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = toon_format::decode_default(stdout.trim()).unwrap();
    assert!(parsed["wallets"].is_array());
    assert!(parsed["total"].as_u64().unwrap() >= 1);
}

// ==================== create ====================

/// `create` requires OS keychain access (macOS Keychain / Linux secret-service),
/// which is only available in interactive sessions. This test verifies the
/// command is wired correctly by checking it produces an actionable error
/// when keychain access fails, or succeeds when it's available.
#[test]
fn create_runs_without_panic() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp).arg("create").output().unwrap();

    let combined = get_combined_output(&output);
    if output.status.success() {
        // Keychain was accessible — verify keys.toml was created
        let keys_path = temp.path().join(".tempo/wallet/keys.toml");
        assert!(keys_path.exists(), "keys.toml should be created");
        let keys_content = std::fs::read_to_string(&keys_path).unwrap();
        assert!(
            keys_content.contains("wallet_address"),
            "keys.toml should contain wallet_address: {keys_content}"
        );
    } else {
        // Keychain not accessible — should produce a clear error, not a panic
        assert!(
            combined.contains("Keychain") || combined.contains("keychain"),
            "should mention keychain error: {combined}"
        );
    }
}

// ==================== logout ====================

#[test]
fn logout_no_wallet_succeeds() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["logout", "--yes"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("Not logged in") || combined.contains("not logged in"),
        "should mention not logged in: {combined}"
    );
}

#[test]
fn logout_no_wallet_json_shape() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["-j", "logout", "--yes"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["logged_in"], false);
    assert_eq!(parsed["disconnected"], false);
}

// ==================== keys list ====================

#[test]
fn keys_list_empty() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp).args(["keys", "list"]).output().unwrap();

    assert!(output.status.success());
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("No keys") || combined.contains("0 key"),
        "should mention no keys: {combined}"
    );
}

#[test]
fn keys_list_json_shape() {
    let temp = TestConfigBuilder::new()
        .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
        .build();

    let output = test_command(&temp)
        .args(["-j", "keys", "list"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(parsed["keys"].is_array());
    assert!(parsed["total"].as_u64().unwrap() >= 1);
    let key = &parsed["keys"][0];
    assert!(key["address"].is_string());
}

#[test]
fn keys_list_toon_shape() {
    let temp = TestConfigBuilder::new()
        .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
        .build();

    let output = test_command(&temp)
        .args(["-t", "keys", "list"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = toon_format::decode_default(stdout.trim()).unwrap();
    assert!(parsed["keys"].is_array());
    assert!(parsed["total"].as_u64().unwrap() >= 1);
}

// ==================== sessions ====================

#[test]
fn sessions_list_empty_json() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["-j", "sessions", "list"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(parsed["sessions"].is_array());
    assert_eq!(parsed["total"], 0);
}

#[test]
fn sessions_list_with_session_json() {
    let temp = TestConfigBuilder::new().build();
    seed_local_session(&temp, "https://api.example.com");

    let output = test_command(&temp)
        .args(["-j", "sessions", "list"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(parsed["total"].as_u64().unwrap() >= 1);
    let session = &parsed["sessions"][0];
    assert!(session["origin"].is_string());
}

#[test]
fn sessions_list_state_all_json() {
    let temp = TestConfigBuilder::new().build();
    seed_local_session(&temp, "https://api.example.com");

    let output = test_command(&temp)
        .args(["-j", "sessions", "list", "--state", "all"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(parsed["sessions"].is_array());
}

#[test]
fn sessions_info_not_found_json() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["-j", "sessions", "info", "https://nonexistent.example.com"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["total"], 0);
}

#[test]
fn sessions_info_found_json() {
    let temp = TestConfigBuilder::new().build();
    seed_local_session(&temp, "https://api.example.com");

    let output = test_command(&temp)
        .args(["-j", "sessions", "info", "https://api.example.com"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(parsed["total"].as_u64().unwrap() >= 1);
}

#[test]
fn sessions_sync_empty_json() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["-j", "sessions", "sync"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["synced"], 0);
    assert_eq!(parsed["removed"], 0);
}

// ==================== services ====================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn services_category_filter() {
    let mock = MockServicesServer::start().await;
    let temp = TestConfigBuilder::new().build();

    // Filter by existing category
    let output = test_command(&temp)
        .env("TEMPO_SERVICES_URL", &mock.services_url)
        .args(["-j", "services", "--category", "ai"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(parsed.as_array().is_some_and(|a| !a.is_empty()));

    // Filter by non-existent category
    let output = test_command(&temp)
        .env("TEMPO_SERVICES_URL", &mock.services_url)
        .args(["-j", "services", "--category", "nonexistent"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(parsed.as_array().is_some_and(|a| a.is_empty()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn services_search_filter() {
    let mock = MockServicesServer::start().await;
    let temp = TestConfigBuilder::new().build();

    // Search for "openai" (matches the mock service)
    let output = test_command(&temp)
        .env("TEMPO_SERVICES_URL", &mock.services_url)
        .args(["-j", "services", "--search", "openai"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(parsed.as_array().is_some_and(|a| !a.is_empty()));

    // Search for something that doesn't match
    let output = test_command(&temp)
        .env("TEMPO_SERVICES_URL", &mock.services_url)
        .args(["-j", "services", "--search", "zzz_no_match"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(parsed.as_array().is_some_and(|a| a.is_empty()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn services_info_not_found() {
    let mock = MockServicesServer::start().await;
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .env("TEMPO_SERVICES_URL", &mock.services_url)
        .args(["services", "info", "nonexistent_service"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("not found"),
        "should mention not found: {combined}"
    );
}

// ==================== sign --dry-run structured output ====================

#[test]
fn sign_dry_run_valid_challenge_succeeds() {
    let temp = TestConfigBuilder::new().build();
    // Valid charge challenge for Tempo mainnet
    let challenge = r#"Payment id="test", realm="test", method="tempo", intent="charge", request="eyJhbW91bnQiOiIxMDAwIiwiY3VycmVuY3kiOiIweDIwYzAwMDAwMDAwMDAwMDAwMDAwMDAwMGI5NTM3ZDExYzYwZThiNTAiLCJtZXRob2REZXRhaWxzIjp7ImNoYWluSWQiOjQyMTd9fQ""#;

    let output = test_command(&temp)
        .args(["sign", "--dry-run", "--challenge", challenge])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Challenge is valid"),
        "should confirm valid challenge: {stderr}"
    );
}

#[test]
fn sign_dry_run_invalid_challenge_fails() {
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["sign", "--dry-run", "--challenge", "not a valid challenge"])
        .output()
        .unwrap();

    assert!(!output.status.success());
}

// ==================== version ====================

#[test]
fn version_flag_outputs_version() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp).arg("--version").output().unwrap();

    assert!(output.status.success());
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("tempo wallet"),
        "should show version: {combined}"
    );
}

// ==================== unknown subcommand ====================

#[test]
fn unknown_subcommand_fails() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp).arg("nonexistent").output().unwrap();

    assert_exit_code(&output, 2, "unknown subcommand should exit with E_USAGE");
}
