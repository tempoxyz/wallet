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
fn mpp_accepts_implicit_query_url() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-mpp"))
        .arg("http://example.com")
        .assert()
        .success();
}

#[test]
fn mpp_rejects_whoami_with_migration_hint() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-mpp"))
        .arg("whoami")
        .assert()
        .failure()
        .stderr(predicate::str::contains("not a tempo-mpp command"));
}
