//! Top-level command dispatch and analytics tracking.

use anyhow::Result;

use crate::args::{Cli, Commands, ServicesCommands, SessionCommands};
use crate::commands::{completions, services, sessions};

impl Cli {
    /// Application entry point: build context, dispatch command, flush analytics.
    pub(crate) async fn run(mut self) -> Result<()> {
        let command = match self.command.take() {
            Some(c) => c,
            None => {
                use clap::CommandFactory;
                return Self::command().print_help().map_err(Into::into);
            }
        };

        tempo_common::cli::run_cli(&self.global, &["tempo_mpp", "mpp"], |ctx| async move {
            let cmd_name = command_name(&command);
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
