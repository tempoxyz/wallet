//! Test command helpers.

use std::process::Command;
use tempfile::TempDir;

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
pub fn make_test_command(binary_path: std::path::PathBuf, temp_dir: &TempDir) -> Command {
    let mut cmd = Command::new(binary_path);

    // Set HOME so ~/.tempo resolves inside the temp directory
    cmd.env("HOME", temp_dir.path());

    // Prevent whoami from auto-triggering browser login in tests
    cmd.env("TEMPO_NO_AUTO_LOGIN", "1");

    // Disable auto-JSON detection (tests capture stdout, which is not a TTY)
    cmd.env("TEMPO_NO_AUTO_JSON", "1");

    cmd
}

/// Combine stdout and stderr from a process output into a single string.
pub fn get_combined_output(output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    format!("{}{}", stdout, stderr)
}
