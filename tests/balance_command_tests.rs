//! Integration tests for balance command
//!
//! Tests for the balance command that checks wallet balances across networks.
//!
//! These tests use mock network mode (PGET_MOCK_NETWORK=1) to avoid real RPC calls,
//! making them fast and reliable without network access.

use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::process::Command;

mod common;
use common::{
    mock_test_command, setup_test_config, test_command, TestConfigBuilder,
    TEST_EVM_KEY as VALID_EVM_KEY,
};

// ============================================================================
// Fast tests (no network calls, pure CLI parsing)
// ============================================================================

#[test]
fn test_balance_no_config() {
    let temp = setup_test_config(None, None);

    mock_test_command(&temp)
        .arg("balance")
        .assert()
        .failure()
        .stderr(predicate::str::contains("No payment methods configured"));
}

#[test]
fn test_balance_help() {
    Command::new(assert_cmd::cargo::cargo_bin!("pget"))
        .args(["balance", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Check wallet balance"))
        .stdout(predicate::str::contains("address"))
        .stdout(predicate::str::contains("--network"));
}

#[test]
fn test_balance_help_shows_network_flag() {
    Command::new(assert_cmd::cargo::cargo_bin!("pget"))
        .args(["balance", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("-n, --network"));
}

#[test]
fn test_balance_help_via_alias() {
    Command::new(assert_cmd::cargo::cargo_bin!("pget"))
        .args(["b", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Check wallet balance"));
}

// ============================================================================
// Mock network tests (use PGET_MOCK_NETWORK=1 for fast, reliable tests)
// ============================================================================

#[test]
fn test_balance_with_evm_config() {
    let temp = setup_test_config(Some(VALID_EVM_KEY), None);

    mock_test_command(&temp)
        .arg("balance")
        .assert()
        .success()
        .stdout(predicate::str::contains("Tempo Stablecoin Balances:"))
        .stdout(predicate::str::contains("mock"));
}

#[test]
fn test_balance_with_network_filter() {
    let temp = setup_test_config(Some(VALID_EVM_KEY), None);

    mock_test_command(&temp)
        .args(["balance", "--network", "tempo"])
        .assert()
        .success()
        .stdout(predicate::str::contains("tempo:"))
        .stdout(predicate::str::contains("1.000000")); // Mock returns 1 αUSD for tempo
}

#[test]
fn test_balance_with_network_filter_short() {
    let temp = setup_test_config(Some(VALID_EVM_KEY), None);

    mock_test_command(&temp)
        .args(["balance", "-n", "tempo-moderato"])
        .assert()
        .success()
        .stdout(predicate::str::contains("tempo-moderato:"))
        .stdout(predicate::str::contains("5.000000")); // Mock returns 5 αUSD for testnets
}

#[test]
fn test_balance_alias() {
    let temp = setup_test_config(Some(VALID_EVM_KEY), None);

    mock_test_command(&temp)
        .arg("b")
        .assert()
        .success()
        .stdout(predicate::str::contains("Tempo Stablecoin Balances:"));
}

#[test]
fn test_balance_alias_with_network() {
    let temp = setup_test_config(Some(VALID_EVM_KEY), None);

    mock_test_command(&temp)
        .args(["b", "-n", "tempo"])
        .assert()
        .success()
        .stdout(predicate::str::contains("tempo:"));
}

#[test]
fn test_balance_with_keystore_config() {
    let temp = TestConfigBuilder::new()
        .with_evm_keystore("test-wallet")
        .build();

    mock_test_command(&temp)
        .arg("balance")
        .assert()
        .code(predicate::in_iter([0, 1])); // May fail if keystore can't be read
}

#[test]
fn test_balance_with_quiet_flag() {
    let temp = setup_test_config(Some(VALID_EVM_KEY), None);

    mock_test_command(&temp)
        .args(["balance", "-q"])
        .assert()
        .success();
}

#[test]
fn test_balance_with_verbosity() {
    let temp = setup_test_config(Some(VALID_EVM_KEY), None);

    mock_test_command(&temp)
        .args(["balance", "-v"])
        .assert()
        .success();
}

#[test]
fn test_balance_with_color_never() {
    let temp = setup_test_config(Some(VALID_EVM_KEY), None);

    mock_test_command(&temp)
        .args(["balance", "--color", "never"])
        .assert()
        .success();
}

#[test]
fn test_balance_invalid_network() {
    let temp = setup_test_config(Some(VALID_EVM_KEY), None);

    // Invalid network filter results in no networks matching
    mock_test_command(&temp)
        .args(["balance", "--network", "invalid-network"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No balances found"));
}

#[test]
fn test_balance_after_init() {
    let temp = setup_test_config(Some(VALID_EVM_KEY), None);

    // First verify config works
    test_command(&temp).arg("config").assert().success();

    // Then check balance with mock
    mock_test_command(&temp)
        .arg("balance")
        .assert()
        .success()
        .stdout(predicate::str::contains("Tempo Stablecoin Balances:"));
}

#[test]
fn test_balance_combined_with_global_flags() {
    let temp = setup_test_config(Some(VALID_EVM_KEY), None);

    mock_test_command(&temp)
        .args(["balance", "-v", "-q", "--color", "never"])
        .assert()
        .success();
}

#[test]
fn test_balance_output_format() {
    let temp = setup_test_config(Some(VALID_EVM_KEY), None);

    // Verify the output format includes expected fields
    mock_test_command(&temp)
        .args(["balance", "-n", "tempo"])
        .assert()
        .success()
        .stdout(predicate::str::contains("atomic units"))
        .stdout(predicate::str::contains("tempo:"));
}

#[test]
fn test_balance_multiple_networks_evm() {
    let temp = setup_test_config(Some(VALID_EVM_KEY), None);

    // Without network filter, should show multiple networks
    let output = mock_test_command(&temp).arg("balance").assert().success();

    // Should have output for Tempo networks
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(stdout.contains("tempo") || stdout.contains("tempo-moderato"));
}
