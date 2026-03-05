//! Integration tests for CLI features:
//! - Shell completions (bash, zsh, fish, powershell)
//! - Command aliases (short forms for common commands)
//! - Display options (verbosity, quiet mode, color control)
//! - Help output organization (sections, annotations)

use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::process::Command;

mod common;
use common::{get_combined_output, seed_local_session, test_command, TestConfigBuilder};

#[test]
fn test_completions_bash() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["completions", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("_tempo-wallet"))
        .stdout(predicate::str::contains("complete"));
}

#[test]
fn test_completions_zsh() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["completions", "zsh"])
        .assert()
        .success()
        .stdout(predicate::str::contains("#compdef tempo-wallet"));
}

#[test]
fn test_completions_fish() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["completions", "fish"])
        .assert()
        .success()
        .stdout(predicate::str::contains("complete -c tempo-wallet"));
}

#[test]
fn test_completions_powershell() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["completions", "powershell"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Register-ArgumentCompleter"));
}

#[test]
fn test_quiet_flag() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["completions", "-s"])
        .assert()
        .success();
}

#[test]
fn test_verbosity_single() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["completions", "-v"])
        .assert()
        .success();
}

#[test]
fn test_verbosity_multiple() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["completions", "-vv"])
        .assert()
        .success();
}

#[test]
fn test_verbosity_long_form() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["completions", "--verbose"])
        .assert()
        .success();
}

#[test]
fn test_color_auto() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["completions", "--color", "auto"])
        .assert()
        .success();
}

#[test]
fn test_color_always() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["completions", "--color", "always"])
        .assert()
        .success();
}

#[test]
fn test_color_never() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["completions", "--color", "never"])
        .assert()
        .success();
}

#[test]
fn test_help_has_display_options_section() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Display Options:"));
}

#[test]
fn test_help_has_http_options_section() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["query", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("HTTP Options:"));
}

#[test]
fn test_top_level_help_compact_no_hidden_commands() {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .arg("--help")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Hidden commands must not appear in top-level help
    assert!(
        !stdout.contains("query ") && !stdout.contains("  query"),
        "hidden 'query' command leaked: {stdout}"
    );
    assert!(
        !stdout.contains("balance ") && !stdout.contains("  balance"),
        "hidden 'balance' command leaked: {stdout}"
    );
    assert!(
        !stdout.contains("completions"),
        "hidden 'completions' command leaked: {stdout}"
    );
    assert!(
        !stdout.contains("wallets ") && !stdout.contains("  wallets"),
        "hidden 'wallets' command leaked: {stdout}"
    );
    // Visible commands must appear
    assert!(stdout.contains("login"), "missing 'login' command");
    assert!(stdout.contains("logout"), "missing 'logout' command");
    assert!(stdout.contains("whoami"), "missing 'whoami' command");
    assert!(stdout.contains("sessions"), "missing 'sessions' command");
    assert!(stdout.contains("services"), "missing 'services' command");
}

#[test]
fn test_query_help_has_key_flags() {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["query", "--help"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Key HTTP flags
    for flag in ["-X", "-H", "-d,", "--json", "-L", "-i,", "-I", "-m,", "-o,"] {
        assert!(stdout.contains(flag), "missing flag {flag} in query help");
    }
    // Examples section
    assert!(
        stdout.contains("Examples"),
        "missing Examples section in query help"
    );
    // Hidden flags must not appear
    assert!(
        !stdout.contains("--write-meta"),
        "hidden --write-meta leaked in query help"
    );
    assert!(
        !stdout.contains("--price-json"),
        "hidden --price-json leaked in query help"
    );
    assert!(
        !stdout.contains("--rpc"),
        "hidden --rpc leaked in query help"
    );
}

#[test]
fn test_alias_with_display_options() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["completions", "-s", "--color", "never"])
        .assert()
        .success();
}

#[test]
fn test_version_flag() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("tempo-wallet"))
        .stdout(predicate::str::is_match(r"[0-9a-f]{7}").unwrap());
}

#[test]
fn test_version_includes_build_info() {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should contain version, 7-char commit, date, and profile
    assert!(
        stdout.contains(env!("CARGO_PKG_VERSION")),
        "missing version in: {stdout}"
    );
    assert!(
        stdout.contains('(') && stdout.contains(')'),
        "missing build info parens in: {stdout}"
    );
}

#[test]
fn test_version_json() {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["-j", "--version"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(parsed.get("version").is_some(), "missing 'version' field");
    assert!(
        parsed.get("git_commit").is_some(),
        "missing 'git_commit' field"
    );
    assert!(
        parsed.get("build_date").is_some(),
        "missing 'build_date' field"
    );
    assert!(parsed.get("profile").is_some(), "missing 'profile' field");
    // git_commit should be a 7-char hex string (or "unknown" in unusual builds)
    let commit = parsed["git_commit"].as_str().unwrap();
    assert!(
        commit == "unknown" || (commit.len() == 7 && commit.chars().all(|c| c.is_ascii_hexdigit())),
        "unexpected git_commit format: {commit}"
    );
}

#[test]
fn test_login_help() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["login", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("log in"));
}

#[test]
fn test_logout_help() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["logout", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Log out"))
        .stdout(predicate::str::contains("--yes"));
}

#[test]
fn test_multiple_global_flags_together() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["completions", "-v", "--color", "never"])
        .assert()
        .success();
}

#[test]
fn test_verbosity_levels() {
    // Single -v
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["completions", "-v"])
        .assert()
        .success();

    // Double -vv
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["completions", "-vv"])
        .assert()
        .success();

    // Triple -vvv
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["completions", "-vvv"])
        .assert()
        .success();
}

#[test]
fn test_all_color_modes() {
    for color_mode in ["auto", "always", "never"] {
        Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
            .args(["completions", "--color", color_mode])
            .assert()
            .success();
    }
}

#[test]
fn test_completions_no_arg_shows_shells() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .arg("completions")
        .assert()
        .success()
        .stdout(predicate::str::contains("bash"))
        .stdout(predicate::str::contains("zsh"))
        .stdout(predicate::str::contains("fish"));
}

#[test]
fn test_completions_invalid_shell() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["completions", "invalid-shell"])
        .assert()
        .failure();
}

#[test]
fn test_completions_case_sensitivity() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["completions", "BASH"])
        .assert()
        .failure();
}

#[test]
fn test_session_list_json_empty() {
    let temp = TestConfigBuilder::new().build();
    let mut cmd = test_command(&temp);
    cmd.args(["-j", "sessions", "list"]);
    let output = cmd.output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["total"], 0);
    assert!(parsed["sessions"].as_array().unwrap().is_empty());
}

#[test]
fn test_session_list_json_via_alias_short_j() {
    let temp = TestConfigBuilder::new().build();
    let mut cmd = test_command(&temp);
    cmd.args(["-j", "sessions", "list"]);
    let output = cmd.output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["total"], 0);
}

#[test]
fn test_session_list_json_via_alias_long() {
    let temp = TestConfigBuilder::new().build();
    let mut cmd = test_command(&temp);
    cmd.args(["--json-output", "sessions", "list"]);
    let output = cmd.output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["total"], 0);
}

#[test]
fn test_key_list_json_has_total() {
    let temp_dir = TestConfigBuilder::new().build();

    let output = test_command(&temp_dir)
        .args(["-j", "keys", "list"])
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
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
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
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["q", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("HTTP request"));
}

#[test]
fn test_query_help() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
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
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["whoami", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("wallet"));
}

#[test]
fn test_help_flag() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .arg("-h")
        .assert()
        .success();
}

#[test]
fn test_invalid_command() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["sessions", "invalid-subcommand"])
        .assert()
        .failure();
}

#[test]
fn test_invalid_flag() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .arg("--invalid-flag")
        .assert()
        .failure();
}

// ==================== Implicit Query (bare URL) ====================

#[test]
fn test_bare_url_acts_as_query() {
    // `tempo-wallet http://example.com` should work like `tempo-wallet query http://example.com`
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .arg("http://example.com")
        .assert()
        .success();
}

#[test]
fn test_bare_url_with_verbose() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["-v", "http://example.com"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Making GET request"));
}

#[test]
fn test_bare_url_with_include_headers() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["-i", "http://example.com"])
        .assert()
        .success()
        .stdout(predicate::str::contains("HTTP 200"));
}

#[test]
fn test_bare_url_with_method() {
    // `-X HEAD` with a bare URL
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["-X", "HEAD", "http://example.com"])
        .assert()
        .success();
}

#[test]
fn test_i_alias_for_head() {
    // `-I` should act as HEAD
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["-I", "http://example.com"])
        .assert()
        .success();
}

#[test]
fn test_explicit_query_still_works() {
    // Explicit `query` subcommand should still work
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["query", "http://example.com"])
        .assert()
        .success();
}

#[test]
fn test_typo_subcommand_not_swallowed() {
    // A typo'd subcommand should fail with a clap error, not be treated as query
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["qurey", "http://example.com"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("error"));
}

#[test]
fn test_unknown_command_shows_clean_error() {
    // `tempo-wallet foo` should show a clean "not a command" error, not a URL parse error
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["foo"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("is not a tempo-wallet command"))
        .stderr(predicate::str::contains("tempo-wallet --help"));
}

#[test]
fn test_bare_url_with_dry_run() {
    // `--dry-run` with a bare URL should succeed without making a payment
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["--dry-run", "http://example.com"])
        .assert()
        .success();
}

#[test]
fn test_no_args_shows_help() {
    // Running with no arguments should show help and succeed
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage"));
}

// ==================== Network Validation ====================

#[test]
fn test_invalid_network_flag_rejected() {
    let temp = TestConfigBuilder::new().build();
    let mut cmd = test_command(&temp);
    cmd.args(["-n", "not-a-network", "whoami"]);
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
    cmd.args(["sessions", "close", "--all"]);
    cmd.assert().success();
}

#[test]
fn test_session_help() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["sessions", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("close"))
        .stdout(predicate::str::contains("info"))
        .stdout(predicate::str::contains("recover"));
}

#[test]
fn test_session_shows_help() {
    let temp = TestConfigBuilder::new().build();
    let mut cmd = test_command(&temp);
    cmd.arg("sessions");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Manage payment sessions"));
}

#[test]
fn test_private_key_env_value_hidden_in_help() {
    // --private-key is a hidden global flag — it shouldn't appear in help,
    // and the env var value should never be leaked.
    let output = Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["--help"])
        .env("TEMPO_PRIVATE_KEY", "0xsecretkey")
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

// ==================== JSON Error Rendering ====================

#[test]
fn test_json_error_output_when_output_format_json() {
    let temp = TestConfigBuilder::new().build();
    let mut cmd = test_command(&temp);
    // Unknown network triggers a config error at the top-level
    cmd.args(["-j", "-n", "not-a-network", "whoami"]);
    let output = cmd.output().expect("failed to run");
    assert!(!output.status.success());
    // Error should be JSON to stdout; stderr may contain logs
    let stdout = String::from_utf8_lossy(&output.stdout);
    let val: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid json error");
    assert!(val.get("code").is_some(), "missing code in json error");
    assert!(
        val.get("message").is_some(),
        "missing message in json error"
    );
}

#[test]
fn test_json_error_output_for_network_error() {
    let temp = TestConfigBuilder::new().build();
    let mut cmd = test_command(&temp);
    // Use an unroutable local port to trigger a connection error quickly
    cmd.args(["-j", "query", "http://127.0.0.1:9"]);
    let output = cmd.output().expect("failed to run");
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let val: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid json error");
    assert!(val.get("code").is_some());
    assert!(val.get("message").is_some());
}

// ==================== Input Validation Tests ====================

#[test]
fn test_rejects_file_url_with_json_error() {
    let temp = TestConfigBuilder::new().build();
    let mut cmd = test_command(&temp);
    cmd.args(["-j", "query", "file:///etc/hosts"]);
    let output = cmd.output().expect("failed to run");
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let val: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid json error");
    assert_eq!(val.get("code").and_then(|v| v.as_str()), Some("E_USAGE"));
}

#[test]
fn test_rejects_data_url_with_json_error() {
    let temp = TestConfigBuilder::new().build();
    let mut cmd = test_command(&temp);
    cmd.args(["-j", "query", "data:text/plain,hi"]);
    let output = cmd.output().expect("failed to run");
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let val: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid json error");
    assert_eq!(val.get("code").and_then(|v| v.as_str()), Some("E_USAGE"));
}

#[test]
fn test_rejects_header_with_crlf_injection() {
    let temp = TestConfigBuilder::new().build();
    let mut cmd = test_command(&temp);
    // Embed a CRLF in a single -H argument to simulate header injection
    let bad_header = "X-Test: good\r\nInjected: bad".to_string();
    cmd.args(["-j", "-H", &bad_header, "http://example.com"]);
    let output = cmd.output().expect("failed to run");
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let val: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid json error");
    assert_eq!(val.get("code").and_then(|v| v.as_str()), Some("E_USAGE"));
}

// ==================== Session JSON Output Tests ====================

#[test]
fn test_session_list_text_empty() {
    let temp = TestConfigBuilder::new().build();
    let mut cmd = test_command(&temp);
    cmd.args(["sessions", "list"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("No active sessions"));
}

#[test]
fn test_session_close_all_json_empty() {
    let temp = TestConfigBuilder::new().build();
    let mut cmd = test_command(&temp);
    cmd.args(["-j", "sessions", "close", "--all"]);
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
    cmd.args(["sessions", "close"]);
    cmd.assert().failure();
}

#[test]
fn test_session_close_invalid_channel_id() {
    let temp = TestConfigBuilder::new().build();
    let mut cmd = test_command(&temp);
    cmd.args(["sessions", "close", "0xinvalid"]);
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
    cmd.args(["-j", "sessions", "list", "--state", "closing,finalizable"]);
    let output = cmd.output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["total"], 0);
    assert!(parsed["sessions"].as_array().unwrap().is_empty());
}

// ==================== Golden-Output JSON Tests ====================

#[test]
fn test_whoami_json_structure_no_wallet() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp).args(["-j", "whoami"]).output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    // Verify required top-level fields exist
    assert!(parsed.get("ready").is_some(), "missing 'ready' field");
}

#[test]
fn test_whoami_json_structure_with_wallet() {
    let temp = TestConfigBuilder::new()
        .with_keys_toml(
            "[[keys]]\nwallet_type = \"local\"\nwallet_address = \"0xAAA\"\nkey_address = \"0xAAA\"\nkey = \"0xdeadbeef\"\n",
        )
        .build();

    let output = test_command(&temp).args(["-j", "whoami"]).output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();

    // Verify expected field structure
    assert!(parsed.get("ready").is_some(), "missing 'ready' field");
    assert!(parsed.get("wallet").is_some(), "missing 'wallet' field");
    assert!(
        parsed.get("wallet_type").is_some(),
        "missing 'wallet_type' field"
    );
    assert!(parsed.get("network").is_some(), "missing 'network' field");
}

#[test]
fn test_key_list_json_structure_with_keys() {
    let temp = TestConfigBuilder::new()
        .with_keys_toml(
            "[[keys]]\nwallet_type = \"local\"\nwallet_address = \"0xAAA\"\nkey_address = \"0xAAA\"\nkey = \"0xdeadbeef\"\n",
        )
        .build();

    let output = test_command(&temp)
        .args(["-j", "keys", "list"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();

    // Verify top-level structure
    assert!(parsed.get("keys").is_some(), "missing 'keys' field");
    assert!(parsed.get("total").is_some(), "missing 'total' field");
    assert!(parsed["total"].as_u64().unwrap() > 0, "total should be > 0");

    // Verify key entry structure
    let keys = parsed["keys"].as_array().unwrap();
    assert!(!keys.is_empty(), "keys array should not be empty");
    let key = &keys[0];
    assert!(key.get("label").is_some(), "key missing 'label' field");
    assert!(key.get("address").is_some(), "key missing 'address' field");
}

#[test]
fn test_session_list_json_structure() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["-j", "sessions", "list"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();

    // Verify required top-level fields
    assert!(parsed.get("sessions").is_some(), "missing 'sessions' field");
    assert!(parsed.get("total").is_some(), "missing 'total' field");
    assert_eq!(parsed["total"], 0);
    assert!(parsed["sessions"].as_array().unwrap().is_empty());
}

#[test]
fn test_session_close_all_json_structure() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["-j", "sessions", "close", "--all"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();

    // Verify required close-summary fields
    assert!(parsed.get("closed").is_some(), "missing 'closed' field");
    assert!(parsed.get("pending").is_some(), "missing 'pending' field");
    assert!(parsed.get("failed").is_some(), "missing 'failed' field");
    assert!(parsed.get("results").is_some(), "missing 'results' field");
}

// ==================== Config & Env Precedence Tests ====================

#[test]
fn test_config_detected_from_macos_path() {
    let temp = TestConfigBuilder::new()
        .with_config_toml("tempo_rpc = \"https://macos-custom-rpc.example.com\"\n")
        .with_keys_toml(
            "[[keys]]\nwallet_type = \"local\"\nwallet_address = \"0xAAA\"\nkey_address = \"0xAAA\"\nkey = \"0xdeadbeef\"\n",
        )
        .build();

    // On macOS HOME points to the temp dir; config should be found.
    // Whoami loads config — success means config was detected.
    let output = test_command(&temp).arg("whoami").output().unwrap();
    assert!(
        output.status.success(),
        "whoami should succeed with macOS-layout config: {}",
        get_combined_output(&output)
    );
}

#[test]
fn test_config_detected_from_linux_xdg_path() {
    let temp = TestConfigBuilder::new()
        .with_config_toml("moderato_rpc = \"https://linux-custom-rpc.example.com\"\n")
        .with_keys_toml(
            "[[keys]]\nwallet_type = \"local\"\nwallet_address = \"0xAAA\"\nkey_address = \"0xAAA\"\nkey = \"0xdeadbeef\"\n",
        )
        .build();

    // XDG_CONFIG_HOME is set by test_command; config should be found via Linux path.
    let output = test_command(&temp).arg("whoami").output().unwrap();
    assert!(
        output.status.success(),
        "whoami should succeed with XDG-layout config: {}",
        get_combined_output(&output)
    );
}

#[test]
fn test_explicit_config_path_overrides_default() {
    let temp = TestConfigBuilder::new()
        .with_keys_toml(
            "[[keys]]\nwallet_type = \"local\"\nwallet_address = \"0xAAA\"\nkey_address = \"0xAAA\"\nkey = \"0xdeadbeef\"\n",
        )
        .build();

    // Write a separate config file in a custom location
    let custom_config = temp.path().join("custom-config.toml");
    std::fs::write(
        &custom_config,
        "tempo_rpc = \"https://explicit-rpc.example.com\"\n",
    )
    .unwrap();

    // Use -c to point at the custom config
    let output = test_command(&temp)
        .args(["-c", custom_config.to_str().unwrap(), "whoami"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "whoami should succeed with explicit -c config: {}",
        get_combined_output(&output)
    );
}

#[test]
fn test_explicit_config_path_not_found_fails() {
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-c", "/nonexistent/path/config.toml", "whoami"])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "should fail when explicit config path not found"
    );
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("not found") || combined.contains("Config"),
        "should mention config issue: {combined}"
    );
}

#[test]
fn test_missing_default_config_falls_back_to_defaults() {
    // Empty temp dir with no config.toml written — should use defaults
    let temp = tempfile::TempDir::new().unwrap();

    // Session list works without any config (uses defaults)
    let output = test_command(&temp)
        .args(["sessions", "list"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "session list should succeed with no config: {}",
        get_combined_output(&output)
    );
}

#[test]
fn test_malformed_config_toml_fails_gracefully() {
    let temp = TestConfigBuilder::new()
        .with_config_toml("this is [[[invalid toml!!! {{{")
        .build();

    let output = test_command(&temp).arg("whoami").output().unwrap();

    // Should fail but not panic
    assert!(!output.status.success(), "should fail on malformed config");
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("parse") || combined.contains("config") || combined.contains("Config"),
        "should mention config parse error: {combined}"
    );
}

#[test]
fn test_config_with_unknown_fields_still_loads() {
    // Old or future config files may have fields we don't know about
    let temp = TestConfigBuilder::new()
        .with_config_toml(
            "tempo_rpc = \"https://rpc.example.com\"\nunknown_field = true\n\n[unknown_section]\nfoo = \"bar\"\n",
        )
        .with_keys_toml(
            "[[keys]]\nwallet_type = \"local\"\nwallet_address = \"0xAAA\"\nkey_address = \"0xAAA\"\nkey = \"0xdeadbeef\"\n",
        )
        .build();

    let output = test_command(&temp).arg("whoami").output().unwrap();
    assert!(
        output.status.success(),
        "should tolerate unknown config fields: {}",
        get_combined_output(&output)
    );
}

#[test]
fn test_empty_config_file_loads_ok() {
    let temp = TestConfigBuilder::new()
        .with_config_toml("")
        .with_keys_toml(
            "[[keys]]\nwallet_type = \"local\"\nwallet_address = \"0xAAA\"\nkey_address = \"0xAAA\"\nkey = \"0xdeadbeef\"\n",
        )
        .build();

    let output = test_command(&temp).arg("whoami").output().unwrap();
    assert!(
        output.status.success(),
        "empty config should load as defaults: {}",
        get_combined_output(&output)
    );
}

#[test]
fn test_rpc_env_var_override() {
    let temp = TestConfigBuilder::new()
        .with_config_toml("tempo_rpc = \"https://config-file-rpc.example.com\"\n")
        .with_keys_toml(
            "[[keys]]\nwallet_type = \"local\"\nwallet_address = \"0xAAA\"\nkey_address = \"0xAAA\"\nkey = \"0xdeadbeef\"\n",
        )
        .build();

    //  TEMPO_RPC_URLshould override config file settings.
    // Whoami loads config and resolves network — this verifies the env override
    // is applied without error (actual RPC is not called for whoami).
    let output = test_command(&temp)
        .env("TEMPO_RPC_URL", "https://env-override-rpc.example.com")
        .arg("whoami")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "whoami should succeed with  TEMPO_RPC_URLenv: {}",
        get_combined_output(&output)
    );
}

#[test]
fn test_typed_rpc_override_in_config() {
    // Config with typed override for moderato
    let temp = TestConfigBuilder::new()
        .with_config_toml("moderato_rpc = \"https://typed-moderato-rpc.example.com\"\n")
        .with_keys_toml(
            "[[keys]]\nwallet_type = \"local\"\nwallet_address = \"0xAAA\"\nkey_address = \"0xAAA\"\nkey = \"0xdeadbeef\"\n",
        )
        .build();

    let output = test_command(&temp)
        .args(["-n", "tempo-moderato", "whoami"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "whoami should succeed with typed moderato_rpc config: {}",
        get_combined_output(&output)
    );
}

#[test]
fn test_rpc_table_override_in_config() {
    // Config with [rpc] table override
    let config = "[rpc]\ntempo = \"https://table-rpc.example.com\"\n";
    let temp = TestConfigBuilder::new()
        .with_config_toml(config)
        .with_keys_toml(
            "[[keys]]\nwallet_type = \"local\"\nwallet_address = \"0xAAA\"\nkey_address = \"0xAAA\"\nkey = \"0xdeadbeef\"\n",
        )
        .build();

    let output = test_command(&temp).arg("whoami").output().unwrap();
    assert!(
        output.status.success(),
        "whoami should succeed with [rpc] table config: {}",
        get_combined_output(&output)
    );
}

#[test]
fn test_invalid_network_flag_fails() {
    let temp = TestConfigBuilder::new().build();

    let output = test_command(&temp)
        .args(["-n", "nonexistent-network", "whoami"])
        .output()
        .unwrap();

    assert!(!output.status.success(), "should fail with unknown network");
    let combined = get_combined_output(&output);
    assert!(
        combined.to_lowercase().contains("unknown network"),
        "should mention unknown network: {combined}"
    );
}

// ==================== Session List/Close Behavior Coverage ====================

#[test]
fn test_session_list_json_schema_fields() {
    // Verify the JSON list schema has exactly "sessions" and "total"
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["-j", "sessions", "list"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(parsed["sessions"].is_array(), "missing sessions array");
    assert!(parsed["total"].is_number(), "missing total number");
    let obj = parsed.as_object().unwrap();
    for key in obj.keys() {
        assert!(
            key == "sessions" || key == "total",
            "unexpected field in list JSON: {key}"
        );
    }
}

#[test]
fn test_session_list_with_network_filter_empty() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["sessions", "list", "--network", "tempo"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("No active sessions"),
        "network filter should still show empty message: {stdout}"
    );
}

#[test]
fn test_session_list_with_network_filter_json() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["-j", "sessions", "list", "--network", "tempo-moderato"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["total"], 0);
}

#[test]
fn test_session_list_closed_text_empty() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["sessions", "list", "--state", "closing,finalizable"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("No sessions pending finalization"),
        "should show closed-empty message: {stdout}"
    );
}

#[test]
fn test_session_close_all_text_empty() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["sessions", "close", "--all"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("No active sessions to close"),
        "close --all on empty should report no sessions: {stdout}"
    );
}

#[test]
fn test_session_close_all_json_schema() {
    // Verify close summary JSON schema: closed/pending/failed/results
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["-j", "sessions", "close", "--all"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let obj = parsed.as_object().unwrap();
    for key in obj.keys() {
        assert!(
            key == "closed" || key == "pending" || key == "failed" || key == "results",
            "unexpected field in close JSON: {key}"
        );
    }
    assert!(parsed["results"].is_array());
}

#[test]
fn test_session_close_closed_text_empty() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["sessions", "close", "--closed"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("No channels pending finalization"),
        "close --closed on empty should report none: {stdout}"
    );
}

#[test]
fn test_session_close_closed_json_schema() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["-j", "sessions", "close", "--closed"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["closed"], 0);
    assert_eq!(parsed["pending"], 0);
    assert_eq!(parsed["failed"], 0);
    assert!(parsed["results"].as_array().unwrap().is_empty());
}

#[test]
fn test_session_close_no_target_error_message() {
    // `session close` without URL/--all/--orphaned/--closed should fail with guidance
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["sessions", "close"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let combined = get_combined_output(&output);
    assert!(
        combined.contains("Specify") || combined.contains("URL") || combined.contains("--all"),
        "should prompt user to specify a target: {combined}"
    );
}

#[test]
fn test_session_close_nonexistent_url_text() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["sessions", "close", "https://nonexistent.example.com"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("No active session"),
        "should report no session: {stdout}"
    );
}

#[test]
fn test_session_close_nonexistent_url_json() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["-j", "sessions", "close", "https://nonexistent.example.com"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["closed"], 0);
    assert_eq!(parsed["failed"], 1);
    let results = parsed["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["status"], "error");
}

#[test]
fn test_sessions_info_no_local_text() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["sessions", "info", "https://nonexistent.example.com"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let combined = get_combined_output(&output).to_lowercase();
    assert!(
        combined.contains("no local session"),
        "expected 'no local session' message, got: {combined}"
    );
}

#[test]
fn test_sessions_info_no_local_json() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["-j", "sessions", "info", "https://nonexistent.example.com"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let val: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(val["total"], 0);
    assert!(val["sessions"].as_array().unwrap().is_empty());
    assert_eq!(val["message"], "no local session for origin");
}

#[test]
fn test_sessions_info_single_does_not_print_count() {
    let temp = TestConfigBuilder::new().build();
    // Seed a local session for https://example.com
    seed_local_session(&temp, "https://example.com");

    let output = test_command(&temp)
        .args(["sessions", "info", "https://example.com"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should contain detailed fields
    assert!(stdout.contains("https://example.com"));
    assert!(stdout.contains("Network"));
    assert!(stdout.contains("Channel"));
    // Should NOT contain a trailing count summary like "1 session(s)."
    assert!(
        !stdout.contains("session(s)"),
        "info should not print a count footer: {stdout}"
    );
}

#[test]
fn test_sessions_info_help_annotations() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["sessions", "info", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("URL/origin"))
        .stdout(predicate::str::contains("defaults to Tempo"));
}

#[test]
fn test_sessions_recover_help_annotations() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["sessions", "recover", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Re-sync a local session's state"));
}

#[test]
fn test_sessions_close_json_uses_normalized_origin() {
    let temp = TestConfigBuilder::new().build();
    seed_local_session(&temp, "https://example.com");

    // Close using a URL with a path — JSON should report the stored normalized origin
    let output = test_command(&temp)
        .args(["-j", "sessions", "close", "https://example.com/v1/path?x=1"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let val: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let results = val["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["origin"], "https://example.com");
}

// ==================== Services ====================

#[test]
fn test_services_help() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["services", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("info"))
        .stdout(predicate::str::contains("--category"))
        .stdout(predicate::str::contains("--search"));
}

#[test]
fn test_services_shows_help_with_no_args() {
    // `tempo-wallet services` without network hits the API, but `--help` should work offline.
    // Test that the bare subcommand is recognized (won't error with "not a command").
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["services", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Browse the MPP service directory"));
}

#[test]
fn test_services_info_missing_id() {
    // `tempo-wallet services info` without a service ID should fail
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["services", "info"])
        .assert()
        .failure();
}

#[test]
fn test_services_bare_id_is_accepted() {
    // `tempo-wallet services fal` should be parsed as a valid command (shorthand for `services info fal`)
    // It will fail at runtime (network) but should not fail argument parsing.
    // We test via --help to confirm the positional arg is documented.
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["services", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("SERVICE_ID"));
}

// ==================== TOON Output (-t / --toon-output) ====================

#[test]
fn test_session_list_toon_output() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["-t", "sessions", "list"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.trim().is_empty(),
        "toon output should not be empty: {stdout}"
    );
    assert!(
        !stdout.trim().starts_with('{'),
        "toon output should not be JSON: {stdout}"
    );
    assert!(
        stdout.contains("total"),
        "toon output should contain 'total' field: {stdout}"
    );
}

#[test]
fn test_toon_output_flag_long_form() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["--toon-output", "sessions", "list"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.trim().is_empty(),
        "toon output (long form) should not be empty: {stdout}"
    );
}

#[test]
fn test_toon_error_output_when_output_format_toon() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["-t", "-n", "not-a-network", "whoami"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("code"),
        "toon error output should contain 'code': {stdout}"
    );
    assert!(
        !stdout.trim().starts_with('{'),
        "toon error output should not be JSON: {stdout}"
    );
}

#[test]
fn test_toon_and_json_output_conflict() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["-t", "-j", "whoami"])
        .output()
        .unwrap();
    assert!(!output.status.success(), "toon + json should conflict");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cannot be used with") || stderr.to_lowercase().contains("conflict"),
        "should mention conflict: {stderr}"
    );
}

#[test]
fn test_key_list_toon_has_fields() {
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp)
        .args(["-t", "keys", "list"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("keys"),
        "toon keys list should contain 'keys': {stdout}"
    );
    assert!(
        stdout.contains("total"),
        "toon keys list should contain 'total': {stdout}"
    );
}
