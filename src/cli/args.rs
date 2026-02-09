use clap::{Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
pub enum OutputFormat {
    Text,
    Json,
    Yaml,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}

#[derive(Parser, Debug)]
#[command(name = "tempoctl")]
#[command(about = "A wget-like tool for HTTP-based payment requests", long_about = None)]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Configuration file path
    #[arg(short = 'C', long = "config", value_name = "PATH", global = true)]
    pub config: Option<String>,

    /// Filter to specific networks (comma-separated, e.g. "base,base-sepolia")
    #[arg(
        short = 'n',
        long,
        value_name = "NETWORKS",
        env = "TEMPOCTL_NETWORK",
        global = true,
        help_heading = "Payment Options"
    )]
    pub network: Option<String>,

    /// Verbosity level (can be used multiple times: -v, -vv, -vvv)
    #[arg(short = 'v', long = "verbosity", action = clap::ArgAction::Count, global = true, help_heading = "Display Options")]
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

    /// Do not print log messages (aliases: -s, --silent)
    #[arg(
        short = 'q',
        long = "quiet",
        visible_short_alias = 's',
        visible_alias = "silent",
        global = true,
        help_heading = "Display Options"
    )]
    pub quiet: bool,

    /// Format output as JSON (shorthand for --output-format json)
    #[arg(
        long = "json-output",
        visible_alias = "jo",
        global = true,
        help_heading = "Display Options"
    )]
    pub json_output: bool,
}

/// Make an HTTP request with optional payment
#[derive(Parser, Debug)]
pub struct QueryArgs {
    /// URL to request
    #[arg(value_name = "URL")]
    pub url: String,

    /// Maximum amount willing to pay (in atomic units)
    #[arg(
        short = 'M',
        long,
        visible_alias = "max",
        value_name = "AMOUNT",
        env = "TEMPOCTL_MAX_AMOUNT",
        help_heading = "Payment Options"
    )]
    pub max_amount: Option<String>,

    /// Require confirmation before paying
    #[arg(
        short = 'y',
        long,
        env = "TEMPOCTL_CONFIRM",
        help_heading = "Payment Options"
    )]
    pub confirm: bool,

    /// Dry run mode - show what would be paid without executing
    #[arg(short = 'D', long, help_heading = "Payment Options")]
    pub dry_run: bool,

    /// Disable automatic token swaps when you don't have the requested currency
    #[arg(
        long = "no-swap",
        env = "TEMPOCTL_NO_SWAP",
        help_heading = "Payment Options"
    )]
    pub no_swap: bool,

    /// Allow insecure operations (skip TLS verification)
    #[arg(short = 'k', long = "insecure", help_heading = "Request Options")]
    pub insecure: bool,

    /// Include HTTP headers in output
    #[arg(short = 'i', long = "include", help_heading = "Display Options")]
    pub include_headers: bool,

    /// Show only HTTP headers
    #[arg(short = 'I', long = "head", help_heading = "Display Options")]
    pub head_only: bool,

    /// Output format for response
    #[arg(
        long,
        value_name = "FORMAT",
        default_value = "text",
        help_heading = "Display Options"
    )]
    pub output_format: OutputFormat,

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

    /// Set user agent
    #[arg(
        short = 'A',
        long = "user-agent",
        value_name = "AGENT",
        help_heading = "HTTP Options"
    )]
    pub user_agent: Option<String>,

    /// Follow redirects
    #[arg(short = 'L', long = "location", help_heading = "HTTP Options")]
    pub follow_redirects: bool,

    /// Connection timeout in seconds
    #[arg(
        long = "connect-timeout",
        value_name = "SECONDS",
        help_heading = "HTTP Options"
    )]
    pub connect_timeout: Option<u64>,

    /// Maximum time for the request
    #[arg(
        short = 'm',
        long = "max-time",
        value_name = "SECONDS",
        help_heading = "HTTP Options"
    )]
    pub max_time: Option<u64>,

    /// POST data
    #[arg(
        short = 'd',
        long = "data",
        value_name = "DATA",
        help_heading = "HTTP Options"
    )]
    pub data: Option<String>,

    /// Send JSON data with Content-Type header
    #[arg(long = "json", value_name = "JSON", help_heading = "HTTP Options")]
    pub json: Option<String>,

    /// Override RPC URL for the request
    #[arg(
        short = 'r',
        long = "rpc",
        visible_alias = "rpc-url",
        value_name = "URL",
        env = "TEMPOCTL_RPC_URL",
        help_heading = "RPC Options"
    )]
    pub rpc_url: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Make an HTTP request with optional payment
    #[command(alias = "q")]
    Query(Box<QueryArgs>),
    /// Log in to your Tempo wallet
    #[command(alias = "l")]
    Login,
    /// Log out and disconnect your wallet
    Logout {
        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },
    /// Manage configuration
    #[command(alias = "c")]
    #[command(args_conflicts_with_subcommands = true)]
    Config {
        #[command(subcommand)]
        command: Option<ConfigCommands>,

        /// Output format for config display (when no subcommand is given)
        #[arg(long, value_name = "FORMAT", default_value = "text")]
        output_format: OutputFormat,
    },
    /// Show version information
    #[command(alias = "v")]
    Version,
    /// Generate shell completions script
    #[command(alias = "com")]
    Completions {
        /// The shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },
    /// Check wallet balance (uses global --network/-n filter)
    #[command(alias = "b")]
    Balance {
        /// Check balance for specific address (defaults to configured addresses)
        address: Option<String>,
    },
    /// Manage and inspect supported networks
    #[command(alias = "n")]
    #[command(args_conflicts_with_subcommands = true)]
    Networks {
        #[command(subcommand)]
        command: Option<NetworkCommands>,
        /// Output format (when no subcommand is given, same as 'networks list')
        #[arg(long, value_name = "FORMAT", default_value = "text")]
        output_format: OutputFormat,
    },
    /// Inspect payment requirements without executing payment
    Inspect {
        /// URL to inspect
        url: String,
        /// Output format
        #[arg(long, value_name = "FORMAT", default_value = "text")]
        output_format: OutputFormat,
    },
    /// Tempo wallet management
    #[command(alias = "w")]
    #[command(subcommand_required = true, arg_required_else_help = true)]
    Wallet {
        #[command(subcommand)]
        command: WalletCommands,
    },
    /// Manage access keys for Tempo wallet
    #[command(alias = "k")]
    #[command(args_conflicts_with_subcommands = true)]
    Keys {
        #[command(subcommand)]
        command: Option<KeysCommands>,
        /// Output format
        #[arg(long, value_name = "FORMAT", default_value = "text")]
        output_format: OutputFormat,
    },
    /// List available payment services
    #[command(alias = "svc")]
    #[command(args_conflicts_with_subcommands = true)]
    Services {
        #[command(subcommand)]
        command: Option<ServicesCommands>,
        /// Output format
        #[arg(long, value_name = "FORMAT", default_value = "text")]
        output_format: OutputFormat,
        /// Force refresh from server
        #[arg(short = 'r', long)]
        refresh: bool,
    },
    /// Show who you are: wallet, balances, access keys
    Whoami {
        /// Output format
        #[arg(long, value_name = "FORMAT", default_value = "text")]
        output_format: OutputFormat,
    },
}

#[derive(Subcommand, Debug)]
pub enum WalletCommands {
    /// Refresh the access key
    Refresh,
}

#[derive(Subcommand, Debug)]
pub enum KeysCommands {
    /// List all access keys
    List,
    /// Switch to a different access key
    Switch {
        /// Key index to switch to
        index: usize,
    },
    /// Delete an access key
    Delete {
        /// Key index to delete
        index: usize,
    },
}

#[derive(Subcommand, Debug)]
pub enum ServicesCommands {
    /// List all available services
    List {
        /// Force refresh from server
        #[arg(short = 'r', long)]
        refresh: bool,
    },
    /// Show detailed info for a service
    Info {
        /// Service name or alias
        name: String,
    },
}

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
#[allow(clippy::enum_variant_names)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    PowerShell,
}

#[derive(Subcommand, Debug)]
pub enum ConfigCommands {
    /// Get a specific configuration value
    Get {
        /// Configuration key (supports dot notation, e.g., "evm.address")
        key: String,
        /// Output format
        #[arg(long, value_name = "FORMAT", default_value = "text")]
        output_format: OutputFormat,
    },
    /// Validate configuration file
    Validate,
}

#[derive(Subcommand, Debug)]
pub enum NetworkCommands {
    /// List all supported networks
    List {
        /// Output format
        #[arg(long, value_name = "FORMAT", default_value = "text")]
        output_format: OutputFormat,
    },
    /// Show detailed information about a network
    Info {
        /// Network name (e.g., "base", "ethereum")
        network: String,
        /// Output format
        #[arg(long, value_name = "FORMAT", default_value = "text")]
        output_format: OutputFormat,
    },
}

impl QueryArgs {
    pub fn parse_headers(&self) -> Vec<(String, String)> {
        let header_map = crate::http::parse_headers(&self.headers);
        header_map.into_iter().collect()
    }

    pub fn get_timeout(&self) -> Option<u64> {
        self.max_time.or(self.connect_timeout)
    }

    pub fn effective_output_format(&self, json_output: bool) -> OutputFormat {
        if json_output {
            OutputFormat::Json
        } else {
            self.output_format
        }
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
        let mut full_args = vec!["tempoctl"];
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
