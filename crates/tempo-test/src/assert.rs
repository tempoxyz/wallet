//! Assertion and parsing helpers for CLI integration tests.

use std::process::Output;

use serde_json::Value;

/// Assert the process exited with a specific exit code.
///
/// # Panics
///
/// Panics when the process exit code does not match `expected`.
pub fn assert_exit_code(output: &Output, expected: i32, context: &str) {
    assert_eq!(
        output.status.code(),
        Some(expected),
        "expected exit code {expected}: {context}"
    );
}

/// Assert clean stderr (empty) — required for structured output modes.
///
/// # Panics
///
/// Panics when stderr is not empty.
pub fn assert_clean_stderr(output: &Output) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.trim().is_empty(),
        "structured mode should not write to stderr: {stderr}"
    );
}

/// Parse stdout as JSON, panicking with context on failure.
///
/// # Panics
///
/// Panics when stdout is not valid JSON.
#[must_use]
pub fn parse_json_stdout(output: &Output) -> Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("invalid JSON stdout: {e}\n---\n{stdout}"))
}

/// Parse stdout as TOON, panicking with context on failure.
///
/// # Panics
///
/// Panics when stdout is not valid TOON.
#[must_use]
pub fn parse_toon_stdout(output: &Output) -> Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    toon_format::decode_default(stdout.trim())
        .unwrap_or_else(|e| panic!("invalid TOON stdout: {e}\n---\n{stdout}"))
}

/// Assert a structured error response (JSON) has the expected code.
///
/// # Panics
///
/// Panics when stdout is not valid JSON or expected fields are missing/mismatched.
pub fn assert_structured_error(output: &Output, expected_code: &str) {
    let json = parse_json_stdout(output);
    assert_eq!(json["code"], expected_code, "wrong error code in: {json}");
    assert!(json["message"].is_string(), "missing message in: {json}");
}

/// Assert JSON and TOON decoded payloads are equivalent.
///
/// # Panics
///
/// Panics when the payloads differ.
pub fn assert_json_toon_equivalent(json: &Value, toon: &Value) {
    assert_eq!(json, toon, "JSON and TOON decoded payloads diverged");
}

/// Combine stdout and stderr from a process output into a single string.
#[must_use]
pub fn get_combined_output(output: &Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    format!("{stdout}{stderr}")
}
