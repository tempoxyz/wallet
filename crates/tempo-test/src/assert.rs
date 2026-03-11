//! Assertion helpers for CLI integration tests.

use std::process::Output;

use serde_json::Value;

/// Assert the process exited with a specific exit code.
pub fn assert_exit_code(output: &Output, expected: i32, context: &str) {
    assert_eq!(
        output.status.code(),
        Some(expected),
        "expected exit code {expected}: {context}"
    );
}

/// Assert clean stderr (empty) — required for structured output modes.
pub fn assert_clean_stderr(output: &Output) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.trim().is_empty(),
        "structured mode should not write to stderr: {stderr}"
    );
}

/// Parse stdout as JSON, panicking with context on failure.
pub fn parse_json_stdout(output: &Output) -> Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("invalid JSON stdout: {e}\n---\n{stdout}"))
}

/// Parse stdout as TOON, panicking with context on failure.
pub fn parse_toon_stdout(output: &Output) -> Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    toon_format::decode_default(stdout.trim())
        .unwrap_or_else(|e| panic!("invalid TOON stdout: {e}\n---\n{stdout}"))
}

/// Assert a structured error response (JSON) has the expected code.
pub fn assert_structured_error(output: &Output, expected_code: &str) {
    let json = parse_json_stdout(output);
    assert_eq!(json["code"], expected_code, "wrong error code in: {json}");
    assert!(json["message"].is_string(), "missing message in: {json}");
}

/// Assert JSON and TOON decoded payloads are equivalent.
pub fn assert_json_toon_equivalent(json: &Value, toon: &Value) {
    assert_eq!(json, toon, "JSON and TOON decoded payloads diverged");
}

/// Run a command with a format flag (`-j` or `-t`) prepended, parse stdout.
pub fn run_structured(
    cmd_fn: impl Fn(&tempfile::TempDir) -> std::process::Command,
    temp: &tempfile::TempDir,
    flag: &str,
    args: &[&str],
) -> (Output, Value) {
    let mut cmd = cmd_fn(temp);
    let all_args: Vec<&str> = std::iter::once(flag).chain(args.iter().copied()).collect();
    let output = cmd.args(all_args).output().expect("command should run");
    assert!(output.status.success(), "command failed: {output:?}");

    let parsed = if flag == "-j" {
        parse_json_stdout(&output)
    } else {
        parse_toon_stdout(&output)
    };

    (output, parsed)
}

/// Run a command in both JSON and TOON formats, returning all outputs and parsed values.
///
/// `cmd_fn` should be the crate-specific `test_command` function (e.g., from `common/mod.rs`).
pub fn run_structured_both(
    cmd_fn: impl Fn(&tempfile::TempDir) -> std::process::Command,
    temp: &tempfile::TempDir,
    args: &[&str],
) -> (Output, Value, Output, Value) {
    let (json_out, json_val) = run_structured(&cmd_fn, temp, "-j", args);
    let (toon_out, toon_val) = run_structured(&cmd_fn, temp, "-t", args);
    (json_out, json_val, toon_out, toon_val)
}
