use assert_cmd::prelude::*;
use predicates::prelude::*;
use serial_test::serial;
use std::fs;
use std::process::Command;

mod common;
use common::{
    get_test_keystores_dir, setup_test_config, test_command, TestConfigBuilder,
    TEST_EVM_KEY as VALID_EVM_KEY,
};

#[test]
fn test_method_list_no_keystores() {
    let temp = setup_test_config(Some(VALID_EVM_KEY), None);

    test_command(&temp)
        .args(["method", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No keystores found"))
        .stdout(predicate::str::contains("purl method new"));
}

#[test]
fn test_method_list_with_keystores() {
    let temp = TestConfigBuilder::new()
        .with_evm_keystore("test-wallet", VALID_EVM_KEY)
        .build();

    test_command(&temp)
        .args(["method", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Available keystores:"))
        .stdout(predicate::str::contains("test-wallet.json"));
}

#[test]
fn test_method_list_multiple_keystores() {
    let temp = TestConfigBuilder::new()
        .with_evm_keystore("wallet-one", VALID_EVM_KEY)
        .build();

    let keystores_dir = get_test_keystores_dir(&temp);
    fs::create_dir_all(&keystores_dir).unwrap();
    fs::write(
        keystores_dir.join("wallet-two.json"),
        r#"{"address":"0x1234567890123456789012345678901234567890","crypto":{}}"#,
    )
    .unwrap();

    test_command(&temp)
        .args(["method", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("wallet-one.json"))
        .stdout(predicate::str::contains("wallet-two.json"));
}

#[test]
fn test_method_list_alias() {
    let temp = setup_test_config(Some(VALID_EVM_KEY), None);

    test_command(&temp).args(["m", "list"]).assert().success();
}

#[test]
fn test_method_show_nonexistent_keystore() {
    let temp = setup_test_config(Some(VALID_EVM_KEY), None);

    test_command(&temp)
        .args(["method", "show", "nonexistent"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
#[serial]
fn test_method_show_existing_keystore() {
    let temp = tempfile::TempDir::new().unwrap();

    let _keystore_path =
        common::create_test_keystore(&temp, "test-wallet", VALID_EVM_KEY, "test-password");

    test_command(&temp)
        .args(["method", "show", "test-wallet"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Keystore Details:"))
        .stdout(predicate::str::contains("Name: test-wallet"))
        .stdout(predicate::str::contains("Address:"));
}

#[test]
#[serial]
fn test_method_show_displays_path() {
    let temp = tempfile::TempDir::new().unwrap();

    let _keystore_path =
        common::create_test_keystore(&temp, "test-wallet", VALID_EVM_KEY, "test-password");

    test_command(&temp)
        .args(["method", "show", "test-wallet"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Path:"))
        .stdout(predicate::str::contains("purl"))
        .stdout(predicate::str::contains("keystores"))
        .stdout(predicate::str::contains("test-wallet.json"));
}

#[test]
#[serial]
fn test_method_show_displays_encryption_info() {
    let temp = tempfile::TempDir::new().unwrap();

    let _keystore_path =
        common::create_test_keystore(&temp, "test-wallet", VALID_EVM_KEY, "test-password");

    test_command(&temp)
        .args(["method", "show", "test-wallet"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Encryption:"));
}

#[test]
#[serial]
fn test_method_show_name_without_json_extension() {
    let temp = tempfile::TempDir::new().unwrap();

    let _keystore_path =
        common::create_test_keystore(&temp, "test-wallet", VALID_EVM_KEY, "test-password");

    // Should work with or without .json extension
    test_command(&temp)
        .args(["method", "show", "test-wallet"])
        .assert()
        .success();
}

#[test]
fn test_method_verify_nonexistent_keystore() {
    let temp = setup_test_config(Some(VALID_EVM_KEY), None);

    test_command(&temp)
        .args(["method", "verify", "nonexistent"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn test_method_alias_list() {
    let temp = setup_test_config(Some(VALID_EVM_KEY), None);

    test_command(&temp).args(["m", "list"]).assert().success();
}

#[test]
#[serial]
fn test_method_alias_show() {
    let temp = tempfile::TempDir::new().unwrap();

    let _keystore_path =
        common::create_test_keystore(&temp, "test-wallet", VALID_EVM_KEY, "test-password");

    test_command(&temp)
        .args(["m", "show", "test-wallet"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Keystore Details:"));
}

#[test]
fn test_method_help() {
    Command::new(assert_cmd::cargo::cargo_bin!("purl"))
        .args(["method", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Manage payment methods"))
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("new"))
        .stdout(predicate::str::contains("import"))
        .stdout(predicate::str::contains("show"))
        .stdout(predicate::str::contains("verify"));
}

#[test]
fn test_method_list_help() {
    Command::new(assert_cmd::cargo::cargo_bin!("purl"))
        .args(["method", "list", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("List available keystores"));
}

#[test]
fn test_method_new_help() {
    Command::new(assert_cmd::cargo::cargo_bin!("purl"))
        .args(["method", "new", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Create a new keystore"))
        .stdout(predicate::str::contains("--name"))
        .stdout(predicate::str::contains("--generate"));
}

#[test]
fn test_method_import_help() {
    Command::new(assert_cmd::cargo::cargo_bin!("purl"))
        .args(["method", "import", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Import a private key"))
        .stdout(predicate::str::contains("--name"))
        .stdout(predicate::str::contains("--private-key"));
}

#[test]
fn test_method_show_help() {
    Command::new(assert_cmd::cargo::cargo_bin!("purl"))
        .args(["method", "show", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Show keystore details"));
}

#[test]
fn test_method_verify_help() {
    Command::new(assert_cmd::cargo::cargo_bin!("purl"))
        .args(["method", "verify", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Verify keystore integrity"));
}

#[test]
fn test_method_invalid_subcommand() {
    Command::new(assert_cmd::cargo::cargo_bin!("purl"))
        .args(["method", "invalid"])
        .assert()
        .failure();
}

#[test]
fn test_method_show_corrupted_keystore() {
    let temp = tempfile::TempDir::new().unwrap();
    let keystores_dir = get_test_keystores_dir(&temp);
    fs::create_dir_all(&keystores_dir).unwrap();

    fs::write(
        keystores_dir.join("corrupted.json"),
        "this is not valid json",
    )
    .unwrap();

    test_command(&temp)
        .args(["method", "show", "corrupted"])
        .assert()
        .failure();
}

#[test]
fn test_method_list_displays_address_if_available() {
    let temp = tempfile::TempDir::new().unwrap();
    let keystores_dir = get_test_keystores_dir(&temp);
    fs::create_dir_all(&keystores_dir).unwrap();

    fs::write(
        keystores_dir.join("with-address.json"),
        r#"{"address":"1234567890abcdef1234567890abcdef12345678","crypto":{}}"#,
    )
    .unwrap();

    test_command(&temp)
        .args(["method", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "0x1234567890abcdef1234567890abcdef12345678",
        ));
}

#[test]
fn test_method_list_handles_keystore_without_address() {
    let temp = tempfile::TempDir::new().unwrap();
    let keystores_dir = get_test_keystores_dir(&temp);
    fs::create_dir_all(&keystores_dir).unwrap();

    fs::write(keystores_dir.join("no-address.json"), r#"{"crypto":{}}"#).unwrap();

    test_command(&temp)
        .args(["method", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("no-address.json"))
        .stdout(predicate::str::contains("no address field"));
}

#[test]
fn test_method_show_with_configured_keystore() {
    let temp = TestConfigBuilder::new()
        .with_evm_keystore("configured-wallet", VALID_EVM_KEY)
        .build();

    test_command(&temp)
        .args(["method", "show", "configured-wallet"])
        .assert()
        .success()
        .stdout(predicate::str::contains("configured-wallet"));
}

#[test]
fn test_method_list_after_config_init() {
    let temp = TestConfigBuilder::new()
        .with_evm_keystore("init-wallet", VALID_EVM_KEY)
        .with_solana_keystore("solana-wallet", common::TEST_SOLANA_KEY)
        .build();

    test_command(&temp)
        .args(["method", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Available keystores:"))
        .stdout(predicate::str::contains("init-wallet.json"))
        .stdout(predicate::str::contains("solana-wallet.json"));
}

#[test]
fn test_method_list_with_quiet_flag() {
    let temp = setup_test_config(Some(VALID_EVM_KEY), None);

    test_command(&temp)
        .args(["method", "list", "-q"])
        .assert()
        .success();
}

#[test]
fn test_method_list_with_verbosity() {
    let temp = setup_test_config(Some(VALID_EVM_KEY), None);

    test_command(&temp)
        .args(["method", "list", "-v"])
        .assert()
        .success();
}

#[test]
#[serial]
fn test_method_show_with_color_options() {
    let temp = tempfile::TempDir::new().unwrap();

    let _keystore_path =
        common::create_test_keystore(&temp, "test-wallet", VALID_EVM_KEY, "test-password");

    test_command(&temp)
        .args(["method", "show", "test-wallet", "--color", "never"])
        .assert()
        .success();
}
