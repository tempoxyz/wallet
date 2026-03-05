//! Snapshot-like structure tests for JSON and TOON outputs.

use serde_json::Value;

mod common;
use common::{seed_local_session, test_command, TestConfigBuilder};

#[test]
fn whoami_json_and_toon_have_expected_shape() {
    // Keys.toml minimal to make whoami output deterministic enough for structure checks
    let keys = r#"[[keys]]
wallet_address = "0x0000000000000000000000000000000000000001"
key_address = "0x0000000000000000000000000000000000000001"
chain_id = 4217
provisioned = true
"#;
    let temp = TestConfigBuilder::new().with_keys_toml(keys).build();

    // JSON
    let output = test_command(&temp).args(["-j", "whoami"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: Value = serde_json::from_str(stdout.trim()).expect("valid json");
    // Snapshot-ish: check stable keys exist
    assert!(v.get("wallet").is_some(), "missing wallet");
    assert!(v.get("key").is_some(), "missing key");

    // TOON
    let output = test_command(&temp).args(["-t", "whoami"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Decode TOON to JSON to check shape
    let decoded: Value = toon_format::decode_default(&stdout).expect("toon decode");
    assert!(decoded.get("wallet").is_some());
    assert!(decoded.get("key").is_some());
}

#[test]
fn keys_list_json_and_toon_have_expected_shape() {
    let keys = r#"[[keys]]
wallet_address = "0x0000000000000000000000000000000000000001"
key_address = "0x0000000000000000000000000000000000000001"
chain_id = 4217
provisioned = true
"#;
    let temp = TestConfigBuilder::new().with_keys_toml(keys).build();

    // JSON
    let output = test_command(&temp)
        .args(["keys", "list", "-j"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: Value = serde_json::from_str(stdout.trim()).expect("valid json");
    assert!(v.get("keys").is_some());

    // TOON
    let output = test_command(&temp)
        .args(["keys", "list", "-t"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let decoded: Value = toon_format::decode_default(&stdout).expect("toon decode");
    assert!(decoded.get("keys").is_some());
}

#[test]
fn sessions_list_json_and_toon_have_expected_shape() {
    let temp = TestConfigBuilder::new().build();
    // Seed one local session for a known origin
    seed_local_session(&temp, "https://example.com");

    // JSON
    let output = test_command(&temp)
        .args(["sessions", "list", "-j"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: Value = serde_json::from_str(stdout.trim()).expect("valid json");
    // Expect an array or object with sessions field; tests assert one of them exists
    if v.is_array() {
        assert!(!v.as_array().unwrap().is_empty(), "expected sessions");
    } else {
        assert!(v.get("sessions").is_some(), "missing sessions field");
    }

    // TOON
    let output = test_command(&temp)
        .args(["sessions", "list", "-t"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let decoded: Value = toon_format::decode_default(&stdout).expect("toon decode");
    if decoded.is_array() {
        assert!(!decoded.as_array().unwrap().is_empty(), "expected sessions");
    } else {
        assert!(decoded.get("sessions").is_some());
    }
}
