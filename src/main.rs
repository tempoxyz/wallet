//!  tempo-wallet— a command-line HTTP client with automatic payment support.
#![forbid(unsafe_code)]
#![deny(warnings)]
//!
//!  Tempo Walletworks like curl/wget but handles HTTP 402 (Payment Required)
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

/// Entry point for the  tempo-walletCLI.
///
///  Tempo Walletis a command-line HTTP client (like curl/wget) that automatically
/// handles paid APIs. When a server responds with HTTP 402 Payment Required,
///  tempo-walletdetects the payment details from the `WWW-Authenticate` header,
/// submits a transaction through the user's configured wallet using the
/// Machine Payment Protocol (MPP), and retries the request with a payment
/// receipt — supporting both one-shot charges and persistent sessions.
///
/// This function parses CLI arguments, dispatches to the appropriate
/// subcommand (query, session management, login/logout, whoami, or shell
/// completions), and installs a Ctrl-C handler for graceful shutdown.
#[tokio::main]
async fn main() {
    let mut cli = parse_cli();

    init_tracing(&cli);
    init_color_support(&cli);

    let output_format = cli.resolve_output_format();

    let result = async {
        let mut config = load_config_with_overrides(cli.config.as_ref())?;
        config.check_for_updates().await;

        match cli.command.take() {
            Some(command) => handle_command(cli, command, config).await,
            None => Cli::command().print_help().map_err(Into::into),
        }
    }
    .await;

    if let Err(e) = result {
        match output_format {
            config::OutputFormat::Json | config::OutputFormat::Toon => {
                let output = cli::output::render_error_structured(&e, output_format);
                println!("{}", output);
            }
            _ => {
                eprintln!("Error: {e:#}");
            }
        }
        ExitCode::from(&e).exit();
    }
}

/// Parse CLI args, treating a bare URL as an implicit `query` subcommand.
///
/// This allows ` tempo-wallethttps://example.com` as a shorthand for
/// ` tempo-walletquery https://example.com`, making the primary use case
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
            // This handles cases like ` tempo-wallethttps://example.com` or
            // ` tempo-wallet-X POST --json '{}' https://example.com`.
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
                    // This catches ` tempo-walletfoo` and gives a clean error.
                    if let Some(Commands::Query(ref q)) = cli.command {
                        let url = q.url.clone();
                        if !url.contains("://") && !url.contains("localhost") && !url.contains('.')
                        {
                            // Single-word non-command: still an error
                            if !url.contains(' ') {
                                eprintln!("error: '{url}' is not a  tempo-walletcommand. See ' tempo-wallet--help' for a list of available commands.");
                                ExitCode::InvalidUsage.exit();
                            }
                        }
                    }
                    cli
                }
                Err(_) => original_err.exit(),
            }
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
async fn handle_command(cli: Cli, command: Commands, config: config::Config) -> Result<()> {
    if let Some(ref pk) = cli.private_key {
        wallet::credentials::set_credentials_override(pk.clone());
    }

    if let Some(ref network) = cli.network {
        validate_network_flag(network)?;
    }

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
            Commands::Update => "update",
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
        Commands::Query(query) => {
            let mut query_config = config.clone();
            if let Some(ref rpc_url) = query.rpc_url {
                query_config.set_rpc_override(rpc_url.clone());
            }
            cli::query::make_request(cli, *query, analytics.clone(), query_config).await
        }

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
            let output_format = cli.resolve_output_format();
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
            let output_format = cli.resolve_output_format();

            if let Some(subcommand) = command {
                let show_output = cli.should_show_output();
                match subcommand {
                    SessionCommands::List { state, network } => {
                        let net = network.as_deref().or(cli.network.as_deref());
                        let mut selected: Vec<cli::session::ListSessionState> = Vec::new();
                        if state.is_empty() {
                            // default will be applied in list_sessions
                        } else if state
                            .iter()
                            .any(|s| matches!(s, crate::cli::SessionStateArg::All))
                        {
                            selected = vec![
                                cli::session::ListSessionState::Active,
                                cli::session::ListSessionState::Closing,
                                cli::session::ListSessionState::Finalizable,
                                cli::session::ListSessionState::Orphaned,
                            ];
                        } else {
                            for s in state {
                                let m = match s {
                                    crate::cli::SessionStateArg::Active => {
                                        cli::session::ListSessionState::Active
                                    }
                                    crate::cli::SessionStateArg::Closing => {
                                        cli::session::ListSessionState::Closing
                                    }
                                    crate::cli::SessionStateArg::Finalizable => {
                                        cli::session::ListSessionState::Finalizable
                                    }
                                    crate::cli::SessionStateArg::Orphaned => {
                                        cli::session::ListSessionState::Orphaned
                                    }
                                    crate::cli::SessionStateArg::All => continue,
                                };
                                selected.push(m);
                            }
                        }
                        cli::session::list_sessions(&config, output_format, &selected, net).await
                    }
                    SessionCommands::Info { target, network } => {
                        let net = network.as_deref().or(cli.network.as_deref());
                        cli::session::show_session_info(&config, output_format, &target, net).await
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
                            analytics.as_ref(),
                        )
                        .await
                    }
                    SessionCommands::Recover { origin } => {
                        cli::session::recover_session(&config, output_format, &origin).await
                    }
                    SessionCommands::Sync => {
                        cli::session::sync_sessions(&config, output_format, show_output).await
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
                        let output_format = cli.resolve_output_format();
                        cli::auth::show_wallet_list(output_format).await
                    }
                    WalletCommands::Create => {
                        let wallet_addr =
                            cli::local_wallet::create_local_wallet(cli.network.as_deref())?;
                        let output_format = cli.resolve_output_format();
                        let network = cli.network.as_deref();
                        cli::auth::show_whoami(&config, output_format, network, Some(&wallet_addr))
                            .await
                    }
                    WalletCommands::Fund { address, no_wait } => {
                        let output_format = cli.resolve_output_format();
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
            let network = cli.network.as_deref();
            let output_format = cli.resolve_output_format();

            if let Some(ref a) = analytics {
                a.track(analytics::Event::WhoamiViewed, analytics::EmptyPayload);
            }
            cli::auth::show_whoami(&config, output_format, network, None).await
        }

        Commands::Keys { command } => {
            let network = cli.network.as_deref();
            let output_format = cli.resolve_output_format();
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
            let output_format = cli.resolve_output_format();
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

        Commands::Update => run_self_update(),
    };

    if let Some(ref a) = analytics {
        a.flush().await;
    }

    result
}

// ==================== Simple Commands ====================

const INSTALL_SCRIPT_URL: &str = "https://presto-binaries.tempo.xyz/install.sh";

/// Download and run the install script to update to the latest version.
fn run_self_update() -> Result<()> {
    eprintln!("Updating  tempo-walletto the latest version...\n");

    let status = std::process::Command::new("bash")
        .arg("-c")
        .arg(format!("curl -fsSL {INSTALL_SCRIPT_URL} | bash"))
        .status()
        .map_err(|e| anyhow::anyhow!("failed to run update script: {e}"))?;

    if !status.success() {
        anyhow::bail!("update failed (exit code {})", status.code().unwrap_or(1));
    }

    Ok(())
}

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
        // Map verbosity count to tracing level for the  tempo-walletcrate only;
        // keep all other crates at warn to avoid noise from hyper/reqwest/alloy.
        let filter_str = match cli.verbose {
            0 => "warn",
            1 => "warn,presto=info",
            2 => "warn,presto=debug,mpp=debug",
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
