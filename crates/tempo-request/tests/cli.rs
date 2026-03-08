use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::process::Command;

mod common;

#[test]
fn request_help_shows_usage() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-request"))
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("HTTP"));
}

#[test]
fn request_accepts_url() {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-request"))
        .arg("http://example.com")
        .assert()
        .success();
}
