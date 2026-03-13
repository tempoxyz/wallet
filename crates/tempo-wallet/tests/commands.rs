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

// ==================== keys ====================

#[test]
fn keys_empty() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp).arg("keys").output().unwrap();

    assert!(output.status.success());
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("No keys") || combined.contains("0 key"),
        "should mention no keys: {combined}"
    );
}

#[test]
fn keys_json_shape() {
    let temp = TestConfigBuilder::new()
        .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
        .build();

    let output = test_command(&temp).args(["-j", "keys"]).output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(parsed["keys"].is_array());
    assert!(parsed["total"].as_u64().unwrap() >= 1);
    let key = &parsed["keys"][0];
    assert!(key["address"].is_string());
    assert!(key["key"].is_string(), "JSON should include private key");
}

#[test]
fn keys_toon_shape() {
    let temp = TestConfigBuilder::new()
        .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
        .build();

    let output = test_command(&temp).args(["-t", "keys"]).output().unwrap();

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
fn sessions_sync_empty_json() {
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

// ==================== services ====================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn services_category_filter() {
    let mock = MockServicesServer::start().await;
    let temp = TestConfigBuilder::new().build();

    // Filter by existing category
    let output = test_command(&temp)
        .env("TEMPO_SERVICES_URL", &mock.services_url)
        .args(["-j", "services", "--search", "ai"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(parsed.as_array().is_some_and(|a| !a.is_empty()));

    // Filter by non-existent category
    let output = test_command(&temp)
        .env("TEMPO_SERVICES_URL", &mock.services_url)
        .args(["-j", "services", "--search", "nonexistent"])
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
        .args(["services", "nonexistent_service"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("not found"),
        "should mention not found: {combined}"
    );
}

// ==================== mpp-sign ====================

/// Valid charge challenge for Tempo mainnet (chainId 4217).
const VALID_CHARGE_CHALLENGE: &str = r#"Payment id="test", realm="test", method="tempo", intent="charge", request="eyJhbW91bnQiOiIxMDAwIiwiY3VycmVuY3kiOiIweDIwYzAwMDAwMDAwMDAwMDAwMDAwMDAwMGI5NTM3ZDExYzYwZThiNTAiLCJtZXRob2REZXRhaWxzIjp7ImNoYWluSWQiOjQyMTd9fQ""#;

#[test]
fn sign_help_shows_flags() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["mpp-sign", "--help"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let combined = get_combined_output(&output);
    assert!(combined.contains("--challenge"), "should show --challenge");
    assert!(combined.contains("--dry-run"), "should show --dry-run");
}

#[test]
fn sign_dry_run_valid_challenge_succeeds() {
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args([
            "mpp-sign",
            "--dry-run",
            "--challenge",
            VALID_CHARGE_CHALLENGE,
        ])
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
fn sign_dry_run_json_emits_structured() {
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args([
            "-j",
            "mpp-sign",
            "--dry-run",
            "--challenge",
            VALID_CHARGE_CHALLENGE,
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["valid"], true);
    assert_eq!(parsed["method"], "tempo");
}

#[test]
fn sign_dry_run_invalid_challenge_fails() {
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args([
            "mpp-sign",
            "--dry-run",
            "--challenge",
            "not a valid challenge",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
}

#[test]
fn sign_dry_run_unsupported_method() {
    let temp = TestConfigBuilder::new().build();
    let challenge = r#"Payment id="x", realm="x", method="stripe", intent="charge", request="e30""#;

    let output = test_command(&temp)
        .args(["mpp-sign", "--dry-run", "--challenge", challenge])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Unsupported"),
        "should mention unsupported: {stderr}"
    );
}

#[test]
fn sign_dry_run_missing_chain_id() {
    let temp = TestConfigBuilder::new().build();
    // request = base64url({"amount":"1000","currency":"0x00"}) — no methodDetails/chainId
    let challenge = r#"Payment id="x", realm="x", method="tempo", intent="charge", request="eyJhbW91bnQiOiIxMDAwIiwiY3VycmVuY3kiOiIweDAwIn0""#;

    let output = test_command(&temp)
        .args(["mpp-sign", "--dry-run", "--challenge", challenge])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("chainId"),
        "should mention chainId: {stderr}"
    );
}

#[test]
fn sign_no_wallet_configured() {
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["mpp-sign", "--challenge", VALID_CHARGE_CHALLENGE])
        .output()
        .unwrap();

    assert!(!output.status.success());
}

#[test]
fn sign_empty_stdin_fails() {
    use std::process::Stdio;

    let temp = TestConfigBuilder::new().build();
    let mut child = test_command(&temp)
        .arg("mpp-sign")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn");
    drop(child.stdin.take()); // close stdin immediately
    let output = child.wait_with_output().expect("Failed to wait");
    assert!(!output.status.success());
}

#[test]
fn sign_dry_run_reads_from_stdin() {
    use std::io::Write;
    use std::process::Stdio;

    let temp = TestConfigBuilder::new().build();
    let mut child = test_command(&temp)
        .args(["mpp-sign", "--dry-run"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(VALID_CHARGE_CHALLENGE.as_bytes())
        .unwrap();
    drop(child.stdin.take());
    let output = child.wait_with_output().expect("Failed to wait");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "sign via stdin failed: {stderr}");
    assert!(
        stderr.contains("Challenge is valid"),
        "should confirm valid: {stderr}"
    );
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

// ==================== transfer ====================

#[test]
fn transfer_help_shows_flags() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["transfer", "--help"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let combined = get_combined_output(&output);
    assert!(combined.contains("--to"), "should show --to flag");
    assert!(combined.contains("--dry-run"), "should show --dry-run flag");
    assert!(
        combined.contains("--fee-token"),
        "should show --fee-token flag"
    );
}

#[test]
fn transfer_no_wallet_fails() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args([
            "transfer",
            "1.00",
            "usdc",
            "--to",
            "0x70997970C51812dc3A010C7d01b50e0d17dc79C8",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("No wallet") || combined.contains("login"),
        "should mention no wallet or login: {combined}"
    );
}

#[test]
fn transfer_missing_to_flag_fails() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["transfer", "1.00", "usdc"])
        .output()
        .unwrap();

    assert_exit_code(&output, 2, "missing --to should exit with E_USAGE");
}

// ==================== unknown subcommand ====================

#[test]
fn unknown_subcommand_fails() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp).arg("nonexistent").output().unwrap();

    assert_exit_code(&output, 2, "unknown subcommand should exit with E_USAGE");
}
