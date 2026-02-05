//! Integration tests for the config command

use assert_cmd::prelude::*;
use predicates::prelude::*;

mod common;
use common::{setup_test_config, test_command};

#[test]
fn test_config_empty() {
    let temp = setup_test_config();

    test_command(&temp)
        .arg("config")
        .assert()
        .success()
        .stdout(predicate::str::contains("No payment methods configured"));
}

#[test]
fn test_config_get_nonexistent_key() {
    let temp = setup_test_config();

    test_command(&temp)
        .args(["config", "get", "nonexistent.key"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn test_config_get_missing_section() {
    let temp = setup_test_config();

    test_command(&temp)
        .args(["config", "get", "missing.key"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn test_config_validate_empty_config() {
    let temp = setup_test_config();

    test_command(&temp)
        .args(["config", "validate"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("No payment methods configured"));
}
