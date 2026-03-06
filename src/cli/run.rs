//! Top-level command dispatch and analytics tracking.

use anyhow::Result;
use clap::ValueEnum;

use super::commands::*;
use super::{Cli, Commands, Context};
use crate::analytics::{self, Analytics};
use crate::cli::args::{KeyCommands, ServicesCommands, SessionCommands, WalletCommands};
use crate::util::sanitize_error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum ColorMode {
    Auto,
    Always,
    Never,
}

impl Cli {
    /// Application entry point: build context, dispatch command, flush analytics.
    pub(crate) async fn run(mut self) -> Result<()> {
        self.init_tracing();
        self.init_color_support();

        let command = match self.command.take() {
            Some(c) => c,
            None => {
                use clap::CommandFactory;
                return Self::command().print_help().map_err(Into::into);
            }
        };

        let ctx = Context::build(self).await?;
        let cmd_name = command_name(&command);
        track_command(&ctx.analytics, cmd_name);

        let result = match command {
            Commands::Query(q) => query::run(&ctx, *q).await,
            Commands::Login => login::run(&ctx).await,
            Commands::Logout { yes } => logout::run(&ctx, yes).await,
            Commands::Completions { shell } => completions::run(&ctx, shell),
            Commands::Sessions { command } => sessions::run(&ctx, command).await,
            Commands::Wallets { command } => wallets::run(&ctx, command).await,
            Commands::Whoami => whoami::run(&ctx).await,
            Commands::Keys { command } => keys::run(&ctx, command).await,
            Commands::Services {
                command,
                service_id,
                category,
                search,
            } => services::run(&ctx, command, service_id, category, search).await,
            Commands::Update => update::run(&ctx, false).await,
        };

        track_result(&ctx.analytics, cmd_name, &result);

        if let Some(ref a) = ctx.analytics {
            a.flush().await;
        }

        result
    }

    /// Initialize tracing subscriber based on CLI verbosity and environment.
    fn init_tracing(&self) {
        use tracing_subscriber::EnvFilter;

        // Silent mode (-s) is absolute: override any RUST_LOG with "off"
        let filter = if self.silent {
            EnvFilter::new("off")
        } else if let Ok(env) = std::env::var("RUST_LOG") {
            EnvFilter::new(env)
        } else {
            // Map verbosity count to tracing level for the tempo-wallet crate only;
            // keep all other crates at warn to avoid noise from hyper/reqwest/alloy.
            let filter_str = match self.verbose {
                0 => "warn",
                1 => "warn,tempo_wallet=info",
                2 => "warn,tempo_wallet=debug,mpp=debug",
                _ => {
                    "trace,hyper=warn,reqwest=warn,h2=warn,rustls=warn,tower=warn,mio=warn,polling=warn"
                }
            };
            EnvFilter::new(filter_str)
        };

        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(false)
            .with_writer(std::io::stderr)
            .without_time()
            .init();
    }

    /// Initialize color support based on user preference and NO_COLOR env var.
    fn init_color_support(&self) {
        use colored::control;
        use std::io::IsTerminal;

        let no_color_env = std::env::var("NO_COLOR").is_ok();

        match self.color {
            ColorMode::Always => control::set_override(true),
            ColorMode::Never => control::set_override(false),
            ColorMode::Auto => {
                if no_color_env || !std::io::stdout().is_terminal() {
                    control::set_override(false);
                }
            }
        }
    }
}

/// Track the initial command run event.
fn track_command(analytics: &Option<Analytics>, cmd_name: &str) {
    if let Some(ref a) = analytics {
        a.track(
            analytics::Event::CommandRun,
            analytics::CommandRunPayload {
                command: cmd_name.to_string(),
            },
        );
    }
}

/// Track command success or failure.
fn track_result(analytics: &Option<Analytics>, cmd_name: &str, result: &Result<()>) {
    let Some(ref a) = analytics else { return };
    match result {
        Ok(()) => {
            a.track(
                analytics::Event::CommandSuccess,
                analytics::CommandRunPayload {
                    command: cmd_name.to_string(),
                },
            );
        }
        Err(e) => {
            a.track(
                analytics::Event::CommandFailure,
                analytics::CommandFailurePayload {
                    command: cmd_name.to_string(),
                    error: sanitize_error(&e.to_string()),
                },
            );
        }
    }
}

/// Derive a short analytics-friendly name from a parsed command.
fn command_name(command: &Commands) -> &'static str {
    match command {
        Commands::Query(_) => "query",
        Commands::Login => "login",
        Commands::Logout { .. } => "logout",
        Commands::Completions { .. } => "completions",
        Commands::Wallets { command } => match command {
            Some(WalletCommands::List) => "wallets list",
            Some(WalletCommands::Create) => "wallets create",
            Some(WalletCommands::Fund { .. }) => "wallets fund",
            None => "wallets",
        },
        Commands::Sessions { command } => match command {
            Some(SessionCommands::List { .. }) => "sessions list",
            Some(SessionCommands::Info { .. }) => "sessions info",
            Some(SessionCommands::Close { .. }) => "sessions close",
            Some(SessionCommands::Recover { .. }) => "sessions recover",
            Some(SessionCommands::Sync) => "sessions sync",
            None => "sessions",
        },
        Commands::Whoami => "whoami",
        Commands::Keys { command } => match command {
            Some(KeyCommands::List) => "keys list",
            Some(KeyCommands::Create { .. }) => "keys create",
            Some(KeyCommands::Clean { .. }) => "keys clean",
            None => "keys",
        },
        Commands::Services { command, .. } => match command {
            Some(ServicesCommands::List) => "services list",
            Some(ServicesCommands::Info { .. }) => "services info",
            None => "services",
        },
        Commands::Update => "update",
    }
}
