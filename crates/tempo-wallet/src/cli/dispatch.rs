//! Top-level command dispatch and analytics tracking.

use anyhow::Result;

use super::commands::{completions, keys, login, logout, wallets, whoami};
use super::{Cli, Commands};
use tempo_common::cli::dispatch::{track_command, track_result};
use tempo_common::context::{Context, ContextArgs};

use crate::cli::args::KeyCommands;

impl Cli {
    /// Application entry point: build context, dispatch command, flush analytics.
    pub(crate) async fn run(mut self) -> Result<()> {
        tempo_common::runtime::init_tracing(
            self.global.silent,
            self.global.verbose,
            &["tempo_wallet"],
        );
        tempo_common::runtime::init_color_support(self.global.color);

        let command = match self.command.take() {
            Some(c) => c,
            None => {
                use clap::CommandFactory;
                return Self::command().print_help().map_err(Into::into);
            }
        };

        let ctx = Context::build(ContextArgs {
            config_path: self.global.config.clone(),
            rpc_url: self.global.rpc_url.clone(),
            requested_network: self.global.network.clone(),
            private_key: self.global.private_key.clone(),
            output_format: self.resolve_output_format(),
            verbosity: self.global.verbosity(),
        })
        .await?;
        let cmd_name = command_name(&command);
        track_command(&ctx.analytics, cmd_name);

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

        track_result(&ctx.analytics, cmd_name, &result);

        ctx.flush_analytics().await;

        result
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
