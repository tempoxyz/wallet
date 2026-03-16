//! Integration tests for tempo-request commands.
//!
//! These run in normal `cargo test` — no network or funded wallet required.

#[path = "commands/analytics.rs"]
mod analytics;
#[path = "commands/basic.rs"]
mod basic;
mod common;
#[path = "commands/errors.rs"]
mod errors;
#[path = "commands/misc.rs"]
mod misc;
#[path = "commands/parity.rs"]
mod parity;
#[path = "commands/payment.rs"]
mod payment;
#[path = "commands/streaming.rs"]
mod streaming;

use crate::common::{
    assert_exit_code, charge_www_authenticate_with_realm, get_combined_output, setup_config_only,
    test_command, write_test_files, MockRpcServer, MockServer, PaymentTestHarness,
    TestConfigBuilder, HARDHAT_PRIVATE_KEY, MODERATO_CHARGE_CHALLENGE,
};
