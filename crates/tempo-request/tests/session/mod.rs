//! Session-intent integration scenarios and shared harnesses.

// Re-export shared helpers so split scenario modules can continue using `use super::*;`.
pub(crate) use crate::common::test_command;
pub(crate) use tempo_test::{get_combined_output, setup_config_only, MODERATO_PRIVATE_KEY};

pub(crate) fn payment_origin_lock_key(url_or_origin: &str) -> String {
    let normalized = url::Url::parse(url_or_origin).map_or_else(
        |_| url_or_origin.to_string(),
        |u| u.origin().ascii_serialization(),
    );
    let safe: String = normalized
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("payment_{safe}")
}

mod harness;
pub(crate) use harness::*;

mod lifecycle;
mod spec_alignment;
mod streaming;
