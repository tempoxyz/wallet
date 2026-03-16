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

/// Parse `TEMPO_TEST_EVENTS` lines into `(event_name, props)` pairs.
pub(crate) fn parse_events_log(path: &std::path::Path) -> Vec<(String, serde_json::Value)> {
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
