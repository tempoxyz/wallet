//! Organized query-command integration suites.
//!
//! `basic` covers request flags and non-payment behavior.
//! `payment` covers 402 handling and key provisioning paths.
//! `streaming` covers SSE/NDJSON output behavior.
//! `parity` covers curl-like compatibility flags.
//! `errors` covers malformed inputs and offline behavior.
//! `analytics` covers event sequencing and redaction checks.
//! `misc` covers TOON, metadata, and remaining edge cases.

use std::process::Output;

use crate::common::test_command;
use tempo_test::{
    assert_exit_code, charge_www_authenticate_with_realm, get_combined_output, setup_config_only,
    write_test_files, MockRpcServer, MockServer, PaymentTestHarness, TestConfigBuilder,
    MODERATO_CHARGE_CHALLENGE, MODERATO_PRIVATE_KEY,
};

fn parse_events_log(path: &std::path::Path) -> Vec<(String, serde_json::Value)> {
    let content = std::fs::read_to_string(path).unwrap_or_default();
    content
        .lines()
        .filter_map(|line| {
            let (name, json_str) = line.split_once('|')?;
            let value: serde_json::Value = serde_json::from_str(json_str).ok()?;
            Some((name.to_string(), value))
        })
        .collect()
}

fn assert_success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context}: {}",
        get_combined_output(output)
    );
}

fn assert_stdout_contains(output: &Output, needle: &str, context: &str) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(needle), "{context}: {stdout}");
}

mod analytics;
mod basic;
mod errors;
mod misc;
mod parity;
mod payment;
mod streaming;
