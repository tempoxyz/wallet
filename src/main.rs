//! presto CLI - A wget-like tool for payment-enabled HTTP requests

// Library modules (from lib.rs)
mod analytics;
mod config;
mod error;
mod http;
mod network;
mod payment;
mod services;
mod util;
mod wallet;

// CLI modules
mod cli;

use mpp::PaymentProtocol;

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser};
use clap_complete::{generate, shells};
use cli::exit_codes::ExitCode;
use cli::{Cli, ColorMode, Commands, NetworkCommands, QueryArgs, Shell, WalletCommands};
use colored::control;

use analytics::Analytics;
use cli::output::handle_regular_response;
use config::{load_config, load_config_with_overrides};
use http::request::RequestContext;
use payment::web_payment::handle_web_payment_request;

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
    let mut cli = Cli::parse();

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
            Commands::Networks { .. } => "networks",
            Commands::Inspect { .. } => "inspect",
            Commands::Wallet { .. } => "wallet",

            Commands::Services { .. } => "services",
            Commands::Keys { .. } => "keys",
            Commands::Whoami { .. } => "whoami",
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
        Commands::Query(query) => make_request(cli, *query, analytics.clone()).await,

        Commands::Login => {
            let network = cli.network.as_deref();
            if let Some(ref a) = analytics {
                a.track(
                    analytics::Event::LoginStarted,
                    analytics::LoginPayload {
                        network: network.unwrap_or("tempo-moderato").to_string(),
                    },
                );
            }
            let result = cli::commands::login::run_login(network, analytics.clone()).await;
            if let Some(ref a) = analytics {
                match &result {
                    Ok(()) => {
                        a.track(
                            analytics::Event::LoginSuccess,
                            analytics::LoginPayload {
                                network: network.unwrap_or("tempo-moderato").to_string(),
                            },
                        );
                    }
                    Err(e) => {
                        a.track(
                            analytics::Event::LoginFailure,
                            analytics::LoginFailurePayload {
                                network: network.unwrap_or("tempo-moderato").to_string(),
                                error: e.to_string(),
                            },
                        );
                    }
                }
            }
            result
        }

        Commands::Logout { yes } => {
            let network = cli.network.as_deref();
            if let Some(ref a) = analytics {
                a.track(analytics::Event::Logout, analytics::EmptyPayload);
            }
            cli::commands::logout::run_logout(yes, network).await
        }

        Commands::Completions { shell } => {
            if let Some(shell) = shell {
                generate_completions(shell)
            } else {
                println!("Supported shells: bash, zsh, fish, power-shell");
                Ok(())
            }
        }

        Commands::Balance {
            address,
            output_format,
        } => {
            let effective_format = cli.effective_output_format(output_format);
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
                effective_format,
            )
            .await
        }

        Commands::Networks {
            command,
            output_format,
        } => {
            if let Some(subcommand) = command {
                match subcommand {
                    NetworkCommands::List { output_format } => {
                        let fmt = cli.effective_output_format(output_format);
                        cli::commands::network::list_networks(fmt)
                            .context("Failed to list networks")
                    }
                    NetworkCommands::Info {
                        network,
                        output_format,
                    } => {
                        let fmt = cli.effective_output_format(output_format);
                        cli::commands::network::show_network_info(&network, fmt)
                            .context("Failed to show network info")
                    }
                }
            } else {
                let fmt = cli.effective_output_format(output_format);
                cli::commands::network::list_networks(fmt).context("Failed to list networks")
            }
        }

        Commands::Inspect { url, output_format } => {
            let fmt = cli.effective_output_format(output_format);
            cli::commands::inspect::inspect_command(&cli, &url, fmt).await
        }

        Commands::Wallet { command } => {
            let network = cli.network.as_deref();
            match command {
                WalletCommands::Refresh => {
                    let result =
                        cli::commands::tempo_wallet::refresh_wallet(network, analytics.clone())
                            .await;
                    if let Some(ref a) = analytics {
                        if result.is_ok() {
                            a.track(analytics::Event::WalletRefreshed, analytics::EmptyPayload);
                        }
                    }
                    result.map_err(Into::into)
                }
            }
        }

        Commands::Services {
            command,
            output_format,
            refresh,
        } => {
            let effective_format = cli.effective_output_format(output_format);
            if let Some(subcommand) = command {
                match subcommand {
                    cli::ServicesCommands::List {
                        refresh,
                        output_format,
                    } => {
                        let fmt = cli.effective_output_format(output_format);
                        cli::commands::services::list_services(fmt, refresh)
                            .await
                            .map_err(Into::into)
                    }
                    cli::ServicesCommands::Info {
                        name,
                        output_format,
                    } => {
                        let fmt = cli.effective_output_format(output_format);
                        cli::commands::services::show_service(&name, fmt)
                            .await
                            .map_err(Into::into)
                    }
                }
            } else {
                cli::commands::services::list_services(effective_format, refresh)
                    .await
                    .map_err(Into::into)
            }
        }

        Commands::Keys {
            command,
            output_format,
        } => {
            let output_format = cli.effective_output_format(output_format);
            let network = cli.network.as_deref();
            if let Some(subcommand) = command {
                match subcommand {
                    cli::KeysCommands::List => {
                        cli::commands::keys::list_keys(output_format, network)
                            .await
                            .map_err(Into::into)
                    }
                    cli::KeysCommands::Switch { index } => {
                        let result =
                            cli::commands::keys::switch_key(index, output_format, network).await;
                        if let Some(ref a) = analytics {
                            if let Ok(Some(label)) = &result {
                                a.track(
                                    analytics::Event::KeySwitched,
                                    analytics::KeySwitchedPayload {
                                        index,
                                        label: label.clone(),
                                    },
                                );
                            }
                        }
                        result.map(|_| ()).map_err(Into::into)
                    }
                    cli::KeysCommands::Delete { index } => {
                        let result =
                            cli::commands::keys::delete_key(index, output_format, network).await;
                        if let Some(ref a) = analytics {
                            if let Ok(Some(label)) = &result {
                                a.track(
                                    analytics::Event::KeyDeleted,
                                    analytics::KeyDeletedPayload {
                                        index,
                                        label: label.clone(),
                                    },
                                );
                            }
                        }
                        result.map(|_| ()).map_err(Into::into)
                    }
                }
            } else {
                cli::commands::keys::list_keys(output_format, network)
                    .await
                    .map_err(Into::into)
            }
        }

        Commands::Whoami { output_format } => {
            let fmt = cli.effective_output_format(output_format);
            let network = cli.network.as_deref();
            if let Some(ref a) = analytics {
                a.track(analytics::Event::WhoamiViewed, analytics::EmptyPayload);
            }
            cli::commands::whoami::show_whoami(fmt, network)
                .await
                .map_err(Into::into)
        }
    };

    if let Some(ref a) = analytics {
        a.flush().await;
    }

    result
}

/// Make an HTTP request (main flow)
async fn make_request(cli: Cli, query: QueryArgs, analytics: Option<Analytics>) -> Result<()> {
    let config = load_config_with_overrides(&cli)?;

    let url = query.url.clone();
    let request_ctx = RequestContext::new(cli, query)?;
    let method_str = request_ctx.method.to_string();

    if let Some(ref a) = analytics {
        a.track(
            analytics::Event::QueryStarted,
            analytics::QueryStartedPayload {
                url: url.clone(),
                method: method_str.clone(),
            },
        );
    }

    if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
        eprintln!("Making {} request to: {url}", request_ctx.method);
    }

    let response = match request_ctx.execute(&url, None).await {
        Ok(r) => r,
        Err(e) => {
            if let Some(ref a) = analytics {
                a.track(
                    analytics::Event::QueryFailure,
                    analytics::QueryFailurePayload {
                        url: url.clone(),
                        method: method_str,
                        error: e.to_string(),
                    },
                );
            }
            return Err(e);
        }
    };

    if !response.is_payment_required() {
        if let Some(ref a) = analytics {
            a.track(
                analytics::Event::QuerySuccess,
                analytics::QuerySuccessPayload {
                    url: url.clone(),
                    method: method_str,
                    status_code: response.status_code,
                },
            );
        }
        handle_regular_response(&request_ctx.cli, &request_ctx.query, response)?;
        return Ok(());
    }

    if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
        eprintln!("402 status: payment required");
    }

    let has_wallet = config.evm.is_some()
        || wallet::credentials::WalletCredentials::load()
            .ok()
            .and_then(|c| c.active_wallet().cloned())
            .is_some();

    if !has_wallet && std::env::var("PRESTO_MOCK_PAYMENT").is_err() {
        anyhow::bail!(crate::error::PrestoError::ConfigMissing(
            "This request requires payment, but no wallet is configured".to_string()
        ));
    }

    let protocol =
        PaymentProtocol::detect(response.get_header("www-authenticate").map(|s| s.as_str()));

    let Some(protocol) = protocol else {
        anyhow::bail!("402 response missing WWW-Authenticate: Payment header");
    };

    if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
        eprintln!("Payment protocol: {}", protocol);
    }

    if let Some(ref a) = analytics {
        a.track(
            analytics::Event::PaymentStarted,
            analytics::PaymentStartedPayload {
                network: String::new(),
                amount: String::new(),
                currency: String::new(),
            },
        );
    }

    match handle_web_payment_request(&config, &request_ctx, &url, &response).await {
        Ok(response) => {
            if let Some(ref a) = analytics {
                a.track(
                    analytics::Event::PaymentSuccess,
                    analytics::PaymentSuccessPayload {
                        network: String::new(),
                        amount: String::new(),
                        currency: String::new(),
                        tx_hash: response
                            .get_header("payment-receipt")
                            .cloned()
                            .unwrap_or_default(),
                    },
                );
                a.track(
                    analytics::Event::QuerySuccess,
                    analytics::QuerySuccessPayload {
                        url: url.clone(),
                        method: method_str,
                        status_code: response.status_code,
                    },
                );
            }
            handle_regular_response(&request_ctx.cli, &request_ctx.query, response)?;
            Ok(())
        }
        Err(e) => {
            if let Some(ref a) = analytics {
                a.track(
                    analytics::Event::PaymentFailure,
                    analytics::PaymentFailurePayload {
                        network: String::new(),
                        amount: String::new(),
                        currency: String::new(),
                        error: e.to_string(),
                    },
                );
            }
            Err(e)
        }
    }
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
