//! Shared analytics tracking for CLI commands.

use crate::analytics::{events, Analytics, CommandFailurePayload, CommandRunPayload};
use crate::security::sanitize_error;

/// Track the initial command run event.
pub fn track_command(analytics: &Option<Analytics>, cmd_name: &str) {
    if let Some(ref a) = analytics {
        a.track(
            events::COMMAND_RUN,
            CommandRunPayload {
                command: cmd_name.to_string(),
            },
        );
    }
}

/// Track command success or failure.
pub fn track_result(analytics: &Option<Analytics>, cmd_name: &str, result: &anyhow::Result<()>) {
    let Some(ref a) = analytics else { return };
    match result {
        Ok(()) => {
            a.track(
                events::COMMAND_SUCCESS,
                CommandRunPayload {
                    command: cmd_name.to_string(),
                },
            );
        }
        Err(e) => {
            a.track(
                events::COMMAND_FAILURE,
                CommandFailurePayload {
                    command: cmd_name.to_string(),
                    error: sanitize_error(&e.to_string()),
                },
            );
        }
    }
}
