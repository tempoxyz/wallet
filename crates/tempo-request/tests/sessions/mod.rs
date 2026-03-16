//! Session-intent integration scenarios and shared harnesses.

// Re-export shared helpers so split scenario modules can continue using `use super::*;`.
pub(crate) use crate::common::{
    get_combined_output, setup_config_only, test_command, MODERATO_PRIVATE_KEY,
};
pub(crate) use tempo_common::payment::session::session_key;

mod harness;
pub(crate) use harness::*;

mod lifecycle;
mod spec_alignment;
mod streaming;
