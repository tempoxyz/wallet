//! Test command construction and structured test runners.

use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

use crate::assert::{parse_json_stdout, parse_toon_stdout};

/// Create a test command for the given binary path with proper environment variables set.
///
/// Each binary crate's `tests/common/mod.rs` should wrap this with the
/// `cargo_bin!` macro to resolve the binary path at compile time:
///
/// ```ignore
/// pub fn test_command(temp_dir: &TempDir) -> Command {
///     tempo_test::make_test_command(
///         assert_cmd::cargo::cargo_bin!("tempo-request"),
///         temp_dir,
///     )
/// }
/// ```
#[must_use]
pub fn make_test_command(binary_path: std::path::PathBuf, temp_dir: &TempDir) -> Command {
    let mut cmd = Command::new(binary_path);

    // Set HOME so ~/.tempo resolves inside the temp directory
    cmd.env("HOME", temp_dir.path());

    // Prevent whoami from auto-triggering browser login in tests
    cmd.env("TEMPO_NO_AUTO_LOGIN", "1");

    // Disable auto-JSON detection (tests capture stdout, which is not a TTY)
    cmd.env("TEMPO_NO_AUTO_JSON", "1");

    // Ensure analytics-dependent tests can exercise event emission even when
    // the developer environment does not provide a PostHog key.
    cmd.env("POSTHOG_API_KEY", "test-posthog-key");

    // Clear agent env vars so tests don't auto-select TOON when run inside
    // an LLM agent host (Amp, Claude Code, Codex, Cursor, etc.)
    cmd.env_remove("AGENT");
    cmd.env_remove("CLAUDE_CODE");
    cmd.env_remove("CODEX");
    cmd.env_remove("AMP_THREAD_ID");
    cmd.env_remove("CURSOR_TRACE_ID");

    cmd
}

/// Run a command with a format flag (`-j` or `-t`) prepended, parse stdout.
///
/// # Panics
///
/// Panics when command execution fails or the command exits unsuccessfully.
pub fn run_structured(
    cmd_fn: impl Fn(&TempDir) -> Command,
    temp: &TempDir,
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
    cmd_fn: impl Fn(&TempDir) -> Command,
    temp: &TempDir,
    args: &[&str],
) -> (Output, Value, Output, Value) {
    let (json_out, json_val) = run_structured(&cmd_fn, temp, "-j", args);
    let (toon_out, toon_val) = run_structured(&cmd_fn, temp, "-t", args);
    (json_out, json_val, toon_out, toon_val)
}
