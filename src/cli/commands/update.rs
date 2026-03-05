//! Update command: self-update presto to the latest version.

use anyhow::Result;

use crate::analytics::{self, Event};
use crate::cli::Context;
use crate::util::sanitize_error;

/// Run the update command with analytics tracking.
pub(crate) async fn run(ctx: &Context, yes: bool) -> Result<()> {
    let result = crate::version::run_update(yes);
    if let Some(ref a) = ctx.analytics {
        match &result {
            Ok(()) => {
                a.track(Event::UpdateSuccess, analytics::EmptyPayload);
            }
            Err(e) => {
                a.track(
                    Event::UpdateFailure,
                    analytics::UpdateFailurePayload {
                        error: sanitize_error(&e.to_string()),
                    },
                );
            }
        }
    }
    result
}
