//! Shared analytics tracking for CLI commands.

use std::time::Duration;

use crate::{
    analytics::{events, Analytics, CommandFailurePayload, CommandSuccessPayload},
    security::sanitize_error,
};

/// Track command success or failure with execution duration.
pub fn track_result<E>(
    analytics: &Option<Analytics>,
    cmd_name: &str,
    result: &Result<(), E>,
    duration: Duration,
) where
    E: std::fmt::Display,
{
    let Some(ref a) = analytics else { return };
    match result {
        Ok(()) => {
            a.track(
                events::COMMAND_SUCCESS,
                CommandSuccessPayload {
                    command: cmd_name.to_string(),
                    duration_ms: duration.as_millis(),
                },
            );
        }
        Err(e) => {
            a.track(
                events::COMMAND_FAILURE,
                CommandFailurePayload {
                    command: cmd_name.to_string(),
                    error: sanitize_error(&e.to_string()),
                    duration_ms: duration.as_millis(),
                },
            );
        }
    }
}
