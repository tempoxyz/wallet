//! Common test utilities for tempo-wallet CLI tests
//!
//! Not every helper is used in every test binary — suppress false positives.
#![allow(dead_code)]
#![allow(clippy::redundant_pub_crate)]

pub(crate) use tempo_test::*;

/// Create a test command for tempo-wallet with proper environment variables set.
pub(crate) fn test_command(temp_dir: &tempfile::TempDir) -> std::process::Command {
    tempo_test::make_test_command(
        assert_cmd::cargo::cargo_bin!("tempo-wallet").to_path_buf(),
        temp_dir,
    )
}
