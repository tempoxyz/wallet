//! Application entry point: build context, dispatch command, flush analytics.

use anyhow::Result;

use crate::args::{Cli, Commands, KeyCommands, ServicesCommands, SessionCommands};
use crate::commands::{
    completions, keys, login, logout, services, sessions, sign, wallets, whoami,
};

/// Run the tempo-wallet application.
pub(crate) async fn run(mut cli: Cli) -> Result<()> {
    let command = match cli.command.take() {
        Some(c) => c,
        None => {
            use clap::CommandFactory;
            return Cli::command().print_help().map_err(Into::into);
        }
    };

    tempo_common::cli::run_cli(&cli.global, &["tempo_wallet"], |ctx| async move {
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
            Commands::Keys { command } => {
                keys::run(&ctx, command.unwrap_or(KeyCommands::List)).await
            }
            Commands::Sessions { command } => {
                sessions::run(&ctx, command.unwrap_or(SessionCommands::List { state: vec![] }))
                    .await
            }
            Commands::Sign { challenge, dry_run } => sign::run(&ctx, challenge, dry_run).await,
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
        Commands::Login => "login",
        Commands::Logout { .. } => "logout",
        Commands::Completions { .. } => "completions",
        Commands::List => "list",
        Commands::Create => "create",
        Commands::Fund { .. } => "fund",
        Commands::Whoami => "whoami",
        Commands::Keys { command } => match command {
            Some(KeyCommands::List) | None => "keys list",
            Some(KeyCommands::Create { .. }) => "keys create",
            Some(KeyCommands::Clean { .. }) => "keys clean",
        },
        Commands::Sessions { command } => match command {
            Some(SessionCommands::List { .. }) | None => "sessions list",
            Some(SessionCommands::Info { .. }) => "sessions info",
            Some(SessionCommands::Close { .. }) => "sessions close",
            Some(SessionCommands::Sync { .. }) => "sessions sync",
        },
        Commands::Sign { .. } => "sign",
        Commands::Services { command, .. } => match command {
            Some(ServicesCommands::List) => "services list",
            Some(ServicesCommands::Info { .. }) => "services info",
            None => "services",
        },
    }
}
