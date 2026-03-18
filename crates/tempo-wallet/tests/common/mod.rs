//! Common test utilities for tempo-wallet CLI tests
//!
//! Keep this module minimal so all test targets can include it without lint allowances.

/// Create a test command for tempo-wallet with proper environment variables set.
pub(crate) fn test_command(temp_dir: &tempfile::TempDir) -> std::process::Command {
    tempo_test::make_test_command(
        assert_cmd::cargo::cargo_bin!("tempo-wallet").to_path_buf(),
        temp_dir,
    )
}
