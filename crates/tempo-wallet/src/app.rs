//! Application entry point: build context, dispatch command, flush analytics.

use crate::{
    args::{Cli, Commands, ServicesCommands, SessionCommands},
    commands::{
        completions, debug, fund, keys, login, logout, refresh, services, sessions, transfer,
        whoami,
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
                Commands::Login { no_browser } => login::run(&ctx, no_browser).await,
                Commands::Refresh => refresh::run(&ctx).await,
                Commands::Logout { yes } => logout::run(&ctx, yes),
                Commands::Completions { shell } => completions::run(&ctx, shell),
                Commands::Fund {
                    address,
                    no_browser,
                } => fund::run(&ctx, address, no_browser).await,
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
                Commands::Debug {
                    server_fn_id,
                    server_fn_data,
                } => {
                    if server_fn_id.is_none() && server_fn_data.is_none() {
                        debug::run(&ctx).await
                    } else {
                        debug::run_with_server_fn(&ctx, server_fn_id, server_fn_data).await
                    }
                }
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
        Commands::Login { .. } => "login",
        Commands::Refresh => "refresh",
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
        Commands::Debug { .. } => "debug",
        Commands::Services { command, .. } => match command {
            Some(ServicesCommands::List) => "services list",
            None => "services",
        },
    }
}
