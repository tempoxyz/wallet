//! CLI argument definitions and parsing.

use clap::{ArgAction, Parser, Subcommand, ValueEnum};

use crate::config::Config;
pub(crate) use crate::config::OutputFormat;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}

/// Long version string including git commit, build date, and profile.
const LONG_VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("PRESTO_GIT_SHA"),
    " ",
    env!("PRESTO_BUILD_DATE"),
    " ",
    env!("PRESTO_BUILD_PROFILE"),
    ")"
);

#[derive(Parser, Debug)]
#[command(name = "presto")]
#[command(about = "A command-line HTTP client with built-in MPP payment support", long_about = None)]
#[command(version = LONG_VERSION)]
#[command(
    // Match curl-style usage: show HTTP options before the URL and list both forms
    override_usage = "\n  presto [HTTP OPTIONS] <URL>\n  presto <COMMAND> [OPTIONS]"
)]
#[command(after_help = "\
\x1b[1;4mHTTP Options\x1b[0m (before <URL>):
  -X, --request <METHOD>        Custom request method (GET, POST, PUT, DELETE, ...)
  -H, --header <HEADER>         Add custom header (e.g. -H 'Accept: text/plain')
  -d, --data <DATA>             POST data (use @filename or @- for stdin)
      --json <JSON>             Send JSON data with Content-Type header
  -m, --timeout <SECONDS>       Maximum time for the request
  -f, --fail                    Fail on HTTP errors (do not output body)
  -o, --output <FILE>           Write output to file
      --dry-run                 Show what would be paid without executing")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

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
        env = "PRESTO_PRIVATE_KEY",
        global = true,
        hide = true,
        hide_env_values = true
    )]
    pub private_key: Option<String>,

    /// Filter to specific networks (comma-separated, e.g. "tempo, tempo-moderato")
    #[arg(short = 'n', long, value_name = "NETWORKS", global = true, hide = true)]
    pub network: Option<String>,

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
}

/// Make an HTTP request with optional payment
#[derive(Parser, Debug)]
#[command(after_help = "\
\x1b[1;4mExamples\x1b[0m:
  presto https://api.example.com/data
  presto -X POST --json '{\"prompt\":\"hello\"}' https://api.example.com/v1/chat
  presto -H 'Accept: text/plain' -o out.txt https://api.example.com/data")]
pub struct QueryArgs {
    /// URL to request
    #[arg(value_name = "URL")]
    pub url: String,

    /// Dry run mode - show what would be paid without executing
    #[arg(long, help_heading = "Payment Options")]
    pub dry_run: bool,

    /// Offline mode - fail immediately without making any network requests
    #[arg(long, help_heading = "HTTP Options")]
    pub offline: bool,

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

    /// Shorthand for HEAD request (fetch headers only)
    #[arg(short = 'I', help_heading = "HTTP Options")]
    pub head: bool,

    /// Add custom header
    #[arg(
        short = 'H',
        long = "header",
        value_name = "HEADER",
        help_heading = "HTTP Options"
    )]
    pub headers: Vec<String>,

    /// Follow redirects (disabled by default)
    #[arg(short = 'L', long = "location", help_heading = "HTTP Options")]
    pub location: bool,

    /// Send data as query parameters with GET
    #[arg(short = 'G', long = "get", help_heading = "HTTP Options")]
    pub get: bool,

    /// Maximum time for the request in seconds
    #[arg(
        short = 'm',
        long = "timeout",
        value_name = "SECONDS",
        help_heading = "HTTP Options"
    )]
    pub max_time: Option<u64>,

    /// Maximum time to establish the TCP connection in seconds
    #[arg(
        long = "connect-timeout",
        value_name = "SECONDS",
        help_heading = "HTTP Options"
    )]
    pub connect_timeout: Option<u64>,

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

    /// Number of retries on transient network errors (timeouts/connect failures)
    #[arg(long = "retries", value_name = "N", help_heading = "HTTP Options")]
    pub retries: Option<u32>,

    /// Initial retry backoff in milliseconds (doubles each retry, capped)
    #[arg(
        long = "retry-backoff",
        value_name = "MILLIS",
        help_heading = "HTTP Options"
    )]
    pub retry_backoff_ms: Option<u64>,
    /// Allow insecure TLS (skip certificate validation)
    #[arg(short = 'k', long = "insecure", help_heading = "HTTP Options")]
    pub insecure: bool,

    /// Fail on HTTP errors (do not output body)
    #[arg(short = 'f', long = "fail", help_heading = "HTTP Options")]
    pub fail_silently: bool,

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

    /// Override the User-Agent header
    #[arg(
        short = 'A',
        long = "user-agent",
        value_name = "STRING",
        help_heading = "HTTP Options"
    )]
    pub user_agent: Option<String>,

    /// Write response headers to a file
    #[arg(
        short = 'D',
        long = "dump-header",
        value_name = "FILE",
        help_heading = "HTTP Options"
    )]
    pub dump_header: Option<String>,

    /// Provide HTTP Basic auth credentials (user:pass)
    #[arg(
        short = 'u',
        long = "user",
        value_name = "USER:PASS",
        help_heading = "HTTP Options"
    )]
    pub user: Option<String>,

    /// Stream response body as it arrives
    #[arg(long = "stream", help_heading = "HTTP Options")]
    pub stream: bool,

    /// Treat response as Server-Sent Events and pass through
    #[arg(long = "sse", help_heading = "HTTP Options")]
    pub sse: bool,

    /// Treat response as SSE and output each event as NDJSON
    #[arg(long = "sse-json", help_heading = "HTTP Options")]
    pub sse_json: bool,

    /// Retry on these HTTP status codes (comma-separated list)
    #[arg(
        long = "retry-http",
        value_name = "CODES",
        help_heading = "HTTP Options"
    )]
    pub retry_http: Option<String>,

    /// Respect Retry-After header for retry delays
    #[arg(long = "retry-after", help_heading = "HTTP Options")]
    pub retry_after: bool,

    /// Add jitter percentage to retry backoff
    #[arg(
        long = "retry-jitter",
        value_name = "PCT",
        help_heading = "HTTP Options"
    )]
    pub retry_jitter: Option<u32>,

    /// Authorization bearer token (alternative to -u Basic auth)
    #[arg(long = "bearer", hide_env_values = true, help_heading = "HTTP Options")]
    pub bearer: Option<String>,

    /// Write response metadata (JSON) to file
    #[arg(
        long = "write-meta",
        value_name = "FILE",
        help_heading = "HTTP Options",
        hide = true
    )]
    pub write_meta: Option<String>,

    /// Fail on HTTP errors but still output the response body
    #[arg(long = "fail-with-body", help_heading = "HTTP Options")]
    pub fail_with_body: bool,

    /// Hard cap the maximum amount to pay (integer of minimal units)
    #[arg(
        long = "max-pay",
        value_name = "AMOUNT",
        help_heading = "Payment Options"
    )]
    pub max_pay: Option<String>,

    /// Currency for --max-pay (symbol or address)
    #[arg(
        long = "currency",
        value_name = "ADDR|SYMBOL",
        help_heading = "Payment Options"
    )]
    pub max_pay_currency: Option<String>,

    /// Save parsed payment receipt to a file (JSON) when available
    #[arg(
        long = "save-receipt",
        value_name = "FILE",
        help_heading = "Payment Options"
    )]
    pub save_receipt: Option<String>,

    /// Output machine-readable price JSON on --dry-run for 402 responses
    #[arg(long = "price-json", help_heading = "Payment Options", hide = true)]
    pub price_json: bool,

    /// Use an HTTP/HTTPS proxy
    #[arg(long = "proxy", value_name = "URL", help_heading = "HTTP Options")]
    pub proxy: Option<String>,

    /// Disable all proxy use
    #[arg(long = "no-proxy", help_heading = "HTTP Options")]
    pub no_proxy: bool,

    /// Maximum redirects when -L is used
    #[arg(long = "max-redirs", value_name = "N", help_heading = "HTTP Options")]
    pub max_redirs: Option<u32>,

    /// Enable HTTP/2 (ALPN)
    #[arg(
        long = "http2",
        help_heading = "HTTP Options",
        conflicts_with = "http1_1"
    )]
    pub http2: bool,

    /// Force HTTP/1.1 only
    #[arg(
        long = "http1.1",
        visible_alias = "http1_1",
        help_heading = "HTTP Options",
        conflicts_with = "http2"
    )]
    pub http1_1: bool,

    /// Set the Referer header
    #[arg(
        short = 'e',
        long = "referer",
        value_name = "URL",
        help_heading = "HTTP Options"
    )]
    pub referer: Option<String>,

    /// Request a compressed response
    #[arg(long = "compressed", help_heading = "HTTP Options")]
    pub compressed: bool,

    /// Save output to a file named after the URL’s last path segment
    #[arg(short = 'O', long = "remote-name", help_heading = "HTTP Options")]
    pub remote_name: bool,

    /// URL-encode a data field (repeatable)
    #[arg(
        long = "data-urlencode",
        value_name = "DATA",
        help_heading = "HTTP Options"
    )]
    pub data_urlencode: Vec<String>,
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
    /// Show who you are: wallet, balances, keys
    #[command(display_order = 4)]
    Whoami,
    /// Alias for whoami
    #[command(hide = true, name = "balance")]
    Balance,
    /// Manage keys
    #[command(display_order = 5, name = "key", hide = true)]
    #[command(args_conflicts_with_subcommands = true)]
    Key {
        #[command(subcommand)]
        command: Option<KeyCommands>,
    },
    /// Manage payment sessions
    #[command(display_order = 6)]
    #[command(args_conflicts_with_subcommands = true)]
    Session {
        #[command(subcommand)]
        command: Option<SessionCommands>,
    },
    /// Manage wallets
    #[command(display_order = 5, hide = true)]
    #[command(args_conflicts_with_subcommands = true)]
    Wallet {
        #[command(subcommand)]
        command: Option<WalletCommands>,
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
    List {
        /// Show all channels: active, orphaned, and closing
        #[arg(long)]
        all: bool,
        /// Scan on-chain for orphaned channels (no local session)
        #[arg(long)]
        orphaned: bool,
        /// Show channels pending finalization (requestClose submitted)
        #[arg(long)]
        closed: bool,
        /// Filter by network (e.g., tempo, tempo-moderato)
        #[arg(long)]
        network: Option<String>,
    },
    /// Close a payment session and remove it locally
    Close {
        /// URL, origin, or channel ID (0x...) to close
        url: Option<String>,
        /// Close all active sessions and on-chain channels
        #[arg(long)]
        all: bool,
        /// Close only orphaned on-chain channels (no local session)
        #[arg(long)]
        orphaned: bool,
        /// Finalize channels pending close (grace period elapsed)
        #[arg(long)]
        closed: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum WalletCommands {
    /// Create a new wallet
    Create {
        /// Create a passkey-based wallet via browser auth
        #[arg(long)]
        passkey: bool,
    },
    /// Fund your wallet (testnet faucet or mainnet bridge)
    Fund {
        /// Wallet address to fund (defaults to current wallet)
        #[arg(long)]
        address: Option<String>,
        /// Skip waiting for balance confirmation
        #[arg(long)]
        no_wait: bool,
    },
    /// Import an existing private key as a local wallet (stores key in OS keychain)
    Import {
        /// Provide the private key directly as hex (use with caution; may appear in shell history)
        #[arg(long = "private-key", value_name = "HEX")]
        private_key: Option<String>,
        /// Read the private key from stdin without prompts (non-interactive)
        #[arg(long = "stdin-key")]
        stdin_key: bool,
    },
    /// Delete a wallet
    Delete {
        /// Wallet address to delete
        #[arg(value_name = "ADDRESS")]
        address: Option<String>,
        /// Delete the passkey wallet
        #[arg(long)]
        passkey: bool,
        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum KeyCommands {
    /// List all keys and their spending limits
    List,
    /// Create a new key for a local wallet (generates fresh 30-day key)
    #[command(hide = true)]
    Create,
    /// Delete keys.toml and reset all local key state
    #[command(hide = true)]
    Clean {
        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },
}

impl Cli {
    /// Verbosity level (0=warn, 1=info, 2=debug, 3+=trace). Returns 0 when silent.
    pub fn verbosity(&self) -> u8 {
        if self.silent || self.verbose == 0 {
            0
        } else if self.verbose == 1 {
            1
        } else if self.verbose == 2 {
            2
        } else {
            3
        }
    }

    /// Whether output should be shown (false when `-q` is used).
    ///
    /// Note: with `WarnLevel`, `-q` maps to `Error` (not `Off`). Treat both
    /// `Off` and `Error` as silent for CLI user-facing logs.
    pub fn should_show_output(&self) -> bool {
        !self.silent
    }

    /// Resolve the effective output format: CLI flag > default (text).
    pub fn resolve_output_format(&self, _config: &Config) -> OutputFormat {
        if self.json_output {
            OutputFormat::Json
        } else {
            OutputFormat::Text
        }
    }
}
