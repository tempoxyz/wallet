//! Integration tests for balance command
//!
//! Tests for the balance command that checks wallet balances across networks.

use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::process::Command;

mod common;
use common::setup_test_config;

// ============================================================================
// Fast tests (no network calls, pure CLI parsing)
// ============================================================================

#[test]
fn test_balance_no_config() {
    let temp = setup_test_config();

    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .env("HOME", temp.path())
        .env("XDG_CONFIG_HOME", temp.path().join(".config"))
        .env("XDG_DATA_HOME", temp.path().join(".local/share"))
        .env("XDG_CACHE_HOME", temp.path().join(".cache"))
        .arg("balance")
        .assert()
        .stderr(predicate::str::contains(
            "No wallet connected. Starting login",
        ));
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
