use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::process::Command;

#[test]
fn mpp_help_includes_mpp_commands() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-mpp"))
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("sessions"))
        .stdout(predicate::str::contains("services"));
}

#[test]
fn mpp_rejects_unknown_subcommand() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-mpp"))
        .arg("whoami")
        .assert()
        .failure()
        .stderr(predicate::str::contains("unrecognized"));
}
