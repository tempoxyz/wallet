//! Application entry point: build context, dispatch command, flush analytics.

use anyhow::Result;

use crate::args::{Cli, Commands, ServicesCommands, SessionCommands};
use crate::commands::{completions, services, sessions};

/// Run the tempo-mpp application.
pub(crate) async fn run(mut cli: Cli) -> Result<()> {
    let command = match cli.command.take() {
        Some(c) => c,
        None => {
            use clap::CommandFactory;
            return Cli::command().print_help().map_err(Into::into);
        }
    };

    tempo_common::cli::run_cli(&cli.global, &["tempo_mpp", "mpp"], |ctx| async move {
        let cmd_name = command_name(&command);
        tempo_common::cli::tracking::track_command(&ctx.analytics, cmd_name);
        let result = match command {
            Commands::Completions { shell } => completions::run(&ctx, shell),
            Commands::Sessions { command } => sessions::run(&ctx, command).await,
            Commands::Services {
                command,
                service_id,
                category,
                search,
            } => {
                services::run(
                    &ctx,
                    services::ServicesArgs {
                        command,
                        service_id,
                        category,
                        search,
                    },
                )
                .await
            }
        };
        (cmd_name, result)
    })
    .await
}

/// Derive a short analytics-friendly name from a parsed command.
fn command_name(command: &Commands) -> &'static str {
    match command {
        Commands::Completions { .. } => "completions",
        Commands::Sessions { command } => match command {
            SessionCommands::List { .. } => "sessions list",
            SessionCommands::Info { .. } => "sessions info",
            SessionCommands::Close { .. } => "sessions close",
            SessionCommands::Sync { .. } => "sessions sync",
        },
        Commands::Services { command, .. } => match command {
            Some(ServicesCommands::List) => "services list",
            Some(ServicesCommands::Info { .. }) => "services info",
            None => "services",
        },
    }
}
