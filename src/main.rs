//! presto — a command-line HTTP client with automatic payment support.
#![forbid(unsafe_code)]
#![deny(warnings)]
//!
//! Presto works like curl/wget but handles HTTP 402 (Payment Required)
//! responses automatically using the [Machine Payments Protocol (MPP)](https://mpp.dev).
//!
//! # Payment flow
//!
//! 1. Send the initial HTTP request
//! 2. If the server responds with 402, parse the `WWW-Authenticate` header
//! 3. Construct and submit a payment via the user's configured wallet
//! 4. Retry the request with a payment credential
//!
//! # Payment intents
//!
//! - **Charge** — one-shot payment settled on-chain per request
//! - **Session** — opens a payment channel on-chain, then exchanges
//!   off-chain vouchers for each subsequent request or SSE token,
//!   settling when the session is closed
//!
//! # Security
//!
//! - Server-controlled text is sanitized before terminal output to
//!   prevent ANSI escape injection (OSC 8 breakout, cursor manipulation)
//! - Redirect targets are validated against an allow-list to prevent
//!   payment credential leakage to unintended hosts
//! - Private keys are stored in the OS keychain (macOS Keychain) or
//!   in a mode-0600 file, and wrapped in [`zeroize::Zeroizing`] in memory

mod analytics;
mod cli;
mod config;
mod error;
mod http;
mod network;
mod payment;
mod services;
mod util;
mod wallet;

use anyhow::Result;
use clap::{CommandFactory, Parser};
use clap_complete::{generate, shells};
use cli::exit_codes::ExitCode;
use cli::{
    Cli, ColorMode, Commands, KeyCommands, ServicesCommands, SessionCommands, Shell, WalletCommands,
};
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
        // Attempt to resolve the desired output format to decide error rendering.
        let output_format = resolve_output_format_for_error();

        match output_format {
            Some(config::OutputFormat::Json) => {
                // Print structured JSON error to stdout only; logs remain on stderr via tracing.
                let json = cli::output::render_error_json(&e);
                println!("{}", json);
            }
            _ => {
                eprintln!("Error: {e:#}");
            }
        }

        ExitCode::from(&e).exit();
    }
}

async fn run() -> Result<()> {
    let mut cli = parse_cli();

    init_tracing(&cli);

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
            // Help requests pass through immediately
            if matches!(original_err.kind(), clap::error::ErrorKind::DisplayHelp) {
                original_err.exit()
            }

            // Version: check for JSON flag in raw args before exiting
            if matches!(original_err.kind(), clap::error::ErrorKind::DisplayVersion) {
                let args: Vec<String> = std::env::args().collect();
                if args.iter().any(|a| a == "-j" || a == "--json-output") {
                    print_version_json();
                    std::process::exit(0);
                }
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
            match Cli::try_parse_from(with_query) {
                Ok(cli) => {
                    // Re-parse succeeded. Check if the URL looks like a
                    // mistyped command (no scheme, no dots, no localhost).
                    // This catches `presto foo` and gives a clean error.
                    if let Some(Commands::Query(ref q)) = cli.command {
                        let url = q.url.clone();
                        if !url.contains("://") && !url.contains("localhost") && !url.contains('.')
                        {
                            // Single-word non-command: still an error
                            if !url.contains(' ') {
                                eprintln!("error: '{url}' is not a presto command. See 'presto --help' for a list of available commands.");
                                ExitCode::InvalidUsage.exit();
                            }

                            // Multi-word string: treat as a prompt → forward to native agent
                            exec_prompt(&url);
                        }
                    }
                    cli
                }
                Err(_) => original_err.exit(),
            }
        }
    }
}

/// Forward a natural-language prompt to the user's native AI agent (e.g. `claude`).
fn exec_prompt(prompt: &str) -> ! {
    let status = std::process::Command::new("claude").arg(prompt).status();

    match status {
        Ok(s) => std::process::exit(s.code().unwrap_or(1)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("error: 'claude' CLI not found. Install it: https://docs.anthropic.com/en/docs/claude-cli");
            ExitCode::GeneralError.exit();
        }
        Err(e) => {
            eprintln!("error: failed to run claude: {e}");
            ExitCode::GeneralError.exit();
        }
    }
}

/// Print version information as structured JSON and exit.
fn print_version_json() {
    let json = serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "git_commit": env!("PRESTO_GIT_SHA"),
        "build_date": env!("PRESTO_BUILD_DATE"),
        "profile": env!("PRESTO_BUILD_PROFILE"),
    });
    println!("{}", serde_json::to_string_pretty(&json).unwrap());
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
            .map_err(|_| anyhow::anyhow!(error::PrestoError::UnknownNetwork(name.to_string())))?;
    }
    Ok(())
}

/// Handle CLI subcommands
async fn handle_command(cli: Cli, command: Commands) -> Result<()> {
    if let Some(ref pk) = cli.private_key {
        wallet::credentials::set_credentials_override(pk.clone());
    }

    if let Some(ref network) = cli.network {
        validate_network_flag(network)?;
    }

    let config = load_config_with_overrides(cli.config.as_ref()).unwrap_or_default();
    let analytics = Analytics::new(cli.network.as_deref(), Some(&config)).await;

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
            Commands::Wallets { .. } => "wallets",
            Commands::Sessions { .. } => "sessions",
            Commands::Whoami | Commands::Balance => "whoami",
            Commands::Keys { .. } => "keys",
            Commands::Services { .. } => "services",
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
                                    error: analytics::sanitize_error(&err_str),
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

        Commands::Sessions { command } => {
            let config = load_config_with_overrides(cli.config.as_ref())?;
            let output_format = cli.resolve_output_format(&config);

            if let Some(subcommand) = command {
                let show_output = cli.should_show_output();
                match subcommand {
                    SessionCommands::List {
                        all,
                        orphaned,
                        closed,
                        network,
                    } => {
                        let net = network.as_deref().or(cli.network.as_deref());
                        cli::session::list_sessions(
                            &config,
                            output_format,
                            all,
                            orphaned,
                            closed,
                            net,
                        )
                        .await
                    }
                    SessionCommands::Close {
                        url,
                        all,
                        orphaned,
                        closed,
                    } => {
                        cli::session::close_sessions(
                            &config,
                            url,
                            all,
                            orphaned,
                            closed,
                            output_format,
                            show_output,
                            cli.network.as_deref(),
                        )
                        .await
                    }
                }
            } else {
                if let Some(session_cmd) = Cli::command().find_subcommand_mut("sessions") {
                    session_cmd.print_help()?;
                } else {
                    Cli::command().print_help()?;
                }
                Ok(())
            }
        }

        Commands::Wallets { command } => {
            if let Some(subcommand) = command {
                match subcommand {
                    WalletCommands::List => {
                        let config =
                            load_config_with_overrides(cli.config.as_ref()).unwrap_or_default();
                        let output_format = cli.resolve_output_format(&config);
                        cli::auth::show_wallet_list(output_format).await
                    }
                    WalletCommands::Create => {
                        let wallet_addr =
                            cli::local_wallet::create_local_wallet(cli.network.as_deref())?;
                        let config =
                            load_config_with_overrides(cli.config.as_ref()).unwrap_or_default();
                        let output_format = cli.resolve_output_format(&config);
                        let network = cli.network.as_deref();
                        cli::auth::show_whoami(&config, output_format, network, Some(&wallet_addr))
                            .await
                    }
                    WalletCommands::Fund { address, no_wait } => {
                        let config = load_config_with_overrides(cli.config.as_ref())?;
                        let output_format = cli.resolve_output_format(&config);
                        cli::fund::run_fund(
                            &config,
                            output_format,
                            cli.network.as_deref(),
                            address,
                            no_wait,
                        )
                        .await
                    }
                }
            } else {
                if let Some(wallet_cmd) = Cli::command().find_subcommand_mut("wallets") {
                    wallet_cmd.print_help()?;
                } else {
                    Cli::command().print_help()?;
                }
                Ok(())
            }
        }

        Commands::Whoami | Commands::Balance => {
            let config = load_config_with_overrides(cli.config.as_ref())?;
            let network = cli.network.as_deref();
            let output_format = cli.resolve_output_format(&config);

            if let Some(ref a) = analytics {
                a.track(analytics::Event::WhoamiViewed, analytics::EmptyPayload);
            }
            cli::auth::show_whoami(&config, output_format, network, None).await
        }

        Commands::Keys { command } => {
            let config = load_config_with_overrides(cli.config.as_ref())?;
            let network = cli.network.as_deref();
            let output_format = cli.resolve_output_format(&config);
            match command {
                Some(KeyCommands::List) => {
                    cli::keys::show_keys(&config, output_format, network).await
                }
                Some(KeyCommands::Create { wallet }) => {
                    cli::local_wallet::create_access_key(wallet.as_deref())?;
                    cli::auth::show_whoami(&config, output_format, network, None).await
                }
                Some(KeyCommands::Clean { yes }) => cli::keys::run_key_clean(yes),
                None => {
                    if let Some(key_cmd) = Cli::command().find_subcommand_mut("keys") {
                        key_cmd.print_help()?;
                    } else {
                        Cli::command().print_help()?;
                    }
                    Ok(())
                }
            }
        }

        Commands::Services {
            command,
            service_id,
            category,
            search,
        } => {
            let config = load_config_with_overrides(cli.config.as_ref()).unwrap_or_default();
            let output_format = cli.resolve_output_format(&config);
            match command {
                Some(ServicesCommands::Info { service_id }) => {
                    cli::services::show_service_info(output_format, &service_id).await
                }
                Some(ServicesCommands::List) => {
                    cli::services::list_services(
                        output_format,
                        category.as_deref(),
                        search.as_deref(),
                    )
                    .await
                }
                None if service_id.is_some() => {
                    cli::services::show_service_info(output_format, service_id.as_deref().unwrap())
                        .await
                }
                None => {
                    cli::services::list_services(
                        output_format,
                        category.as_deref(),
                        search.as_deref(),
                    )
                    .await
                }
            }
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

fn init_tracing(cli: &Cli) {
    use tracing_subscriber::EnvFilter;

    // Quiet mode (-q) is absolute: override any RUST_LOG with "off"
    let filter = if cli.silent {
        EnvFilter::new("off")
    } else if let Ok(env) = std::env::var("RUST_LOG") {
        EnvFilter::new(env)
    } else {
        // Map verbosity count to tracing level
        let level = match cli.verbose {
            0 => "warn",
            1 => "info",
            2 => "debug",
            _ => "trace",
        };
        EnvFilter::new(level)
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

/// Best-effort resolution of output format for error rendering.
///
/// Parses CLI and loads config to determine the resolved `OutputFormat`.
/// Returns `None` if parsing fails; defaults to text in that case.
fn resolve_output_format_for_error() -> Option<config::OutputFormat> {
    // Try to parse CLI normally first
    if let Ok(cli) = Cli::try_parse() {
        let cfg = load_config_with_overrides(cli.config.as_ref()).unwrap_or_default();
        return Some(cli.resolve_output_format(&cfg));
    }

    // If normal parsing failed, try the same fallback as parse_cli(): insert "query"
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        let mut with_query = vec![args[0].clone(), "query".to_string()];
        with_query.extend(args[1..].iter().cloned());
        if let Ok(cli) = Cli::try_parse_from(with_query) {
            let cfg = load_config_with_overrides(cli.config.as_ref()).unwrap_or_default();
            return Some(cli.resolve_output_format(&cfg));
        }
    }

    None
}
