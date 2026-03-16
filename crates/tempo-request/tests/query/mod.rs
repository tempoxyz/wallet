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

use crate::common::{
    assert_exit_code, charge_www_authenticate_with_realm, get_combined_output, parse_events_log,
    setup_config_only, test_command, write_test_files, MockRpcServer, MockServer,
    PaymentTestHarness, TestConfigBuilder, MODERATO_CHARGE_CHALLENGE, MODERATO_PRIVATE_KEY,
};

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
