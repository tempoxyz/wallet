//! Integration tests for CLI features:
//! - Shell completions (bash, zsh, fish, powershell)
//! - Command aliases (short forms for common commands)
//! - Display options (verbosity, quiet mode, color control)
//! - Help output organization (sections, annotations)

use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::process::Command;

mod common;
use common::{test_command, TestConfigBuilder};

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
fn test_help_shows_default_values() {
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("[default: auto]"));

    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .args(["query", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("[default: text]"));
}

#[test]
fn test_help_shows_possible_values() {
    Command::new(assert_cmd::cargo::cargo_bin!("presto"))
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("[possible values:"))
        .stdout(predicate::str::contains("auto, always, never"));
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
        .stdout(predicate::str::contains("Log in"));
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
        .args(["completions", "-v", "-q", "--color", "never"])
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
        .stdout(predicate::str::contains("wallet"))
        .stdout(predicate::str::contains("--output-format"));
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
        .args(["-i", "http://example.com"])
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
