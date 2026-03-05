//! Session management commands.

mod close;
mod info;
mod list;
mod recover;
pub(super) mod render;
mod sync;

use close::close_sessions;
use info::show_session_info;
use list::{list_sessions, SessionState as ListSessionState};
use recover::recover_session;
use sync::sync_sessions;

use anyhow::Result;
use clap::CommandFactory;

use crate::cli::args::{SessionCommands, SessionStateArg};
use crate::cli::{Cli, Context};

pub(crate) async fn run(ctx: &Context, command: Option<SessionCommands>) -> Result<()> {
    let output_format = ctx.output_format;

    if let Some(subcommand) = command {
        let show_output = ctx.cli.verbosity().show_output;
        match subcommand {
            SessionCommands::List { state } => {
                let mut selected: Vec<ListSessionState> = Vec::new();
                if state.is_empty() {
                    // default will be applied in list_sessions
                } else if state.iter().any(|s| matches!(s, SessionStateArg::All)) {
                    selected = vec![
                        ListSessionState::Active,
                        ListSessionState::Closing,
                        ListSessionState::Finalizable,
                        ListSessionState::Orphaned,
                    ];
                } else {
                    for s in state {
                        let m = match s {
                            SessionStateArg::Active => ListSessionState::Active,
                            SessionStateArg::Closing => ListSessionState::Closing,
                            SessionStateArg::Finalizable => ListSessionState::Finalizable,
                            SessionStateArg::Orphaned => ListSessionState::Orphaned,
                            SessionStateArg::All => continue,
                        };
                        selected.push(m);
                    }
                }
                list_sessions(
                    &ctx.config,
                    output_format,
                    &selected,
                    ctx.network,
                    &ctx.keys,
                )
                .await
            }
            SessionCommands::Info { target } => {
                show_session_info(&ctx.config, output_format, &target, ctx.network).await
            }
            SessionCommands::Close {
                url,
                all,
                orphaned,
                closed,
            } => {
                close_sessions(
                    &ctx.config,
                    url,
                    all,
                    orphaned,
                    closed,
                    output_format,
                    show_output,
                    ctx.network,
                    ctx.analytics.as_ref(),
                    &ctx.keys,
                )
                .await
            }
            SessionCommands::Recover { origin } => {
                recover_session(&ctx.config, output_format, &origin, ctx.analytics.as_ref()).await
            }
            SessionCommands::Sync => sync_sessions(&ctx.config, output_format, show_output).await,
        }
    } else {
        if let Some(session_cmd) = Cli::command().find_subcommand_mut("sessions") {
            session_cmd.print_help()?;
        } else {
            Cli::command().print_help()?;
        }
        Ok(())
    }
}
