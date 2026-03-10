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
fn wallet_help_includes_mpp_commands() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("sessions"))
        .stdout(predicate::str::contains("services"))
        .stdout(predicate::str::contains("sign"));
}

#[test]
fn wallet_rejects_query_command() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-wallet"))
        .env("TEMPO_NO_AUTO_JSON", "1")
        .args(["query", "https://example.com"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unrecognized"));
}
