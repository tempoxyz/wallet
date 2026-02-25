//! Integration tests for CLI features:
//! - Shell completions (bash, zsh, fish, powershell)
//! - Command aliases (short forms for common commands)
//! - Display options (verbosity, quiet mode, color control)
//! - Help output organization (sections, annotations)

use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::process::Command;

mod common;
use common::{get_combined_output, test_command, TestConfigBuilder};

#[test]
fn test_completions_bash() {
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .args(["completions", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("_presto"))
        .stdout(predicate::str::contains("complete"));
}

#[test]
fn test_completions_zsh() {
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .args(["completions", "zsh"])
        .assert()
        .success()
        .stdout(predicate::str::contains("#compdef presto"));
}

#[test]
fn test_completions_fish() {
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .args(["completions", "fish"])
        .assert()
        .success()
        .stdout(predicate::str::contains("complete -c presto"));
}

#[test]
fn test_completions_powershell() {
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .args(["completions", "powershell"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Register-ArgumentCompleter"));
}

#[test]
fn test_quiet_flag() {
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .args(["completions", "-q"])
        .assert()
        .success();
}

#[test]
fn test_verbosity_single() {
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .args(["completions", "-v"])
        .assert()
        .success();
}

#[test]
fn test_verbosity_multiple() {
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .args(["completions", "-vv"])
        .assert()
        .success();
}

#[test]
fn test_verbosity_long_form() {
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .args(["completions", "--verbose"])
        .assert()
        .success();
}

#[test]
fn test_color_auto() {
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .args(["completions", "--color", "auto"])
        .assert()
        .success();
}

#[test]
fn test_color_always() {
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .args(["completions", "--color", "always"])
        .assert()
        .success();
}

#[test]
fn test_color_never() {
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .args(["completions", "--color", "never"])
        .assert()
        .success();
}

#[test]
fn test_help_has_display_options_section() {
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Display Options:"));
}

#[test]
fn test_help_has_http_options_section() {
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .args(["query", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("HTTP Options:"));
}

#[test]
fn test_alias_with_display_options() {
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .args(["completions", "-q", "--color", "never"])
        .assert()
        .success();
}

#[test]
fn test_version_flag() {
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .arg("--version")
        .assert()
        .success();
}

#[test]
fn test_login_help() {
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .args(["login", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("log in"));
}

#[test]
fn test_logout_help() {
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .args(["logout", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Log out"))
        .stdout(predicate::str::contains("--yes"));
}

#[test]
fn test_multiple_global_flags_together() {
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .args(["completions", "-v", "--color", "never"])
        .assert()
        .success();
}

#[test]
fn test_verbosity_levels() {
    // Single -v
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .args(["completions", "-v"])
        .assert()
        .success();

    // Double -vv
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .args(["completions", "-vv"])
        .assert()
        .success();

    // Triple -vvv
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .args(["completions", "-vvv"])
        .assert()
        .success();
}

#[test]
fn test_all_color_modes() {
    for color_mode in ["auto", "always", "never"] {
        Command::new(assert_cmd::cargo::cargo_bin!("presto"))
            .args(["completions", "--color", color_mode])
            .assert()
            .success();
    }
}

#[test]
fn test_completions_no_arg_shows_shells() {
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .arg("completions")
        .assert()
        .success()
        .stdout(predicate::str::contains("bash"))
        .stdout(predicate::str::contains("zsh"))
        .stdout(predicate::str::contains("fish"));
}

#[test]
fn test_completions_invalid_shell() {
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .args(["completions", "invalid-shell"])
        .assert()
        .failure();
}

#[test]
fn test_completions_case_sensitivity() {
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .args(["completions", "BASH"])
        .assert()
        .failure();
}

#[test]
fn test_session_list_json_empty() {
    let temp = TestConfigBuilder::new().build();
    let mut cmd = test_command(&temp);
    cmd.args(["session", "list", "--output-format", "json"]);
    let output = cmd.output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["total"], 0);
    assert!(parsed["sessions"].as_array().unwrap().is_empty());
}

#[test]
fn test_key_list_json_has_total() {
    let temp_dir = TestConfigBuilder::new().build();

    let output = test_command(&temp_dir)
        .args(["key", "list", "--output-format", "json"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"keys\""),
        "expected JSON keys array: {stdout}"
    );
    assert!(
        stdout.contains("\"total\""),
        "expected JSON total field: {stdout}"
    );
}

#[test]
fn test_main_help_lists_all_commands() {
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("URL"))
        .stdout(predicate::str::contains("login"))
        .stdout(predicate::str::contains("logout"))
        .stdout(predicate::str::contains("balance"))
        .stdout(predicate::str::contains("whoami"));
}

#[test]
fn test_query_alias() {
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .args(["q", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("HTTP request"));
}

#[test]
fn test_query_help() {
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .args(["query", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("HTTP request"))
        .stdout(predicate::str::contains("URL"))
        .stdout(predicate::str::contains("Payment Options:"))
        .stdout(predicate::str::contains("HTTP Options:"));
}

#[test]
fn test_whoami_help() {
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .args(["whoami", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("wallet"));
}

#[test]
fn test_help_flag() {
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .arg("-h")
        .assert()
        .success();
}

#[test]
fn test_invalid_command() {
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .args(["session", "invalid-subcommand"])
        .assert()
        .failure();
}

#[test]
fn test_invalid_flag() {
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .arg("--invalid-flag")
        .assert()
        .failure();
}

// ==================== Implicit Query (bare URL) ====================

#[test]
fn test_bare_url_acts_as_query() {
    // `presto http://example.com` should work like `presto query http://example.com`
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .arg("http://example.com")
        .assert()
        .success();
}

#[test]
fn test_bare_url_with_verbose() {
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .args(["-v", "http://example.com"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Making GET request"));
}

#[test]
fn test_bare_url_with_include_headers() {
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .args(["--output-format", "text", "-i", "http://example.com"])
        .assert()
        .success()
        .stdout(predicate::str::contains("HTTP 200"));
}

#[test]
fn test_bare_url_with_method() {
    // `-X HEAD` with a bare URL
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .args(["-X", "HEAD", "http://example.com"])
        .assert()
        .success();
}

#[test]
fn test_explicit_query_still_works() {
    // Explicit `query` subcommand should still work
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .args(["query", "http://example.com"])
        .assert()
        .success();
}

#[test]
fn test_typo_subcommand_not_swallowed() {
    // A typo'd subcommand should fail with a clap error, not be treated as query
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .args(["qurey", "http://example.com"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("error"));
}

#[test]
fn test_bare_url_with_dry_run() {
    // `--dry-run` with a bare URL should succeed without making a payment
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .args(["--dry-run", "http://example.com"])
        .assert()
        .success();
}

#[test]
fn test_no_args_shows_help() {
    // Running with no arguments should show help and succeed
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage"));
}

// ==================== Network Validation ====================

#[test]
fn test_invalid_network_flag_rejected() {
    let temp = TestConfigBuilder::new().build();
    let mut cmd = test_command(&temp);
    cmd.args(["-n", "not-a-network", "balance"]);
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("Unknown network"));
}

#[test]
fn test_invalid_network_whoami_rejected() {
    let temp = TestConfigBuilder::new().build();
    let mut cmd = test_command(&temp);
    cmd.args(["-n", "foobar", "whoami"]);
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("Unknown network"));
}

#[test]
fn test_invalid_network_login_rejected() {
    let temp = TestConfigBuilder::new().build();
    let mut cmd = test_command(&temp);
    cmd.args(["-n", "bad-net", "login"]);
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("Unknown network"));
}

// ==================== Error Paths ====================

#[test]
fn test_logout_without_wallet() {
    let temp = TestConfigBuilder::new().build();
    let mut cmd = test_command(&temp);
    cmd.args(["logout", "--yes"]);
    let output = cmd.output().expect("Failed to run command");
    let combined = get_combined_output(&output).to_lowercase();
    assert!(
        combined.contains("not logged in"),
        "Expected 'not logged in' message, got: {combined}"
    );
}

#[test]
fn test_logout_noninteractive_without_yes() {
    let temp = TestConfigBuilder::new().build();
    let mut cmd = test_command(&temp);
    cmd.arg("logout");
    // No wallet → prints "Not logged in." and succeeds regardless
    let output = cmd.output().expect("Failed to run command");
    let combined = get_combined_output(&output).to_lowercase();
    assert!(
        combined.contains("not logged in"),
        "Expected 'not logged in' message, got: {combined}"
    );
}

#[test]
fn test_session_close_all_no_sessions() {
    let temp = TestConfigBuilder::new().build();
    let mut cmd = test_command(&temp);
    cmd.args(["session", "close", "--all"]);
    cmd.assert().success();
}

#[test]
fn test_session_help() {
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .args(["session", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("close"));
}

#[test]
fn test_session_list_alias() {
    let temp = TestConfigBuilder::new().build();
    let mut cmd = test_command(&temp);
    cmd.arg("session");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("No active sessions"));
}

#[test]
fn test_private_key_env_value_hidden_in_help() {
    // --private-key is a hidden global flag — it shouldn't appear in help,
    // and the env var value should never be leaked.
    let output = Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .args(["--help"])
        .env("PRESTO_PRIVATE_KEY", "0xsecretkey")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("0xsecretkey"),
        "help should not show the private key value: {stdout}"
    );
    assert!(
        !stdout.contains("--private-key"),
        "hidden flag should not appear in help: {stdout}"
    );
}

// ==================== Key Management Tests ====================

/// Helper: write a multi-key keys.toml into both macOS and Linux paths.
fn setup_multi_key(temp: &tempfile::TempDir) {
    let wallet_toml = r#"active = "default"

[keys.default]
wallet_address = "0xAAA"
access_key_address = "0xAAA"
access_key = "0xkey1"

[keys.work]
wallet_address = "0xBBB"
access_key_address = "0xBBB"
access_key = "0xkey2"
"#;
    let config_toml = "";

    let macos_dir = temp.path().join("Library/Application Support/presto");
    std::fs::create_dir_all(&macos_dir).unwrap();
    std::fs::write(macos_dir.join("keys.toml"), wallet_toml).unwrap();
    std::fs::write(macos_dir.join("config.toml"), config_toml).unwrap();

    let linux_data = temp.path().join(".local/share/presto");
    let linux_config = temp.path().join(".config/presto");
    std::fs::create_dir_all(&linux_data).unwrap();
    std::fs::create_dir_all(&linux_config).unwrap();
    std::fs::write(linux_data.join("keys.toml"), wallet_toml).unwrap();
    std::fs::write(linux_config.join("config.toml"), config_toml).unwrap();
}

#[test]
fn test_wallet_delete_with_yes() {
    let temp = tempfile::TempDir::new().unwrap();
    setup_multi_key(&temp);

    let output = test_command(&temp)
        .args(["wallet", "delete", "work", "--yes"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Deleted"),
        "should confirm delete: {stdout}"
    );
}

#[test]
fn test_wallet_delete_nonexistent() {
    let temp = tempfile::TempDir::new().unwrap();
    setup_multi_key(&temp);

    let output = test_command(&temp)
        .args(["wallet", "delete", "nonexistent", "--yes"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let combined = get_combined_output(&output);
    assert!(combined.contains("not found"), "should error: {combined}");
}

#[test]
fn test_wallet_delete_without_yes_noninteractive() {
    let temp = tempfile::TempDir::new().unwrap();
    setup_multi_key(&temp);

    let output = test_command(&temp)
        .args(["wallet", "delete", "work"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("Use --yes"),
        "should require --yes in non-interactive mode: {combined}"
    );
}

#[test]
fn test_wallet_delete_active_switches() {
    let temp = tempfile::TempDir::new().unwrap();
    setup_multi_key(&temp);

    // Delete the active key "default"
    let output = test_command(&temp)
        .args(["wallet", "delete", "default", "--yes"])
        .output()
        .unwrap();

    assert!(output.status.success());
}

#[test]
fn test_key_global_flag_selects_key() {
    let temp = tempfile::TempDir::new().unwrap();
    setup_multi_key(&temp);

    let output = test_command(&temp)
        .args(["--key", "work", "whoami"])
        .output()
        .unwrap();

    let combined = get_combined_output(&output);
    assert!(
        combined.contains("0xBBB"),
        "should use work key's address 0xBBB: {combined}"
    );
}

// ==================== Session JSON Output Tests ====================

#[test]
fn test_session_list_text_empty() {
    let temp = TestConfigBuilder::new().build();
    let mut cmd = test_command(&temp);
    cmd.args(["session", "list"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("No active sessions"));
}

#[test]
fn test_session_close_all_json_empty() {
    let temp = TestConfigBuilder::new().build();
    let mut cmd = test_command(&temp);
    cmd.args(["session", "close", "--all", "--output-format", "json"]);
    let output = cmd.output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["closed"], 0);
    assert_eq!(parsed["pending"], 0);
    assert_eq!(parsed["failed"], 0);
    assert!(parsed["results"].as_array().unwrap().is_empty());
}

#[test]
fn test_session_close_no_args_fails() {
    let temp = TestConfigBuilder::new().build();
    let mut cmd = test_command(&temp);
    cmd.args(["session", "close"]);
    cmd.assert().failure();
}

#[test]
fn test_session_close_invalid_channel_id() {
    let temp = TestConfigBuilder::new().build();
    let mut cmd = test_command(&temp);
    cmd.args(["session", "close", "0xinvalid"]);
    let output = cmd.output().unwrap();
    // Short hex string (not 66 chars) is treated as a URL, not a channel ID
    // Should print "No active session" not a crash
    assert!(
        !output.status.success() || {
            let combined = get_combined_output(&output).to_lowercase();
            combined.contains("no active session") || combined.contains("error")
        }
    );
}

#[test]
fn test_session_list_closed_json_empty() {
    let temp = TestConfigBuilder::new().build();
    let mut cmd = test_command(&temp);
    cmd.args(["session", "list", "--closed", "--output-format", "json"]);
    let output = cmd.output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["total"], 0);
    assert!(parsed["sessions"].as_array().unwrap().is_empty());
}
