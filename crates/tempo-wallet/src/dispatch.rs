//! Top-level command dispatch and analytics tracking.

use anyhow::Result;

use crate::args::{Cli, Commands, KeyCommands};
use crate::commands::{completions, keys, login, logout, wallets, whoami};

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

        tempo_common::cli::run_cli(&self.global, &["tempo_wallet"], |ctx| async move {
            let cmd_name = command_name(&command);
            tempo_common::cli::tracking::track_command(&ctx.analytics, cmd_name);
            let result = match command {
                Commands::Login => login::run(&ctx).await,
                Commands::Logout { yes } => logout::run(&ctx, yes),
                Commands::Completions { shell } => completions::run(&ctx, shell),
                Commands::List => wallets::list(&ctx),
                Commands::Create => wallets::create(&ctx).await,
                Commands::Fund { address, no_wait } => wallets::fund(&ctx, address, no_wait).await,
                Commands::Whoami => whoami::run(&ctx).await,
                Commands::Keys { command } => keys::run(&ctx, command).await,
            };
            (cmd_name, result)
        })
        .await
    }
}

/// Derive a short analytics-friendly name from a parsed command.
fn command_name(command: &Commands) -> &'static str {
    match command {
        Commands::Login => "login",
        Commands::Logout { .. } => "logout",
        Commands::Completions { .. } => "completions",
        Commands::List => "list",
        Commands::Create => "create",
        Commands::Fund { .. } => "fund",
        Commands::Whoami => "whoami",
        Commands::Keys { command } => match command {
            KeyCommands::List => "keys list",
            KeyCommands::Create { .. } => "keys create",
            KeyCommands::Clean { .. } => "keys clean",
        },
    }
}
