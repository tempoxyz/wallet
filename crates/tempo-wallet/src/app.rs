//! Application entry point: build context, dispatch command, flush analytics.

use crate::{
    args::{Cli, Commands, ServicesCommands, SessionCommands},
    commands::{
        completions, debug, fund, keys, login, logout, services, sessions, transfer, whoami,
    },
};
use tempo_common::error::TempoError;

/// Run the tempo-wallet application.
pub(crate) async fn run(mut cli: Cli) -> Result<(), TempoError> {
    let command = if let Some(c) = cli.command.take() {
        c
    } else {
        use clap::CommandFactory;
        return Cli::command().print_help().map_err(Into::into);
    };

    tempo_common::cli::run_cli(
        &cli.global,
        &["tempo_wallet"],
        "tempo-wallet",
        |ctx| async move {
            let cmd_name = command_name(&command);
            let result = match command {
                Commands::Login => login::run(&ctx).await,
                Commands::Logout { yes } => logout::run(&ctx, yes),
                Commands::Completions { shell } => completions::run(&ctx, shell),
                Commands::Fund {
                    address,
                    chain,
                    token,
                    list_chains,
                    no_wait,
                    dry_run,
                } => fund::run(&ctx, address, chain, token, list_chains, no_wait, dry_run).await,
                Commands::Whoami => whoami::run(&ctx).await,
                Commands::Keys => keys::run(&ctx).await,
                Commands::Sessions { command } => {
                    sessions::run(
                        &ctx,
                        command.unwrap_or(SessionCommands::List {
                            orphaned: false,
                            all: false,
                        }),
                    )
                    .await
                }
                Commands::Transfer {
                    amount,
                    token,
                    to,
                    fee_token,
                    dry_run,
                } => transfer::run(&ctx, amount, token, to, fee_token, dry_run).await,
                Commands::Debug => debug::run(&ctx).await,
                Commands::Services {
                    service_id, search, ..
                } => services::run(&ctx, services::ServicesArgs { service_id, search }).await,
            };
            (cmd_name, result)
        },
    )
    .await
}

/// Derive a short analytics-friendly name from a parsed command.
const fn command_name(command: &Commands) -> &'static str {
    match command {
        Commands::Login => "login",
        Commands::Logout { .. } => "logout",
        Commands::Completions { .. } => "completions",
        Commands::Fund { .. } => "fund",
        Commands::Whoami => "whoami",
        Commands::Keys => "keys",
        Commands::Sessions { command } => match command {
            Some(SessionCommands::List { .. }) | None => "sessions list",
            Some(SessionCommands::Close { .. }) => "sessions close",
            Some(SessionCommands::Sync { .. }) => "sessions sync",
        },
        Commands::Transfer { .. } => "transfer",
        Commands::Debug => "debug",
        Commands::Services { command, .. } => match command {
            Some(ServicesCommands::List) => "services list",
            None => "services",
        },
    }
}
