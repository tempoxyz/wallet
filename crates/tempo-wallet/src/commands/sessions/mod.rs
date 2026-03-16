//! Session management commands.

mod close;
mod list;
mod render;
mod sync;
mod util;

use crate::args::SessionCommands;
use tempo_common::{cli::context::Context, error::TempoError};

// Common imports shared by submodules
use tempo_common::payment::{session as session_store, session::ChannelStatus};

pub(crate) async fn run(ctx: &Context, command: SessionCommands) -> Result<(), TempoError> {
    match command {
        SessionCommands::List { state } => list::list_channels(ctx, state).await,
        SessionCommands::Close {
            url,
            all,
            orphaned,
            finalize,
            cooperative,
            dry_run,
        } => {
            close::close_sessions(ctx, url, all, orphaned, finalize, cooperative, dry_run).await
        }
        SessionCommands::Sync { origin } => sync::sync_sessions(ctx, origin.as_deref()).await,
    }
}
