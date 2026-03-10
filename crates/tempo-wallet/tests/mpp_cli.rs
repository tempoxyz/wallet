use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::io::Write;
use std::process::Stdio;

mod common;
use common::{test_command, TestConfigBuilder};

/// Valid charge challenge for Tempo mainnet (chainId 4217).
///
/// request = base64url({"amount":"1000","currency":"0x20c000000000000000000000b9537d11c60e8b50","methodDetails":{"chainId":4217}})
const VALID_CHARGE_CHALLENGE: &str = r#"Payment id="test", realm="test", method="tempo", intent="charge", request="eyJhbW91bnQiOiIxMDAwIiwiY3VycmVuY3kiOiIweDIwYzAwMDAwMDAwMDAwMDAwMDAwMDAwMGI5NTM3ZDExYzYwZThiNTAiLCJtZXRob2REZXRhaWxzIjp7ImNoYWluSWQiOjQyMTd9fQ""#;

#[test]
fn sign_help_shows_flags() {
    let temp = TestConfigBuilder::new().build();
    let mut cmd = test_command(&temp);
    cmd.args(["sign", "--help"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("--challenge"))
        .stdout(predicate::str::contains("--dry-run"));
}

#[test]
fn sign_dry_run_valid_challenge() {
    let temp = TestConfigBuilder::new().build();
    let mut cmd = test_command(&temp);
    cmd.args(["sign", "--dry-run", "--challenge", VALID_CHARGE_CHALLENGE]);
    cmd.assert()
        .success()
        .stderr(predicate::str::contains("Challenge is valid"));
}

#[test]
fn sign_dry_run_invalid_challenge() {
    let temp = TestConfigBuilder::new().build();
    let mut cmd = test_command(&temp);
    cmd.args(["sign", "--dry-run", "--challenge", "not a valid challenge"]);
    cmd.assert().failure();
}

#[test]
fn sign_dry_run_unsupported_method() {
    let temp = TestConfigBuilder::new().build();
    let challenge = r#"Payment id="x", realm="x", method="stripe", intent="charge", request="e30""#;
    let mut cmd = test_command(&temp);
    cmd.args(["sign", "--dry-run", "--challenge", challenge]);
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("Unsupported"));
}

#[test]
fn sign_dry_run_missing_chain_id() {
    let temp = TestConfigBuilder::new().build();
    // request = base64url({"amount":"1000","currency":"0x00"}) — no methodDetails/chainId
    let challenge = r#"Payment id="x", realm="x", method="tempo", intent="charge", request="eyJhbW91bnQiOiIxMDAwIiwiY3VycmVuY3kiOiIweDAwIn0""#;
    let mut cmd = test_command(&temp);
    cmd.args(["sign", "--dry-run", "--challenge", challenge]);
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("chainId"));
}

#[test]
fn sign_no_wallet_configured() {
    let temp = TestConfigBuilder::new().build();
    let mut cmd = test_command(&temp);
    cmd.args(["sign", "--challenge", VALID_CHARGE_CHALLENGE]);
    cmd.assert().failure();
}

#[test]
fn sign_empty_stdin_fails() {
    let temp = TestConfigBuilder::new().build();
    let mut cmd = test_command(&temp);
    cmd.arg("sign");
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("Failed to spawn");
    drop(child.stdin.take()); // close stdin immediately
    let output = child.wait_with_output().expect("Failed to wait");
    assert!(!output.status.success());
}

#[test]
fn sign_dry_run_reads_from_stdin() {
    let temp = TestConfigBuilder::new().build();
    let mut cmd = test_command(&temp);
    cmd.args(["sign", "--dry-run"]);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("Failed to spawn");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(VALID_CHARGE_CHALLENGE.as_bytes())
        .unwrap();
    drop(child.stdin.take());
    let output = child.wait_with_output().expect("Failed to wait");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "sign via stdin failed: {stderr}");
    assert!(stderr.contains("Challenge is valid"), "stderr: {stderr}");
}
