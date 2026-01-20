//! purl CLI - A curl-like tool for payment-enabled HTTP requests

mod balance_command;
mod cli;
mod config_commands;
mod config_utils;
mod errors;
mod exit_codes;
mod init;
mod inspect_command;
mod network_commands;
mod output;
mod request;
mod wallet_commands;
mod web_payment;

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser};
use clap_complete::{generate, shells};
use cli::{
    Cli, ColorMode, Commands, ConfigCommands, MethodCommands, NetworkCommands, OutputFormat, Shell,
};
use colored::control;
use exit_codes::ExitCode;
use purl::protocol::web::PaymentProtocol;
use purl::{Config, WalletConfig};
use std::path::PathBuf;

use config_utils::{load_config, load_config_with_overrides};
use output::{
    build_config_display, decrypt_keystores_upfront, handle_regular_response,
    print_payment_method_text, write_output,
};

use request::RequestContext;
use web_payment::handle_web_payment_request;

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
        eprintln!("{}", errors::format_error_with_suggestion(&e));
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
        Commands::Init { force, skip_ai } => init::run_init(*force, *skip_ai),

        Commands::Config {
            command,
            output_format,
            unsafe_show_private_keys,
        } => {
            if let Some(subcommand) = command {
                match subcommand {
                    ConfigCommands::Get { key, output_format } => {
                        config_commands::get_command(cli, key, *output_format)
                    }
                    ConfigCommands::Validate => config_commands::validate_command(cli),
                }
            } else {
                show_config(cli, *output_format, *unsafe_show_private_keys)
            }
        }

        Commands::Version => show_version(),

        Commands::Method { command } => match command {
            MethodCommands::List => wallet_commands::list_command(),
            MethodCommands::New { name, generate } => wallet_commands::new_command(name, *generate),
            MethodCommands::Import { name, private_key } => {
                wallet_commands::import_command(name, private_key.clone())
            }
            MethodCommands::Show { name } => wallet_commands::show_command(name),
            MethodCommands::Verify { name } => wallet_commands::verify_command(name),
        },

        Commands::Completions { shell } => generate_completions(*shell),

        Commands::Balance { address } => {
            let config = load_config(cli.config.as_ref())?;
            balance_command::balance_command(&config, address.clone(), cli.network.clone()).await
        }

        Commands::Networks {
            command,
            output_format,
        } => {
            if let Some(subcommand) = command {
                match subcommand {
                    NetworkCommands::List { output_format } => {
                        network_commands::list_networks(*output_format)
                            .context("Failed to list networks")
                    }
                    NetworkCommands::Info {
                        network,
                        output_format,
                    } => network_commands::show_network_info(network, *output_format)
                        .context("Failed to show network info"),
                }
            } else {
                network_commands::list_networks(*output_format).context("Failed to list networks")
            }
        }

        Commands::Inspect { url } => inspect_command::inspect_command(cli, url),
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
        .context("URL is required (or use 'purl init' to initialize configuration)")?;

    if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
        eprintln!("Making {} request to: {url}", request_ctx.method);
    }

    let response = request_ctx.execute(url, None)?;

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

fn show_config(cli: &Cli, output_format: OutputFormat, show_private_keys: bool) -> Result<()> {
    let config = load_config(cli.config.as_ref())?;
    let config_path = if let Some(ref path) = cli.config {
        PathBuf::from(path)
    } else {
        Config::default_config_path()?
    };

    let use_password_cache = !cli.no_cache_password;

    let decrypted_keys = if show_private_keys {
        Some(decrypt_keystores_upfront(&config, use_password_cache)?)
    } else {
        None
    };

    match output_format {
        OutputFormat::Json => {
            let display_data = build_config_display(
                &config,
                &config_path,
                show_private_keys,
                decrypted_keys.as_ref(),
            );
            let output = serde_json::to_string_pretty(&display_data)?;
            write_output(cli, output)?;
        }
        OutputFormat::Yaml => {
            let display_data = build_config_display(
                &config,
                &config_path,
                show_private_keys,
                decrypted_keys.as_ref(),
            );
            let output = serde_yaml::to_string(&display_data)?;
            write_output(cli, output)?;
        }
        OutputFormat::Text => {
            println!("Config file: {}", config_path.display());
            println!();

            if let Some(evm) = &config.evm {
                print_payment_method_text(
                    "evm",
                    evm.keystore.as_ref(),
                    evm.get_address().ok().as_deref(),
                    "address",
                    decrypted_keys
                        .as_ref()
                        .and_then(|k| k.evm_private_key.as_deref()),
                    show_private_keys,
                );
            }

            if config.evm.is_none() {
                println!("No payment methods configured.");
                println!("Run 'purl init' to configure payment methods.");
            }
        }
    }

    Ok(())
}

// ==================== Simple Commands ====================

/// Show version information
fn show_version() -> Result<()> {
    const CLI_VERSION: &str = env!("CARGO_PKG_VERSION");

    println!("purl CLI: v{CLI_VERSION}");
    println!("purl: v{}", purl::VERSION);

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

#[cfg(test)]
mod tests {
    use super::*;
    use output::DecryptedKeys;
    use purl::{Config, EvmConfig};

    #[test]
    fn test_decrypt_keystores_upfront_with_plain_keys() {
        let config = Config {
            evm: Some(EvmConfig {
                private_key: Some(
                    "0x1234567890123456789012345678901234567890123456789012345678901234"
                        .to_string(),
                ),
                keystore: None,
            }),
            ..Default::default()
        };

        let result = decrypt_keystores_upfront(&config, false);
        assert!(result.is_ok());

        let keys = result.unwrap();
        assert!(keys.evm_private_key.is_some());
    }

    #[test]
    fn test_decrypt_keystores_upfront_with_no_keys() {
        let config = Config {
            evm: None,
            ..Default::default()
        };

        let result = decrypt_keystores_upfront(&config, false);
        assert!(result.is_ok());

        let keys = result.unwrap();
        assert!(keys.evm_private_key.is_none());
    }

    #[test]
    fn test_build_config_display_without_private_keys() {
        let config = Config {
            evm: Some(EvmConfig {
                private_key: Some(
                    "0x1234567890123456789012345678901234567890123456789012345678901234"
                        .to_string(),
                ),
                keystore: None,
            }),
            ..Default::default()
        };

        let config_path = PathBuf::from("/test/config.toml");
        let display = build_config_display(&config, &config_path, false, None);

        let evm = display.get("evm").unwrap();
        assert!(evm.get("private_key").is_none());
    }

    #[test]
    fn test_build_config_display_with_private_keys() {
        let config = Config {
            evm: Some(EvmConfig {
                private_key: Some(
                    "0x1234567890123456789012345678901234567890123456789012345678901234"
                        .to_string(),
                ),
                keystore: None,
            }),
            ..Default::default()
        };

        let decrypted_keys = DecryptedKeys {
            evm_private_key: Some(
                "0x1234567890123456789012345678901234567890123456789012345678901234".to_string(),
            ),
        };

        let config_path = PathBuf::from("/test/config.toml");
        let display = build_config_display(&config, &config_path, true, Some(&decrypted_keys));

        let evm = display.get("evm").unwrap();
        assert!(evm.get("private_key").is_some());
    }
}
