use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::process::Command;

#[test]
fn wallet_help_includes_identity_commands() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("login"))
        .stdout(predicate::str::contains("logout"))
        .stdout(predicate::str::contains("whoami"));
}

#[test]
fn wallet_rejects_query_command_with_migration_hint() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .args(["query", "https://example.com"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unrecognized"));
}

#[test]
fn wallet_rejects_services_command_with_migration_hint() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .arg("services")
        .assert()
        .failure()
        .stderr(predicate::str::contains("unrecognized"));
}
