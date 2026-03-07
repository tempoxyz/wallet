//! Shared CLI infrastructure for Tempo extension binaries.

use clap::{ArgAction, Parser};

use crate::context::{Context, ContextArgs};
use crate::output::OutputFormat;
use crate::runtime::ColorMode;
use crate::util::Verbosity;

/// Global CLI flags shared by all Tempo extension binaries.
#[derive(Parser, Debug)]
pub struct GlobalArgs {
    /// Configuration file path
    #[arg(
        short = 'c',
        long = "config",
        value_name = "PATH",
        global = true,
        hide = true
    )]
    pub config: Option<String>,

    /// Use a private key directly for payment (bypasses wallet login)
    #[arg(
        long = "private-key",
        value_name = "KEY",
        env = "TEMPO_PRIVATE_KEY",
        global = true,
        hide = true,
        hide_env_values = true
    )]
    pub private_key: Option<String>,

    /// Network to use (e.g. "tempo", "tempo-moderato")
    #[arg(short = 'n', long, value_name = "NETWORK", global = true, hide = true)]
    pub network: Option<String>,

    /// Override RPC URL (applies to all commands)
    #[arg(
        short = 'r',
        long = "rpc",
        visible_alias = "rpc-url",
        value_name = "URL",
        env = "TEMPO_RPC_URL",
        global = true,
        hide = true
    )]
    pub rpc_url: Option<String>,

    /// Verbosity: repeat -v to increase (info, debug, trace)
    #[arg(short = 'v', long = "verbose", action = ArgAction::Count, global = true, help_heading = "Display Options")]
    pub verbose: u8,

    /// Silent mode: suppress non-essential output
    #[arg(
        short = 's',
        long = "silent",
        global = true,
        help_heading = "Display Options"
    )]
    pub silent: bool,

    /// Control color output
    #[arg(
        long,
        value_name = "MODE",
        default_value = "auto",
        global = true,
        hide = true
    )]
    pub color: ColorMode,

    /// Quick switch for JSON output format
    #[arg(
        short = 'j',
        long = "json-output",
        help_heading = "Display Options",
        global = true
    )]
    pub json_output: bool,

    /// Quick switch for TOON output format (compact, token-efficient)
    #[arg(
        short = 't',
        long = "toon-output",
        help_heading = "Display Options",
        global = true,
        conflicts_with = "json_output"
    )]
    pub toon_output: bool,
}

impl GlobalArgs {
    /// Resolve the effective output format from CLI flags.
    pub fn resolve_output_format(&self) -> OutputFormat {
        if self.json_output {
            OutputFormat::Json
        } else if self.toon_output {
            OutputFormat::Toon
        } else {
            OutputFormat::Text
        }
    }

    /// Build a `Verbosity` from CLI flags (silent overrides verbose).
    pub fn verbosity(&self) -> Verbosity {
        Verbosity {
            level: if self.silent { 0 } else { self.verbose.min(3) },
            show_output: !self.silent,
        }
    }

    /// Build a `ContextArgs` for context construction.
    pub fn context_args(&self) -> ContextArgs {
        ContextArgs {
            config_path: self.config.clone(),
            rpc_url: self.rpc_url.clone(),
            requested_network: self.network.clone(),
            private_key: self.private_key.clone(),
            output_format: self.resolve_output_format(),
            verbosity: self.verbosity(),
        }
    }

    /// Build the shared runtime context from these global args.
    pub async fn build_context(&self) -> anyhow::Result<Context> {
        Context::build(self.context_args()).await
    }

    /// Print structured version info for JSON/TOON output formats, then exit.
    ///
    /// Call this from `handle_version` in each binary's CLI parser.
    pub fn emit_structured_version(args: &[String]) {
        let format = if args.iter().any(|a| a == "-j" || a == "--json-output") {
            Some(OutputFormat::Json)
        } else if args.iter().any(|a| a == "-t" || a == "--toon-output") {
            Some(OutputFormat::Toon)
        } else {
            None
        };

        if let Some(output_format) = format {
            let payload = serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"),
                "git_commit": env!("TEMPO_GIT_SHA"),
                "build_date": env!("TEMPO_BUILD_DATE"),
                "profile": env!("TEMPO_BUILD_PROFILE"),
            });

            crate::output::emit_formatted_or_fallback(
                || crate::output::format_structured_pretty_json(output_format, &payload),
                || env!("CARGO_PKG_VERSION").to_string(),
            );
            std::process::exit(0);
        }
    }
}

/// Shared analytics tracking for command dispatch.
pub mod dispatch {
    use crate::analytics::{self, Analytics};
    use crate::util::sanitize_error;

    /// Track the initial command run event.
    pub fn track_command(analytics: &Option<Analytics>, cmd_name: &str) {
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
    pub fn track_result(
        analytics: &Option<Analytics>,
        cmd_name: &str,
        result: &anyhow::Result<()>,
    ) {
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
}

/// Run a CLI binary with shared error handling.
///
/// Handles structured error output for JSON/TOON formats and sets the process
/// exit code based on the error type.
pub fn run_main(output_format: OutputFormat, result: Result<(), anyhow::Error>) {
    let Err(e) = result else { return };

    match output_format {
        OutputFormat::Json | OutputFormat::Toon => {
            let code = crate::exit_codes::ExitCode::from(&e).label();
            let payload = crate::runtime::render_error_payload(&e, code);
            crate::output::emit_formatted_or_fallback(
                || crate::output::format_structured(output_format, &payload),
                || crate::runtime::render_error_fallback(code),
            );
        }
        _ => {
            eprintln!("Error: {e:#}");
        }
    }
    crate::exit_codes::ExitCode::from(&e).exit();
}
