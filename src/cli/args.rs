//! CLI argument definitions and parsing.

use clap::{Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
pub enum OutputFormat {
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}

#[derive(Parser, Debug)]
#[command(name = "presto")]
#[command(about = "A command-line HTTP client with built-in MPP payment support", long_about = None)]
#[command(version)]
#[command(
    override_usage = "presto [OPTIONS] <URL> [-- HTTP_OPTIONS]\n  presto [OPTIONS] <COMMAND>"
)]
#[command(
    after_help = "Examples:\n  # Query Ethereum via Alchemy — no API key needed\n  presto https://alchemy.mpp.tempo.xyz/eth-mainnet/v2 \\\n    -X POST --json '{\"jsonrpc\":\"2.0\",\"method\":\"eth_blockNumber\",\"params\":[],\"id\":1}' | jq .result\n\n  # Use GPT-4o — no API key, no signup, just pay and go\n  presto https://openrouter.mpp.tempo.xyz/v1/chat/completions \\\n    -X POST --json '{\"model\":\"openai/gpt-4o-mini\",\"messages\":[{\"role\":\"user\",\"content\":\"Tell me a fun fact\"}]}' \\\n    | jq -r '.choices[0].message.content'\n\n  # Search the web — find anything, instantly\n  presto https://exa.mpp.tempo.xyz/search \\\n    -X POST --json '{\"query\":\"best new developer tools\",\"numResults\":5}' \\\n    | jq -r '.results[] | \"\\(.title)\\n  \\(.url)\\n\"'"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Configuration file path
    #[arg(short = 'c', long = "config", value_name = "PATH", global = true)]
    pub config: Option<String>,

    /// Filter to specific networks (comma-separated, e.g. "tempo, tempo-moderato")
    #[arg(
        short = 'n',
        long,
        value_name = "NETWORKS",
        global = true,
        help_heading = "Payment Options"
    )]
    pub network: Option<String>,

    /// Verbosity level (can be used multiple times: -v, -vv, -vvv)
    #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count, global = true, help_heading = "Display Options")]
    pub verbosity: u8,

    /// Control color output
    #[arg(
        long,
        value_name = "MODE",
        default_value = "auto",
        global = true,
        help_heading = "Display Options"
    )]
    pub color: ColorMode,

    /// Do not print log messages
    #[arg(
        short = 'q',
        long = "quiet",
        global = true,
        help_heading = "Display Options"
    )]
    pub quiet: bool,

    /// Output format
    #[arg(
        long,
        value_name = "FORMAT",
        default_value = "text",
        global = true,
        help_heading = "Display Options"
    )]
    pub output_format: OutputFormat,
}

/// Make an HTTP request with optional payment
#[derive(Parser, Debug)]
pub struct QueryArgs {
    /// URL to request
    #[arg(value_name = "URL")]
    pub url: String,

    /// Dry run mode - show what would be paid without executing
    #[arg(long, help_heading = "Payment Options")]
    pub dry_run: bool,

    /// Include HTTP headers in output
    #[arg(short = 'i', long = "include", help_heading = "Display Options")]
    pub include_headers: bool,

    /// Write output to file
    #[arg(
        short = 'o',
        long = "output",
        value_name = "FILE",
        help_heading = "Display Options"
    )]
    pub output: Option<String>,

    /// Custom request method
    #[arg(
        short = 'X',
        long = "request",
        value_name = "METHOD",
        help_heading = "HTTP Options"
    )]
    pub method: Option<String>,

    /// Add custom header
    #[arg(
        short = 'H',
        long = "header",
        value_name = "HEADER",
        help_heading = "HTTP Options"
    )]
    pub headers: Vec<String>,

    /// Disable following redirects (redirects are followed by default)
    #[arg(long = "no-redirect", help_heading = "HTTP Options")]
    pub no_redirect: bool,

    /// Maximum time for the request in seconds
    #[arg(
        short = 'm',
        long = "timeout",
        value_name = "SECONDS",
        help_heading = "HTTP Options"
    )]
    pub max_time: Option<u64>,

    /// POST data (use @filename to read from file, @- to read from stdin)
    #[arg(
        short = 'd',
        long = "data",
        value_name = "DATA",
        help_heading = "HTTP Options"
    )]
    pub data: Vec<String>,

    /// Send JSON data with Content-Type header
    #[arg(long = "json", value_name = "JSON", help_heading = "HTTP Options")]
    pub json: Option<String>,

    /// Override RPC URL for the request
    #[arg(
        short = 'r',
        long = "rpc",
        visible_alias = "rpc-url",
        value_name = "URL",
        env = "PRESTO_RPC_URL",
        help_heading = "RPC Options"
    )]
    pub rpc_url: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Make an HTTP request with optional payment
    #[command(alias = "q", display_order = 1, hide = true)]
    Query(Box<QueryArgs>),
    /// Log in to your Tempo wallet
    #[command(display_order = 2)]
    Login,
    /// Log out and disconnect your wallet
    #[command(display_order = 3)]
    Logout {
        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },
    /// Show who you are: wallet, balances, access keys
    #[command(display_order = 4)]
    Whoami,
    /// Alias for whoami
    #[command(hide = true, name = "balance")]
    Balance,
    /// Manage payment sessions
    #[command(display_order = 6)]
    #[command(args_conflicts_with_subcommands = true)]
    Session {
        #[command(subcommand)]
        command: Option<SessionCommands>,
    },
    /// Generate shell completions script
    #[command(hide = true)]
    Completions {
        /// The shell to generate completions for
        #[arg(value_enum)]
        shell: Option<Shell>,
    },
}

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
#[allow(clippy::enum_variant_names)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    #[value(name = "powershell")]
    PowerShell,
}

#[derive(Subcommand, Debug)]
pub enum SessionCommands {
    /// List active payment sessions
    List,
    /// Close a payment session and remove it locally
    Close {
        /// URL or origin to close session for
        url: Option<String>,
        /// Close all active sessions
        #[arg(long)]
        all: bool,
    },
}

impl QueryArgs {
    pub fn parse_headers(&self) -> Vec<(String, String)> {
        let header_map = crate::http::parse_headers(&self.headers);
        header_map.into_iter().collect()
    }

    pub fn get_timeout(&self) -> Option<u64> {
        self.max_time
    }
}

impl Cli {
    pub fn is_verbose(&self) -> bool {
        self.verbosity >= 1
    }

    pub fn should_show_output(&self) -> bool {
        !self.quiet
    }
}

#[cfg(test)]
pub mod test_utils {
    use super::*;
    use clap::Parser;

    pub fn make_cli(args: &[&str]) -> Cli {
        let mut full_args = vec!["presto"];
        full_args.extend(args);
        Cli::parse_from(full_args)
    }

    pub fn make_query_args(args: &[&str]) -> QueryArgs {
        let cli = make_cli(args);
        match cli.command {
            Some(Commands::Query(args)) => *args,
            _ => panic!("Expected Query command"),
        }
    }
}
