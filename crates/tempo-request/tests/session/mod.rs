//! Session-intent integration scenarios and shared harnesses.

// Re-export shared helpers so split scenario modules can continue using `use super::*;`.
pub(crate) use crate::common::test_command;
pub(crate) use tempo_common::session::session_key;
pub(crate) use tempo_test::{get_combined_output, setup_config_only, MODERATO_PRIVATE_KEY};

mod harness;
pub(crate) use harness::*;

mod lifecycle;
mod spec_alignment;
mod streaming;
