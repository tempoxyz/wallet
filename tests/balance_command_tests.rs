//! Integration tests for balance command
//!
//! Tests for the balance command that checks wallet balances across networks.
//!
//! These tests use mock network mode (PRESTO_MOCK_NETWORK=1) to avoid real RPC calls,
//! making them fast and reliable without network access.

use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::process::Command;

mod common;
use common::{mock_test_command, setup_test_config};

// ============================================================================
// Fast tests (no network calls, pure CLI parsing)
// ============================================================================

#[test]
fn test_balance_no_config() {
    let temp = setup_test_config();

    mock_test_command(&temp)
        .arg("balance")
        .assert()
        .failure()
        .stderr(predicate::str::contains("No wallet connected"));
}

#[test]
fn test_balance_help() {
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .args(["balance", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Check wallet balance"))
        .stdout(predicate::str::contains("address"))
        .stdout(predicate::str::contains("--network"));
}

#[test]
fn test_balance_help_shows_network_flag() {
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .args(["balance", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("-n, --network"));
}

#[test]
fn test_balance_help_via_alias() {
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .args(["b", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Check wallet balance"));
}

// ============================================================================
// Mock network tests (use PRESTO_MOCK_NETWORK=1 for fast, reliable tests)
// ============================================================================

#[test]
fn test_balance_with_explicit_address() {
    let temp = setup_test_config();

    mock_test_command(&temp)
        .args(["balance", "0xd8da6bf26964af9d7eed9e03e53415d37aa96045"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Tempo Stablecoin Balances:"))
        .stdout(predicate::str::contains("mock"));
}

#[test]
fn test_balance_with_network_filter_and_address() {
    let temp = setup_test_config();

    mock_test_command(&temp)
        .args([
            "balance",
            "0xd8da6bf26964af9d7eed9e03e53415d37aa96045",
            "--network",
            "tempo",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("tempo:"))
        .stdout(predicate::str::contains("1.000000"));
}

#[test]
fn test_balance_with_network_filter_short_and_address() {
    let temp = setup_test_config();

    mock_test_command(&temp)
        .args([
            "balance",
            "0xd8da6bf26964af9d7eed9e03e53415d37aa96045",
            "-n",
            "tempo-moderato",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("tempo-moderato:"))
        .stdout(predicate::str::contains("5.000000"));
}

#[test]
fn test_balance_alias_with_address() {
    let temp = setup_test_config();

    mock_test_command(&temp)
        .args(["b", "0xd8da6bf26964af9d7eed9e03e53415d37aa96045"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Tempo Stablecoin Balances:"));
}

#[test]
fn test_balance_with_quiet_flag() {
    let temp = setup_test_config();

    mock_test_command(&temp)
        .args([
            "balance",
            "0xd8da6bf26964af9d7eed9e03e53415d37aa96045",
            "-q",
        ])
        .assert()
        .success();
}

#[test]
fn test_balance_with_verbosity() {
    let temp = setup_test_config();

    mock_test_command(&temp)
        .args([
            "balance",
            "0xd8da6bf26964af9d7eed9e03e53415d37aa96045",
            "-v",
        ])
        .assert()
        .success();
}

#[test]
fn test_balance_with_color_never() {
    let temp = setup_test_config();

    mock_test_command(&temp)
        .args([
            "balance",
            "0xd8da6bf26964af9d7eed9e03e53415d37aa96045",
            "--color",
            "never",
        ])
        .assert()
        .success();
}

#[test]
fn test_balance_invalid_network() {
    let temp = setup_test_config();

    mock_test_command(&temp)
        .args([
            "balance",
            "0xd8da6bf26964af9d7eed9e03e53415d37aa96045",
            "--network",
            "invalid-network",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("No balances found"));
}

#[test]
fn test_balance_combined_with_global_flags() {
    let temp = setup_test_config();

    mock_test_command(&temp)
        .args([
            "balance",
            "0xd8da6bf26964af9d7eed9e03e53415d37aa96045",
            "-v",
            "-q",
            "--color",
            "never",
        ])
        .assert()
        .success();
}

#[test]
fn test_balance_output_format() {
    let temp = setup_test_config();

    mock_test_command(&temp)
        .args([
            "balance",
            "0xd8da6bf26964af9d7eed9e03e53415d37aa96045",
            "-n",
            "tempo",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("atomic units"))
        .stdout(predicate::str::contains("tempo:"));
}
