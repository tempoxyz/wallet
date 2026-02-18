//! presto CLI - A command-line HTTP client with automatic payment support.
//!
//! Works like curl/wget but handles HTTP 402 (Payment Required) responses
//! automatically using the Machine Payment Protocol (MPP). When a server
//! demands payment, presto detects the payment protocol from the
//! WWW-Authenticate header, constructs a transaction via the user's
//! configured wallet, and retries the request with a payment receipt —
//! supporting both one-shot charges and persistent sessions.

mod analytics;
mod cli;
mod config;
mod error;
mod http;
mod network;
mod payment;
mod request;
mod util;
mod wallet;

use anyhow::Result;
use clap::{CommandFactory, Parser};
use clap_complete::{generate, shells};
use cli::exit_codes::ExitCode;
use cli::{Cli, ColorMode, Commands, SessionCommands, Shell};
use colored::control;

use analytics::Analytics;
use config::load_config;

/// Entry point for the presto CLI.
///
/// Presto is a command-line HTTP client (like curl/wget) that automatically
/// handles paid APIs. When a server responds with HTTP 402 Payment Required,
/// presto detects the payment details from the `WWW-Authenticate` header,
/// submits a transaction through the user's configured wallet using the
/// Machine Payment Protocol (MPP), and retries the request with a payment
/// receipt — supporting both one-shot charges and persistent sessions.
///
/// This function parses CLI arguments, dispatches to the appropriate
/// subcommand (query, session management, login/logout, balance, or shell
/// completions), and installs a Ctrl-C handler for graceful shutdown.
#[tokio::main]
async fn main() {
    // Set up signal handling for graceful shutdown
    let result = tokio::select! {
        result = run() => result,
        _ = tokio::signal::ctrl_c() => {
            eprintln!("\nInterrupted");
            ExitCode::Interrupted.exit();
        }
    };

    if let Err(e) = result {
        eprintln!("{}", cli::errors::format_error_with_suggestion(&e));
        ExitCode::from(&e).exit();
    }
}

async fn run() -> Result<()> {
    let mut cli = parse_cli();

    init_tracing(cli.verbosity);

    // Initialize color support based on user preference and NO_COLOR env var
    init_color_support(&cli);

    // Handle subcommands
    let command = cli.command.take();
    if let Some(command) = command {
        return handle_command(cli, command).await;
    }

    // No subcommand — show help
    Cli::command().print_help()?;
    Ok(())
}

/// Parse CLI args, treating a bare URL as an implicit `query` subcommand.
///
/// This allows `presto https://example.com` as a shorthand for
/// `presto query https://example.com`, making the primary use case
/// as frictionless as curl/wget.
fn parse_cli() -> Cli {
    match Cli::try_parse() {
        Ok(cli) => cli,
        Err(original_err) => {
            // If normal parsing failed, try again with "query" inserted.
            // This handles cases like `presto https://example.com` or
            // `presto -X POST --json '{}' https://example.com`.
            let args: Vec<String> = std::env::args().collect();
            let mut with_query = vec![args[0].clone(), "query".to_string()];
            with_query.extend(args[1..].iter().cloned());
            Cli::try_parse_from(with_query).unwrap_or_else(|_| original_err.exit())
        }
    }
}

/// Handle CLI subcommands
async fn handle_command(cli: Cli, command: Commands) -> Result<()> {
    let analytics = Analytics::new(cli.network.as_deref()).await;

    if let Some(ref a) = analytics {
        a.identify();

        let is_new_user = wallet::credentials::WalletCredentials::load()
            .ok()
            .and_then(|c| c.active_wallet().cloned())
            .is_none();

        let cmd_name = match &command {
            Commands::Query(_) => "query",
            Commands::Login => "login",
            Commands::Logout { .. } => "logout",
            Commands::Completions { .. } => "completions",
            Commands::Balance { .. } => "balance",
            Commands::Session { .. } => "session",
            Commands::Whoami => "whoami",
        };
        a.track(
            analytics::Event::SessionStarted,
            analytics::SessionStartedPayload {
                is_new_user,
                command: cmd_name.to_string(),
            },
        );
        a.track(
            analytics::Event::CommandRun,
            analytics::CommandRunPayload {
                command: cmd_name.to_string(),
            },
        );
    }

    let result = match command {
        Commands::Query(query) => request::make_request(cli, *query, analytics.clone()).await,

        Commands::Login => {
            let network = cli.network.as_deref();
            if let Some(ref a) = analytics {
                a.track(
                    analytics::Event::LoginStarted,
                    analytics::LoginPayload {
                        network: network.unwrap_or_default().to_string(),
                    },
                );
            }
            let result = cli::commands::login::run_login(network, analytics.clone()).await;
            if let Some(ref a) = analytics {
                let net = network.unwrap_or_default().to_string();
                match &result {
                    Ok(()) => {
                        a.track(
                            analytics::Event::LoginSuccess,
                            analytics::LoginPayload { network: net },
                        );
                    }
                    Err(e) => {
                        let err_str = e.to_string();
                        if err_str.contains("timed out") {
                            a.track(
                                analytics::Event::LoginTimeout,
                                analytics::LoginTimeoutPayload { network: net },
                            );
                        } else {
                            a.track(
                                analytics::Event::LoginFailure,
                                analytics::LoginFailurePayload {
                                    network: net,
                                    error: err_str,
                                },
                            );
                        }
                    }
                }
            }
            result
        }

        Commands::Logout { yes } => {
            let network = cli.network.as_deref();
            let result = cli::commands::logout::run_logout(yes, network).await;
            if let Some(ref a) = analytics {
                if result.is_ok() {
                    a.track(analytics::Event::Logout, analytics::EmptyPayload);
                }
            }
            result
        }

        Commands::Completions { shell } => {
            if let Some(shell) = shell {
                generate_completions(shell)
            } else {
                println!("Supported shells: bash, zsh, fish, powershell");
                Ok(())
            }
        }

        Commands::Balance { address } => {
            let config = load_config(cli.config.as_ref())?;
            if let Some(ref a) = analytics {
                a.track(
                    analytics::Event::BalanceChecked,
                    analytics::BalanceCheckedPayload {
                        network: cli.network.clone().unwrap_or_default(),
                    },
                );
            }
            cli::commands::balance::balance_command(
                &config,
                address,
                cli.network.clone(),
                cli.output_format,
            )
            .await
        }

        Commands::Session { command } => {
            if let Some(subcommand) = command {
                match subcommand {
                    SessionCommands::List => cli::commands::session::list_sessions(),
                    SessionCommands::Close { url, all } => {
                        cli::commands::session::close_sessions(url, all).await
                    }
                }
            } else {
                cli::commands::session::list_sessions()
            }
        }

        Commands::Whoami => {
            let network = cli.network.as_deref();
            if let Some(ref a) = analytics {
                a.track(analytics::Event::WhoamiViewed, analytics::EmptyPayload);
            }
            cli::commands::whoami::show_whoami(cli.output_format, network)
                .await
                .map_err(Into::into)
        }
    };

    if let Some(ref a) = analytics {
        a.flush().await;
    }

    result
}

// ==================== Simple Commands ====================

/// Generate shell completions
fn generate_completions(shell: Shell) -> Result<()> {
    let mut cmd = Cli::command();
    let bin_name = cmd.get_name().to_string();

    match shell {
        Shell::Bash => generate(shells::Bash, &mut cmd, bin_name, &mut std::io::stdout()),
        Shell::Zsh => generate(shells::Zsh, &mut cmd, bin_name, &mut std::io::stdout()),
        Shell::Fish => generate(shells::Fish, &mut cmd, bin_name, &mut std::io::stdout()),
        Shell::PowerShell => generate(
            shells::PowerShell,
            &mut cmd,
            bin_name,
            &mut std::io::stdout(),
        ),
    }

    Ok(())
}

fn init_tracing(verbosity: u8) {
    use tracing_subscriber::EnvFilter;

    let filter = match verbosity {
        0 => EnvFilter::new("warn"),
        1 => EnvFilter::new("info"),
        _ => EnvFilter::new("debug"),
    };

    let filter = if let Ok(env) = std::env::var("RUST_LOG") {
        EnvFilter::new(env)
    } else {
        filter
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(std::io::stderr)
        .without_time()
        .init();
}

/// Initialize color support based on user preference and NO_COLOR env var
fn init_color_support(cli: &Cli) {
    use std::io::IsTerminal;
    let no_color_env = std::env::var("NO_COLOR").is_ok();

    match cli.color {
        ColorMode::Always => control::set_override(true),
        ColorMode::Never => control::set_override(false),
        ColorMode::Auto => {
            if no_color_env || !std::io::stdout().is_terminal() {
                control::set_override(false);
            }
        }
    }
}
