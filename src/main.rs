//! pget CLI - A wget-like tool for payment-enabled HTTP requests

// Library modules (from lib.rs)
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

const VERSION: &str = env!("CARGO_PKG_VERSION");

use config::{Config, WalletConfig};
use mpay::PaymentProtocol;

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser};
use clap_complete::{generate, shells};
use cli::exit_codes::ExitCode;
use cli::{Cli, ColorMode, Commands, ConfigCommands, NetworkCommands, OutputFormat, Shell};
use colored::control;
use std::path::PathBuf;

use cli::output::{handle_regular_response, write_output};
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
    let cli = Cli::parse();

    // Initialize color support based on user preference and NO_COLOR env var
    init_color_support(&cli);

    // Handle subcommands
    if let Some(ref command) = cli.command {
        return handle_command(&cli, command).await;
    }

    // No subcommand - make an HTTP request
    make_request(cli).await
}

/// Handle CLI subcommands
async fn handle_command(cli: &Cli, command: &Commands) -> Result<()> {
    match command {
        Commands::Init { force, skip_ai } => cli::commands::init::run_init(*force, *skip_ai).await,

        Commands::Config {
            command,
            output_format,
        } => {
            if let Some(subcommand) = command {
                match subcommand {
                    ConfigCommands::Get { key, output_format } => {
                        cli::commands::config::get_command(cli, key, *output_format)
                    }
                    ConfigCommands::Validate => cli::commands::config::validate_command(cli),
                }
            } else {
                show_config(cli, *output_format)
            }
        }

        Commands::Version => show_version(),

        Commands::Completions { shell } => generate_completions(*shell),

        Commands::Balance { address } => {
            let config = load_config(cli.config.as_ref())?;
            cli::commands::balance::balance_command(&config, address.clone(), cli.network.clone())
                .await
        }

        Commands::Networks {
            command,
            output_format,
        } => {
            if let Some(subcommand) = command {
                match subcommand {
                    NetworkCommands::List { output_format } => {
                        cli::commands::network::list_networks(*output_format)
                            .context("Failed to list networks")
                    }
                    NetworkCommands::Info {
                        network,
                        output_format,
                    } => cli::commands::network::show_network_info(network, *output_format)
                        .context("Failed to show network info"),
                }
            } else {
                cli::commands::network::list_networks(*output_format)
                    .context("Failed to list networks")
            }
        }

        Commands::Inspect { url } => cli::commands::inspect::inspect_command(cli, url).await,

        Commands::Wallet {
            command,
            output_format,
        } => {
            let network = cli.network.as_deref();
            if let Some(subcommand) = command {
                match subcommand {
                    cli::WalletCommands::Connect => {
                        cli::commands::tempo_wallet::connect_wallet(network)
                            .await
                            .map_err(Into::into)
                    }
                    cli::WalletCommands::Disconnect { yes } => {
                        cli::commands::tempo_wallet::disconnect_wallet(*yes, network)
                            .await
                            .map_err(Into::into)
                    }
                    cli::WalletCommands::Refresh => {
                        cli::commands::tempo_wallet::refresh_wallet(network)
                            .await
                            .map_err(Into::into)
                    }
                }
            } else {
                cli::commands::tempo_wallet::show_wallet(*output_format, network)
                    .await
                    .map_err(Into::into)
            }
        }

        Commands::Keys {
            command,
            output_format,
        } => {
            let network = cli.network.as_deref();
            if let Some(subcommand) = command {
                match subcommand {
                    cli::KeysCommands::List => {
                        cli::commands::keys::list_keys(*output_format, network)
                            .await
                            .map_err(Into::into)
                    }
                    cli::KeysCommands::Switch { index } => {
                        cli::commands::keys::switch_key(*index, *output_format, network)
                            .await
                            .map_err(Into::into)
                    }
                    cli::KeysCommands::Delete { index } => {
                        cli::commands::keys::delete_key(*index, *output_format, network)
                            .await
                            .map_err(Into::into)
                    }
                }
            } else {
                cli::commands::keys::list_keys(*output_format, network)
                    .await
                    .map_err(Into::into)
            }
        }

        Commands::Services {
            command,
            output_format,
            refresh,
        } => {
            if let Some(subcommand) = command {
                match subcommand {
                    cli::ServicesCommands::List { refresh } => {
                        cli::commands::services::list_services(*output_format, *refresh)
                            .await
                            .map_err(Into::into)
                    }
                    cli::ServicesCommands::Info { name } => {
                        cli::commands::services::show_service(name, *output_format)
                            .await
                            .map_err(Into::into)
                    }
                }
            } else {
                cli::commands::services::list_services(*output_format, *refresh)
                    .await
                    .map_err(Into::into)
            }
        }

        Commands::Status { output_format } => {
            let network = cli.network.as_deref();
            cli::commands::status::show_status(*output_format, network)
                .await
                .map_err(Into::into)
        }
    }
}

/// Make an HTTP request (main flow)
async fn make_request(cli: Cli) -> Result<()> {
    let config = load_config_with_overrides(&cli)?;

    let request_ctx = RequestContext::new(cli)?;

    let url = request_ctx
        .cli
        .url
        .as_ref()
        .context("URL is required (or use 'pget init' to initialize configuration)")?;

    if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
        eprintln!("Making {} request to: {url}", request_ctx.method);
    }

    let response = request_ctx.execute(url, None).await?;

    if !response.is_payment_required() {
        handle_regular_response(&request_ctx.cli, response)?;
        return Ok(());
    }

    if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
        eprintln!("402 status: payment required");
    }

    let protocol =
        PaymentProtocol::detect(response.get_header("www-authenticate").map(|s| s.as_str()));

    let Some(protocol) = protocol else {
        anyhow::bail!("402 response missing WWW-Authenticate: Payment header");
    };

    if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
        eprintln!("Payment protocol: {}", protocol);
    }

    let response = handle_web_payment_request(&config, &request_ctx, url, &response).await?;

    handle_regular_response(&request_ctx.cli, response)?;

    Ok(())
}

// ==================== Config Display ====================

fn show_config(cli: &Cli, output_format: OutputFormat) -> Result<()> {
    let config = load_config(cli.config.as_ref())?;
    let config_path = if let Some(ref path) = cli.config {
        PathBuf::from(path)
    } else {
        Config::default_config_path()?
    };

    let display_data = serde_json::json!({
        "config_path": config_path.display().to_string(),
        "evm": config.evm.as_ref().and_then(|evm| {
            evm.get_address().ok().map(|address| {
                serde_json::json!({ "address": address })
            })
        })
    });

    match output_format {
        OutputFormat::Json => {
            let output = serde_json::to_string_pretty(&display_data)?;
            write_output(cli, output)?;
        }
        OutputFormat::Yaml => {
            let output = serde_yaml::to_string(&display_data)?;
            write_output(cli, output)?;
        }
        OutputFormat::Text => {
            println!("Config file: {}", config_path.display());
            println!();

            if let Some(evm) = &config.evm {
                if let Ok(address) = evm.get_address() {
                    println!("[evm]");
                    println!("address = \"{address}\"");
                    println!();
                }
            }

            if config.evm.is_none() {
                println!("No payment methods configured.");
                println!("Run 'pget init' to configure payment methods.");
            }
        }
    }

    Ok(())
}

// ==================== Simple Commands ====================

/// Show version information
fn show_version() -> Result<()> {
    println!("pget: v{VERSION}");

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
