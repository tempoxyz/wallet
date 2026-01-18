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
#[command(name = "purl")]
#[command(about = "A curl-like tool for HTTP-based payment requests", long_about = None)]
#[command(version)]
#[command(args_conflicts_with_subcommands = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// URL to request
    #[arg(value_name = "URL")]
    pub url: Option<String>,

    /// Configuration file path
    #[arg(short = 'C', long = "config", value_name = "PATH", global = true)]
    pub config: Option<String>,

    // Payment Options
    /// Maximum amount willing to pay (in atomic units)
    #[arg(
        long,
        value_name = "AMOUNT",
        env = "PURL_MAX_AMOUNT",
        help_heading = "Payment Options"
    )]
    pub max_amount: Option<String>,

    /// Require confirmation before paying
    #[arg(long, env = "PURL_CONFIRM", help_heading = "Payment Options")]
    pub confirm: bool,

    /// Filter to specific networks (comma-separated, e.g. "base,base-sepolia")
    #[arg(
        long,
        value_name = "NETWORKS",
        env = "PURL_NETWORK",
        help_heading = "Payment Options"
    )]
    pub network: Option<String>,

    /// Dry run mode - show what would be paid without executing
    #[arg(long, help_heading = "Payment Options")]
    pub dry_run: bool,

    // Display Options
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

    // HTTP Options
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

    // Wallet Options
    /// Path to encrypted keystore file
    #[arg(
        long = "keystore",
        value_name = "PATH",
        env = "PURL_KEYSTORE",
        help_heading = "Wallet Options"
    )]
    pub keystore: Option<String>,

    /// Password for keystore decryption
    #[arg(
        long = "password",
        value_name = "PASSWORD",
        env = "PURL_PASSWORD",
        help_heading = "Wallet Options"
    )]
    pub password: Option<String>,

    /// Raw private key (hex, for EVM; use keystore for better security)
    #[arg(
        long = "private-key",
        value_name = "KEY",
        env = "PURL_PRIVATE_KEY",
        help_heading = "Wallet Options"
    )]
    pub private_key: Option<String>,

    /// Disable password caching for keystores
    #[arg(
        long = "no-cache-password",
        global = true,
        help_heading = "Wallet Options"
    )]
    pub no_cache_password: bool,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Initialize purl configuration
    #[command(alias = "i")]
    Init {
        /// Force overwrite existing config
        #[arg(short = 'f', long)]
        force: bool,
        /// Skip installing AI tool integrations
        #[arg(long)]
        skip_ai: bool,
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
        /// Show private keys (when no subcommand is given)
        #[arg(long)]
        unsafe_show_private_keys: bool,
    },
    /// Show version information
    #[command(alias = "v")]
    Version,
    /// Manage payment methods (keystores)
    #[command(alias = "m")]
    Method {
        #[command(subcommand)]
        command: MethodCommands,
    },
    /// Generate shell completions script
    #[command(alias = "com")]
    Completions {
        /// The shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },
    /// Check wallet balance
    #[command(alias = "b")]
    Balance {
        /// Check balance for specific address (defaults to configured addresses)
        address: Option<String>,
        /// Filter to specific network
        #[arg(long = "network", short = 'n')]
        network: Option<String>,
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
pub enum MethodCommands {
    /// List available keystores
    List,
    /// Create a new keystore
    New {
        /// Name for the keystore
        #[arg(short = 'n', long, default_value = purl_lib::constants::DEFAULT_KEYSTORE_NAME)]
        name: String,
        /// Generate a new private key
        #[arg(short = 'g', long)]
        generate: bool,
    },
    /// Import a private key into a new keystore
    Import {
        /// Name for the keystore
        #[arg(short = 'n', long, default_value = purl_lib::constants::IMPORTED_KEYSTORE_NAME)]
        name: String,
        /// Private key to import (hex format)
        #[arg(short = 'k', long)]
        private_key: Option<String>,
    },
    /// Show keystore details without private key
    Show {
        /// Name of the keystore (without .json extension)
        name: String,
    },
    /// Verify keystore integrity
    Verify {
        /// Name of the keystore (without .json extension)
        name: String,
    },
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
        /// Network name (e.g., "base", "ethereum", "solana")
        network: String,
        /// Output format
        #[arg(long, value_name = "FORMAT", default_value = "text")]
        output_format: OutputFormat,
    },
}

impl Cli {
    /// Parse custom headers into (name, value) tuples
    pub fn parse_headers(&self) -> Vec<(String, String)> {
        purl_lib::parse_headers(&self.headers)
    }

    /// Get the effective timeout
    pub fn get_timeout(&self) -> Option<u64> {
        self.max_time.or(self.connect_timeout)
    }

    /// Parse allowed networks from the --network flag
    pub fn allowed_networks(&self) -> Option<Vec<String>> {
        self.network
            .as_ref()
            .map(|nets| nets.split(',').map(|s| s.trim().to_string()).collect())
    }

    /// Check if verbose output is enabled
    pub fn is_verbose(&self) -> bool {
        self.verbosity >= 1
    }

    /// Check if output should be shown (not quiet)
    pub fn should_show_output(&self) -> bool {
        !self.quiet
    }
}

#[cfg(test)]
pub mod test_utils {
    use super::*;
    use clap::Parser;

    /// Create a Cli instance from command-line arguments for testing.
    ///
    /// The "purl" program name is automatically prepended.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let cli = make_cli(&["-d", r#"{"key":"value"}"#, "http://example.com"]);
    /// assert!(cli.data.is_some());
    /// ```
    pub fn make_cli(args: &[&str]) -> Cli {
        let mut full_args = vec!["purl"];
        full_args.extend(args);
        Cli::parse_from(full_args)
    }
}
