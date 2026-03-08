//! Top-level command dispatch and analytics tracking.

use anyhow::Result;

use super::commands::{completions, services, sessions};
use super::{Cli, Commands};
use tempo_common::cli::dispatch::{track_command, track_result};
use tempo_common::context::ContextArgs;
use tempo_common::runtime;

use crate::cli::args::{ServicesCommands, SessionCommands};

impl Cli {
    /// Application entry point: build context, dispatch command, flush analytics.
    pub(crate) async fn run(mut self) -> Result<()> {
        runtime::init_tracing(
            self.global.silent,
            self.global.verbose,
            &["tempo_mpp", "mpp"],
        );
        runtime::init_color_support(self.global.color);

        let command = match self.command.take() {
            Some(c) => c,
            None => {
                use clap::CommandFactory;
                return Self::command().print_help().map_err(Into::into);
            }
        };

        let ctx = tempo_common::context::Context::build(ContextArgs {
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

        track_result(&ctx.analytics, cmd_name, &result);

        ctx.flush_analytics().await;

        result
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
