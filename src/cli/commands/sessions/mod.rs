//! Session management commands.

mod close;
mod display;
mod info;
mod list;
mod sync;

use anyhow::Result;

use crate::cli::args::SessionCommands;
use crate::cli::Context;

// Common re-exports for submodules
use crate::payment::session::store as session_store;
use crate::payment::session::store::SessionStatus;
use crate::payment::session::DEFAULT_GRACE_PERIOD_SECS;

pub(crate) async fn run(ctx: &Context, command: Option<SessionCommands>) -> Result<()> {
    // `subcommand_required = true` in args.rs ensures `command` is always `Some`
    match command.expect("sessions subcommand required") {
        SessionCommands::List { state } => list::list_sessions(ctx, state).await,
        SessionCommands::Info { target } => info::show_session_info(ctx, &target).await,
        SessionCommands::Close {
            url,
            all,
            orphaned,
            finalize,
        } => close::close_sessions(ctx, url, all, orphaned, finalize).await,
        SessionCommands::Sync { origin } => sync::sync_sessions(ctx, origin.as_deref()).await,
    }
}
