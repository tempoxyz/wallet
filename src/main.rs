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
mod util;
mod wallet;

use anyhow::Result;
use clap::{CommandFactory, Parser};
use clap_complete::{generate, shells};
use cli::exit_codes::ExitCode;
use cli::{Cli, ColorMode, Commands, SessionCommands, Shell, WalletCommands};
use colored::control;

use analytics::Analytics;
use config::load_config_with_overrides;

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
/// subcommand (query, session management, login/logout, whoami, or shell
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
        eprintln!("Error: {e:#}");
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
            // Help/version requests should pass through immediately
            if matches!(
                original_err.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) {
                original_err.exit()
            }

            // If normal parsing failed, try again with "query" inserted.
            // This handles cases like `presto https://example.com` or
            // `presto -X POST --json '{}' https://example.com`.
            //
            // Skip the fallback if the first non-flag arg is a known
            // subcommand so we don't swallow its parse errors (e.g.,
            // missing required args) as an implicit query.
            let args: Vec<String> = std::env::args().collect();
            let mut subcommands: Vec<String> = Cli::command()
                .get_subcommands()
                .flat_map(|c| {
                    let mut names = vec![c.get_name().to_string()];
                    names.extend(c.get_all_aliases().map(String::from));
                    names
                })
                .collect();
            subcommands.push("help".to_string());
            let first_positional = args[1..]
                .iter()
                .find(|a| !a.starts_with('-'))
                .map(|s| s.as_str());

            if first_positional.is_some_and(|p| subcommands.iter().any(|s| s == p)) {
                original_err.exit()
            }

            let mut with_query = vec![args[0].clone(), "query".to_string()];
            with_query.extend(args[1..].iter().cloned());
            Cli::try_parse_from(with_query).unwrap_or_else(|_| original_err.exit())
        }
    }
}

/// Validate the --network flag value against known built-in networks.
///
/// This catches typos and invalid names early, before they cause silent
/// failures in login/logout/whoami where the network name is used as an
/// exact match to select wallet credentials.
fn validate_network_flag(network: &str) -> Result<()> {
    // Support comma-separated network lists (e.g. "tempo, tempo-moderato")
    for name in network.split(',').map(|s| s.trim()) {
        network::validate_network_name(name)
            .map_err(|msg| anyhow::anyhow!(error::PrestoError::UnknownNetwork(msg)))?;
    }
    Ok(())
}

/// Handle CLI subcommands
async fn handle_command(cli: Cli, command: Commands) -> Result<()> {
    if let Some(ref key) = cli.key {
        wallet::credentials::set_key_name_override(key.clone());
    }

    if let Some(ref pk) = cli.private_key {
        wallet::credentials::set_credentials_override(pk.clone());
    }

    if let Some(ref network) = cli.network {
        validate_network_flag(network)?;
    }

    let analytics = Analytics::new(cli.network.as_deref()).await;

    if let Some(ref a) = analytics {
        a.identify();

        let is_new_user = wallet::credentials::WalletCredentials::load()
            .ok()
            .is_none_or(|c| !c.has_wallet());

        let cmd_name = match &command {
            Commands::Query(_) => "query",
            Commands::Login => "login",
            Commands::Logout { .. } => "logout",
            Commands::Completions { .. } => "completions",
            Commands::Wallet { .. } => "wallet",
            Commands::Session { .. } => "session",
            Commands::Whoami | Commands::Balance => "whoami",
            Commands::Keys => "keys",
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
        Commands::Query(query) => cli::query::make_request(cli, *query, analytics.clone()).await,

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
            let config = load_config_with_overrides(cli.config.as_ref()).unwrap_or_default();
            let output_format = cli.resolve_output_format(&config);
            let result = cli::auth::run_login(network, analytics.clone(), output_format).await;
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
                        let is_login_timeout = err_str.contains("timed out")
                            || e.chain()
                                .find_map(|cause| cause.downcast_ref::<error::PrestoError>())
                                .is_some_and(|pe| matches!(pe, error::PrestoError::LoginExpired));

                        if is_login_timeout {
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
            let result = cli::auth::run_logout(yes).await;
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

        Commands::Session { command } => {
            if let Some(subcommand) = command {
                match subcommand {
                    SessionCommands::List => cli::session::list_sessions(),
                    SessionCommands::Close { url, all } => {
                        cli::session::close_sessions(url, all).await
                    }
                    SessionCommands::Recover { url } => {
                        let config = load_config_with_overrides(cli.config.as_ref())?;
                        cli::session::recover_session_cmd(&config, &url).await
                    }
                }
            } else {
                cli::session::list_sessions()
            }
        }

        Commands::Wallet { command } => {
            if let Some(subcommand) = command {
                match subcommand {
                    WalletCommands::Create { name, passkey } => {
                        if passkey {
                            let network = cli.network.as_deref();
                            let config =
                                load_config_with_overrides(cli.config.as_ref()).unwrap_or_default();
                            let output_format = cli.resolve_output_format(&config);
                            cli::auth::run_login(network, analytics.clone(), output_format).await
                        } else {
                            let name = name.as_deref().unwrap_or("local-default");
                            cli::wallet::create_local_wallet(name)?;
                            let config =
                                load_config_with_overrides(cli.config.as_ref()).unwrap_or_default();
                            let output_format = cli.resolve_output_format(&config);
                            let network = cli.network.as_deref();
                            cli::auth::show_whoami(&config, output_format, network)
                                .await
                                .map_err(Into::into)
                        }
                    }
                    WalletCommands::Import {
                        name,
                        private_key,
                        stdin_key,
                    } => {
                        let name = name.as_deref().unwrap_or("local-default");
                        cli::wallet::import_wallet(name, private_key, stdin_key)
                    }
                    WalletCommands::Delete {
                        name,
                        name_flag,
                        passkey,
                        yes,
                    } => {
                        if passkey {
                            cli::auth::run_logout(yes).await
                        } else {
                            let name = name
                                .or(name_flag)
                                .unwrap_or_else(|| "local-default".to_string());
                            cli::wallet::delete_wallet(&name, yes)
                        }
                    }
                }
            } else {
                Cli::command()
                    .find_subcommand_mut("wallet")
                    .expect("wallet subcommand exists")
                    .print_help()?;
                Ok(())
            }
        }

        Commands::Whoami | Commands::Balance => {
            let config = load_config_with_overrides(cli.config.as_ref())?;
            let network = cli.network.as_deref();
            let output_format = cli.resolve_output_format(&config);

            // Auto-login if no wallet is connected
            let creds = wallet::credentials::WalletCredentials::load()?;
            if !creds.has_wallet() {
                eprintln!("No wallet connected. Starting login...\n");
                if let Some(ref a) = analytics {
                    a.track(analytics::Event::WhoamiViewed, analytics::EmptyPayload);
                }
                // run_login already displays whoami after success
                return cli::auth::run_login(network, analytics.clone(), output_format).await;
            }

            if let Some(ref a) = analytics {
                a.track(analytics::Event::WhoamiViewed, analytics::EmptyPayload);
            }
            cli::auth::show_whoami(&config, output_format, network)
                .await
                .map_err(Into::into)
        }

        Commands::Keys => {
            let config = load_config_with_overrides(cli.config.as_ref())?;
            let network = cli.network.as_deref();
            let output_format = cli.resolve_output_format(&config);
            cli::auth::show_keys(&config, output_format, network)
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
