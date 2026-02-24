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
#[command(after_help = "\
\x1b[1;4mHTTP Options\x1b[0m (for presto <URL>):
  -X, --request <METHOD>        Custom request method (GET, POST, PUT, DELETE, ...)
  -H, --header <HEADER>         Add custom header (e.g. -H 'Accept: text/plain')
  -d, --data <DATA>             POST data (use @filename or @- for stdin)
      --json <JSON>             Send JSON data with Content-Type header
  -m, --timeout <SECONDS>       Maximum time for the request
      --no-redirect             Disable following redirects
  -i, --include                 Include HTTP response headers in output
  -o, --output <FILE>           Write output to file

\x1b[1;4mPayment Options:\x1b[0m
      --dry-run                 Show what would be paid without executing

\x1b[1;4mExamples:\x1b[0m
  # Query Ethereum via Alchemy — no API key needed
  presto https://alchemy.mpp.tempo.xyz/eth-mainnet/v2 \\
    -X POST --json '{\"jsonrpc\":\"2.0\",\"method\":\"eth_blockNumber\",\"params\":[],\"id\":1}' | jq .result

  # Use GPT-4o — no API key, no signup, just pay and go
  presto https://openrouter.mpp.tempo.xyz/v1/chat/completions \\
    -X POST --json '{\"model\":\"openai/gpt-4o-mini\",\"messages\":[{\"role\":\"user\",\"content\":\"Tell me a fun fact\"}]}' \\
    | jq -r '.choices[0].message.content'

  # Search the web — find anything, instantly
  presto https://exa.mpp.tempo.xyz/search \\
    -X POST --json '{\"query\":\"best new developer tools\",\"numResults\":5}' \\
    | jq -r '.results[] | \"\\(.title)\\n  \\(.url)\\n\"'")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Configuration file path
    #[arg(short = 'c', long = "config", value_name = "PATH", global = true)]
    pub config: Option<String>,

    /// Use a specific access key
    #[arg(long = "key", value_name = "NAME", global = true, hide = true)]
    pub key: Option<String>,

    /// Use a private key directly for payment (bypasses wallet login)
    #[arg(
        long = "private-key",
        value_name = "KEY",
        env = "PRESTO_PRIVATE_KEY",
        global = true,
        hide = true,
        hide_env_values = true
    )]
    pub private_key: Option<String>,

    /// Filter to specific networks (comma-separated, e.g. "tempo, tempo-moderato")
    #[arg(short = 'n', long, value_name = "NETWORKS", global = true, hide = true)]
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
        hide = true
    )]
    pub rpc_url: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Make an HTTP request with optional payment
    #[command(alias = "q", display_order = 1, hide = true)]
    Query(Box<QueryArgs>),
    /// Sign up or log in to your Tempo wallet
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
    /// Manage access keys
    #[command(display_order = 5, hide = true)]
    #[command(args_conflicts_with_subcommands = true)]
    Key {
        #[command(subcommand)]
        command: Option<KeyCommands>,
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
    /// Recover a session from on-chain state
    Recover {
        /// URL to recover session for
        url: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum KeyCommands {
    /// List all access keys
    List,
    /// Create a new access key with a generated wallet
    Create {
        /// Name for the new key
        #[arg(long, value_name = "NAME", default_value = "default")]
        name: String,
        /// Overwrite existing key if it already exists
        #[arg(long)]
        force: bool,
    },
    /// Import an existing private key
    Import {
        /// Name for the imported key
        #[arg(long, value_name = "NAME", default_value = "default")]
        name: String,
        /// Overwrite existing key if it already exists
        #[arg(long)]
        force: bool,
        /// Provide the private key directly as hex (use with caution; may appear in shell history)
        #[arg(long = "private-key", value_name = "HEX", conflicts_with = "stdin_key")]
        private_key: Option<String>,
        /// Read the private key from stdin without prompts (non-interactive)
        #[arg(long = "stdin-key", conflicts_with = "private_key")]
        stdin_key: bool,
    },
    /// Rename a key
    Rename {
        /// Current key name
        old: String,
        /// New key name
        new: String,
    },
    /// Delete a key
    Delete {
        /// Key name to delete
        name: String,
        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },
    /// Switch the active key
    Switch {
        /// Key name to switch to
        name: String,
    },
}

impl QueryArgs {
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
