//! Global CLI argument definitions shared by all Tempo extension binaries.

use clap::{ArgAction, Parser};

use super::{context::ContextArgs, output::OutputFormat, runtime::ColorMode, verbosity::Verbosity};
use crate::error::TempoError;

/// Global CLI flags shared by all Tempo extension binaries.
#[derive(Parser, Debug)]
#[allow(clippy::struct_excessive_bools)]
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

    /// Network to use (e.g. "testnet")
    #[arg(
        short = 'n',
        long,
        value_name = "NETWORK",
        global = true,
        help_heading = "Network"
    )]
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

    /// Emit command schema as JSON for agent introspection
    #[arg(long, global = true, hide = true)]
    pub describe: bool,
}

impl GlobalArgs {
    /// Resolve the effective output format from CLI flags.
    ///
    /// When neither `--json-output` nor `--toon-output` is explicitly set:
    /// - If running inside an LLM agent (detected via env vars), defaults to TOON.
    /// - Otherwise, if stdout is not a terminal, defaults to JSON.
    ///
    /// Set `TEMPO_NO_AUTO_JSON=1` to disable auto-detection.
    #[must_use]
    pub fn resolve_output_format(&self) -> OutputFormat {
        use std::io::IsTerminal;
        if self.json_output {
            OutputFormat::Json
        } else if self.toon_output || is_agent_environment() {
            OutputFormat::Toon
        } else if should_auto_json() && !std::io::stdout().is_terminal() {
            OutputFormat::Json
        } else {
            OutputFormat::Text
        }
    }

    /// Build a `Verbosity` from CLI flags (silent overrides verbose).
    #[must_use]
    pub fn verbosity(&self) -> Verbosity {
        Verbosity {
            level: if self.silent { 0 } else { self.verbose.min(3) },
            show_output: !self.silent,
        }
    }

    /// Warn if private key was provided via CLI argument instead of environment variable.
    ///
    /// Command-line arguments are visible in process listings (`ps`), so passing
    /// secrets via `--private-key` is a security risk. The environment variable
    /// `TEMPO_PRIVATE_KEY` is the recommended alternative.
    pub(crate) fn warn_argv_private_key(&self) {
        if self.private_key.is_some() && std::env::var("TEMPO_PRIVATE_KEY").is_err() {
            eprintln!(
                "WARNING: --private-key on the command line exposes your key in the process list."
            );
            eprintln!("  Use TEMPO_PRIVATE_KEY environment variable instead.");
        }
    }

    /// Build a `ContextArgs` for context construction.
    pub(crate) fn context_args(&self, app_id: &'static str) -> ContextArgs {
        ContextArgs {
            config_path: self.config.clone(),
            rpc_url: self.rpc_url.clone(),
            requested_network: self.network.clone(),
            private_key: self.private_key.clone(),
            output_format: self.resolve_output_format(),
            verbosity: self.verbosity(),
            app_id,
        }
    }

    /// Build the shared runtime context from these global args.
    pub(crate) async fn build_context(
        &self,
        app_id: &'static str,
    ) -> Result<super::context::Context, TempoError> {
        super::context::Context::build(self.context_args(app_id)).await
    }

    /// Emit a JSON schema of the CLI command tree and exit.
    ///
    /// Walks the clap `Command` to produce a stable, agent-friendly description
    /// of every subcommand, flag, and positional argument.
    pub(crate) fn emit_describe<T: clap::CommandFactory>() -> ! {
        let cmd = T::command();
        let schema = describe_command(&cmd);
        // Always emit as JSON regardless of output format flags
        match serde_json::to_string_pretty(&schema) {
            Ok(json) => println!("{json}"),
            Err(e) => {
                eprintln!("Failed to serialize command schema: {e}");
                std::process::exit(1);
            }
        }
        std::process::exit(0)
    }

    /// Resolve output format from raw CLI argv (for pre-parse contexts like --version).
    pub(crate) fn resolve_output_format_from_argv(args: &[String]) -> OutputFormat {
        use std::io::IsTerminal;
        if args.iter().any(|a| a == "-j" || a == "--json-output") {
            OutputFormat::Json
        } else if args.iter().any(|a| a == "-t" || a == "--toon-output") || is_agent_environment() {
            OutputFormat::Toon
        } else if should_auto_json() && !std::io::stdout().is_terminal() {
            OutputFormat::Json
        } else {
            OutputFormat::Text
        }
    }

    /// Print structured version info for JSON/TOON output formats, then exit.
    ///
    /// Call this from `handle_version` in each binary's CLI parser.
    pub(crate) fn emit_structured_version(args: &[String]) {
        let output_format = Self::resolve_output_format_from_argv(args);
        if !output_format.is_structured() {
            return;
        }

        let payload = serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "git_commit": env!("TEMPO_GIT_SHA"),
            "build_date": env!("TEMPO_BUILD_DATE"),
            "profile": env!("TEMPO_BUILD_PROFILE"),
        });

        super::output::emit_formatted_or_fallback(
            || super::output::format_structured_pretty_json(output_format, &payload),
            || env!("CARGO_PKG_VERSION").to_string(),
        );
        std::process::exit(0);
    }
}

/// Parse a CLI struct, handling structured version output for JSON/TOON formats.
///
/// Wraps `clap::Parser::try_parse` to intercept `DisplayVersion` and emit
/// structured version info when `-j` or `-t` is present.
#[must_use]
pub fn parse_cli<T: clap::Parser + clap::CommandFactory>() -> T {
    // Handle --describe before normal parsing
    if std::env::args().any(|a| a == "--describe") {
        GlobalArgs::emit_describe::<T>();
    }

    match T::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            if matches!(err.kind(), clap::error::ErrorKind::DisplayVersion) {
                let args: Vec<String> = std::env::args().collect();
                GlobalArgs::emit_structured_version(&args);
            }

            // Structured error output for usage/parse errors
            if err.use_stderr() {
                let args: Vec<String> = std::env::args().collect();
                let format = GlobalArgs::resolve_output_format_from_argv(&args);
                if format.is_structured() {
                    let payload = serde_json::json!({
                        "code": "E_USAGE",
                        "message": err.to_string().lines().next().unwrap_or("invalid usage"),
                    });
                    super::output::emit_formatted_or_fallback(
                        || super::output::format_structured(format, &payload),
                        || format!("{{\"code\":\"E_USAGE\",\"message\":\"{}\"}}", err.kind()),
                    );
                    std::process::exit(2);
                }
            }

            err.exit()
        }
    }
}

/// Whether auto-JSON detection is enabled (disabled by `TEMPO_NO_AUTO_JSON=1`).
fn should_auto_json() -> bool {
    std::env::var("TEMPO_NO_AUTO_JSON").is_err()
}

/// Environment variables that LLM agent hosts set.
const AGENT_ENV_VARS: &[&str] = &[
    "AGENT",           // Generic agent flag
    "CLAUDE_CODE",     // Claude Code
    "CODEX",           // OpenAI Codex CLI
    "AMP_THREAD_ID",   // Amp
    "CURSOR_TRACE_ID", // Cursor
];

/// Returns `true` when the process is running inside an LLM coding agent.
fn is_agent_environment() -> bool {
    AGENT_ENV_VARS.iter().any(|v| std::env::var(v).is_ok())
}

/// Recursively describe a clap `Command` as a JSON value.
fn describe_command(cmd: &clap::Command) -> serde_json::Value {
    let mut args = Vec::new();
    for arg in cmd.get_arguments() {
        if arg.is_hide_set() {
            continue;
        }
        let mut entry = serde_json::json!({
            "name": arg.get_id().as_str(),
        });
        let map = entry.as_object_mut().unwrap();

        if let Some(short) = arg.get_short() {
            map.insert(
                "short".into(),
                serde_json::Value::String(format!("-{short}")),
            );
        }
        if let Some(long) = arg.get_long() {
            map.insert(
                "long".into(),
                serde_json::Value::String(format!("--{long}")),
            );
        }
        if let Some(help) = arg.get_help() {
            map.insert("help".into(), serde_json::Value::String(help.to_string()));
        }
        if arg.is_required_set() {
            map.insert("required".into(), serde_json::Value::Bool(true));
        }
        if arg.is_global_set() {
            map.insert("global".into(), serde_json::Value::Bool(true));
        }

        // Detect flags (no value) vs options (takes value)
        let num_vals = arg.get_num_args();
        if num_vals.is_some_and(|r| r.max_values() == 0) {
            map.insert("type".into(), serde_json::Value::String("flag".into()));
        } else if arg.get_long().is_some() || arg.get_short().is_some() {
            map.insert("type".into(), serde_json::Value::String("option".into()));
            if let Some(val_names) = arg.get_value_names() {
                let names: Vec<&str> = val_names.iter().map(clap::builder::Str::as_str).collect();
                if names.len() == 1 {
                    map.insert(
                        "value_name".into(),
                        serde_json::Value::String(names[0].to_string()),
                    );
                }
            }
        } else {
            map.insert(
                "type".into(),
                serde_json::Value::String("positional".into()),
            );
        }

        let possible = arg.get_possible_values();
        if !possible.is_empty() {
            let values: Vec<&str> = possible
                .iter()
                .filter(|v| !v.is_hide_set())
                .map(clap::builder::PossibleValue::get_name)
                .collect();
            if !values.is_empty() {
                map.insert("possible_values".into(), serde_json::json!(values));
            }
        }

        args.push(entry);
    }

    let mut subcommands = Vec::new();
    for sub in cmd.get_subcommands() {
        if sub.is_hide_set() {
            continue;
        }
        subcommands.push(describe_command(sub));
    }

    let mut result = serde_json::json!({
        "name": cmd.get_name(),
    });
    let map = result.as_object_mut().unwrap();

    if let Some(about) = cmd.get_about() {
        map.insert("about".into(), serde_json::Value::String(about.to_string()));
    }
    if !args.is_empty() {
        map.insert("args".into(), serde_json::json!(args));
    }
    if !subcommands.is_empty() {
        map.insert("subcommands".into(), serde_json::json!(subcommands));
    }

    result
}
