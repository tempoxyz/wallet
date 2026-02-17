//!  tempo-walletCLI - A command-line HTTP client with built-in payment support

mod analytics;
mod cli;
mod config;
mod error;
mod http;
mod network;
mod payment;
mod util;
mod wallet;

use mpp::PaymentProtocol;

use anyhow::Result;
use clap::{CommandFactory, Parser};
use clap_complete::{generate, shells};
use cli::exit_codes::ExitCode;
use cli::{Cli, ColorMode, Commands, QueryArgs, SessionCommands, Shell};
use colored::control;

use analytics::Analytics;
use cli::output::handle_regular_response;
use config::{load_config, load_config_with_overrides};
use http::request::RequestContext;
use payment::web_payment::handle_web_payment_request;
use payment::web_session::{handle_web_session_request, SessionResult};

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
/// This allows ` tempo-wallethttps://example.com` as a shorthand for
/// ` tempo-walletquery https://example.com`, making the primary use case
/// as frictionless as curl/wget.
fn parse_cli() -> Cli {
    match Cli::try_parse() {
        Ok(cli) => cli,
        Err(original_err) => {
            // If normal parsing failed, try again with "query" inserted.
            // This handles cases like ` tempo-wallethttps://example.com` or
            // ` tempo-wallet-X POST --json '{}' https://example.com`.
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
        Commands::Query(query) => make_request(cli, *query, analytics.clone()).await,

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

/// Make an HTTP request (main flow)
async fn make_request(cli: Cli, query: QueryArgs, analytics: Option<Analytics>) -> Result<()> {
    let mut config = load_config_with_overrides(&cli)?;

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
        let status = response.status_code;
        handle_regular_response(&request_ctx.cli, &request_ctx.query, response)?;
        if status >= 400 {
            anyhow::bail!(crate::error::PrestoError::Http(format!(
                "{} {}",
                status,
                http_status_text(status)
            )));
        }
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
        use std::io::IsTerminal;
        if std::io::stdin().is_terminal() {
            eprintln!("This request requires payment. Let's connect your wallet first.\n");
            let network = request_ctx.cli.network.as_deref();
            cli::commands::login::run_login(network, analytics.clone()).await?;
            eprintln!("\nRetrying request...");
            config = load_config_with_overrides(&request_ctx.cli)?;
        } else {
            anyhow::bail!(crate::error::PrestoError::ConfigMissing(
                "No wallet configured. Run ' tempo-walletlogin' to connect your wallet, then retry the request.".to_string()
            ));
        }
    }

    let protocol =
        PaymentProtocol::detect(response.get_header("www-authenticate").map(|s| s.as_str()));

    let Some(protocol) = protocol else {
        anyhow::bail!(crate::error::PrestoError::MissingHeader(
            "WWW-Authenticate: Payment".to_string()
        ));
    };

    if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
        eprintln!("Payment protocol: {}", protocol);
    }

    // Extract payment details from 402 response for analytics
    let (pay_network, pay_amount, pay_currency) = response
        .get_header("www-authenticate")
        .and_then(|h| mpp::parse_www_authenticate(h).ok())
        .map(|challenge| {
            let network = payment::mpp_ext::method_to_network(&challenge.method)
                .unwrap_or("")
                .to_string();
            let charge: Option<mpp::ChargeRequest> = challenge.request.decode().ok();
            let amount = charge
                .as_ref()
                .map(|c| c.amount.clone())
                .unwrap_or_default();
            let currency = charge
                .as_ref()
                .map(|c| c.currency.clone())
                .unwrap_or_default();
            (network, amount, currency)
        })
        .unwrap_or_default();

    if let Some(ref a) = analytics {
        a.track(
            analytics::Event::PaymentStarted,
            analytics::PaymentStartedPayload {
                network: pay_network.clone(),
                amount: pay_amount.clone(),
                currency: pay_currency.clone(),
            },
        );
    }

    // Detect intent from challenge to branch between charge and session flows.
    let is_session = response
        .get_header("www-authenticate")
        .and_then(|h| mpp::parse_www_authenticate(h).ok())
        .is_some_and(|challenge| challenge.intent.is_session());

    if is_session {
        if request_ctx.cli.is_verbose() && request_ctx.cli.should_show_output() {
            eprintln!("Payment intent: session");
        }

        match handle_web_session_request(&config, &request_ctx, &url, &response).await {
            Ok(result) => {
                if let Some(ref a) = analytics {
                    a.track(
                        analytics::Event::PaymentSuccess,
                        analytics::PaymentSuccessPayload {
                            network: pay_network,
                            amount: pay_amount,
                            currency: pay_currency,
                            tx_hash: String::new(),
                        },
                    );
                    a.track(
                        analytics::Event::QuerySuccess,
                        analytics::QuerySuccessPayload {
                            url: url.clone(),
                            method: method_str,
                            status_code: 200,
                        },
                    );
                }
                match result {
                    SessionResult::Streamed => Ok(()),
                    SessionResult::Response(resp) => {
                        let status = resp.status_code;
                        handle_regular_response(&request_ctx.cli, &request_ctx.query, resp)?;
                        if status >= 400 {
                            anyhow::bail!(crate::error::PrestoError::Http(format!(
                                "{} {}",
                                status,
                                http_status_text(status)
                            )));
                        }
                        Ok(())
                    }
                }
            }
            Err(e) => {
                if let Some(ref a) = analytics {
                    a.track(
                        analytics::Event::PaymentFailure,
                        analytics::PaymentFailurePayload {
                            network: pay_network,
                            amount: pay_amount,
                            currency: pay_currency,
                            error: e.to_string(),
                        },
                    );
                }
                Err(e)
            }
        }
    } else {
        match handle_web_payment_request(&config, &request_ctx, &url, &response).await {
            Ok(response) => {
                if let Some(ref a) = analytics {
                    a.track(
                        analytics::Event::PaymentSuccess,
                        analytics::PaymentSuccessPayload {
                            network: pay_network,
                            amount: pay_amount,
                            currency: pay_currency,
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
                let status = response.status_code;
                handle_regular_response(&request_ctx.cli, &request_ctx.query, response)?;
                if status >= 400 {
                    anyhow::bail!(crate::error::PrestoError::Http(format!(
                        "{} {}",
                        status,
                        http_status_text(status)
                    )));
                }
                Ok(())
            }
            Err(e) => {
                if let Some(ref a) = analytics {
                    a.track(
                        analytics::Event::PaymentFailure,
                        analytics::PaymentFailurePayload {
                            network: pay_network,
                            amount: pay_amount,
                            currency: pay_currency,
                            error: e.to_string(),
                        },
                    );
                }
                Err(e)
            }
        }
    }
}

fn http_status_text(code: u32) -> &'static str {
    match code {
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Error",
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
