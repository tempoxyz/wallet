//! Session management commands.

mod close;
mod info;
mod list;
mod sync;
mod view;

use anyhow::Result;

use crate::cli::args::SessionCommands;
use crate::cli::Context;

pub(crate) async fn run(ctx: &Context, command: Option<SessionCommands>) -> Result<()> {
    if let Some(subcommand) = command {
        match subcommand {
            SessionCommands::List { state } => list::list_sessions(ctx, state).await,
            SessionCommands::Info { target } => info::show_session_info(ctx, &target).await,
            SessionCommands::Close {
                url,
                all,
                orphaned,
                finalize,
            } => close::close_sessions(ctx, url, all, orphaned, finalize).await,
            SessionCommands::Sync { origin } => {
                sync::sync_sessions(ctx, origin.as_deref()).await
            }
        }
    } else {
        use clap::CommandFactory;
        if let Some(session_cmd) = crate::cli::Cli::command().find_subcommand_mut("sessions") {
            session_cmd.print_help()?;
        } else {
            crate::cli::Cli::command().print_help()?;
        }
        Ok(())
    }
}
