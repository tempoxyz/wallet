//! CLI argument definitions and parsing.

use clap::{Parser, Subcommand, ValueEnum};

use tempo_common::output::OutputFormat;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum SessionStateArg {
    Active,
    Closing,
    Finalizable,
    Orphaned,
    All,
}

/// Long version string including git commit, build date, and profile.
const LONG_VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("TEMPO_GIT_SHA"),
    " ",
    env!("TEMPO_BUILD_DATE"),
    " ",
    env!("TEMPO_BUILD_PROFILE"),
    ")"
);

#[derive(Parser, Debug)]
#[command(name = "tempo-mpp")]
#[command(about = "MPP HTTP/query/session operations", long_about = None)]
#[command(version = LONG_VERSION)]
#[command(
    // Match curl-style usage: show HTTP options before the URL and list both forms
    override_usage = "\n  tempo-mpp [HTTP OPTIONS] <URL>\n  tempo-mpp <COMMAND> [OPTIONS]"
)]
#[command(after_help = "\
HTTP Options (before <URL>):
  -X, --request <METHOD>        Custom request method (GET, POST, PUT, DELETE, ...)
  -H, --header <HEADER>         Add custom header (e.g. -H 'Accept: text/plain')
  -d, --data <DATA>             POST data (use @filename or @- for stdin)
     --json <JSON>             Send JSON data with Content-Type header
     --toon <TOON>             Send TOON data (decoded to JSON) with Content-Type header
  -m, --timeout <SECONDS>       Maximum time for the request
  -o, --output <FILE>           Write output to file
      --dry-run                 Show what would be paid without executing")]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    #[command(flatten)]
    pub global: tempo_common::cli::GlobalArgs,
}

/// Make an HTTP request with optional payment
#[derive(Parser, Debug, Default)]
#[command(after_help = "\
Examples:
  tempo-mpp https://api.example.com/data
  tempo-mpp -X POST --json '{\"prompt\":\"hello\"}' https://api.example.com/v1/chat
  tempo-mpp -H 'Accept: text/plain' -o out.txt https://api.example.com/data")]
pub(crate) struct QueryArgs {
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

    /// Send TOON data (decoded to JSON) with Content-Type header
    #[arg(
        long = "toon",
        value_name = "TOON",
        help_heading = "HTTP Options",
        conflicts_with = "json"
    )]
    pub toon: Option<String>,

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

    /// Hard cap the maximum amount to pay (integer of minimal units)
    #[arg(
        long = "max-pay",
        value_name = "AMOUNT",
        help_heading = "Payment Options"
    )]
    pub max_pay: Option<u128>,

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

impl QueryArgs {
    /// Whether the request should use streaming mode (raw, SSE passthrough, or SSE→NDJSON).
    pub(crate) fn is_streaming(&self) -> bool {
        self.stream || self.sse || self.sse_json
    }
}

#[derive(Subcommand, Debug)]
pub(crate) enum Commands {
    /// Make an HTTP request with optional payment
    #[command(alias = "q", display_order = 1, hide = true)]
    Query(Box<QueryArgs>),
    /// Manage payment sessions
    #[command(display_order = 2, name = "sessions")]
    #[command(
        args_conflicts_with_subcommands = true,
        subcommand_required = true,
        arg_required_else_help = true
    )]
    Sessions {
        #[command(subcommand)]
        command: SessionCommands,
    },
    /// Browse the MPP service directory
    #[command(display_order = 3, name = "services")]
    Services {
        #[command(subcommand)]
        command: Option<ServicesCommands>,

        /// Service ID to show details for (shorthand for `services info <ID>`)
        #[arg(value_name = "SERVICE_ID")]
        service_id: Option<String>,

        /// Filter by category (e.g. ai, search, compute)
        #[arg(long, value_name = "CATEGORY")]
        category: Option<String>,

        /// Search by name, description, or tags
        #[arg(long, value_name = "QUERY")]
        search: Option<String>,
    },

    /// Generate shell completions script
    #[command(hide = true)]
    Completions {
        /// The shell to generate completions for
        #[arg(value_enum)]
        shell: Option<clap_complete::Shell>,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum SessionCommands {
    /// List active payment sessions
    List {
        /// Filter by state (comma-separated or repeatable). Defaults to 'active'. Use 'all' for every state.
        #[arg(long = "state", value_enum, value_delimiter = ',')]
        state: Vec<SessionStateArg>,
    },
    /// Show details for a specific session or channel
    ///
    /// Accepts a URL/origin (shows local session details) or a channel ID (0x...).
    /// For channel IDs, if no network is provided, defaults to Tempo mainnet.
    Info {
        /// URL/origin or channel ID (0x...)
        target: String,
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
        finalize: bool,
    },
    /// Sync local sessions with on-chain state
    ///
    /// Without flags, removes stale local records for settled channels.
    /// With `--origin`, re-syncs close timing for a specific session from
    /// on-chain state. Useful after crashes or manual DB edits.
    Sync {
        /// Re-sync a specific origin's close state from on-chain
        #[arg(long)]
        origin: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum ServicesCommands {
    /// List available services
    List,
    /// Show detailed information about a service
    Info {
        /// Service ID (e.g. openai, anthropic)
        service_id: String,
    },
}

impl Cli {
    /// Parse CLI args, treating a bare URL as an implicit `query` subcommand.
    ///
    /// This allows `tempo-mpp https://example.com` as a shorthand for
    /// `tempo-mpp query https://example.com`, making the primary use case
    /// as frictionless as curl/wget.
    pub(crate) fn parse() -> Self {
        match Self::try_parse() {
            Ok(cli) => cli,
            Err(err) => {
                if matches!(err.kind(), clap::error::ErrorKind::DisplayHelp) {
                    err.exit()
                }

                let args: Vec<String> = std::env::args().collect();

                if matches!(err.kind(), clap::error::ErrorKind::DisplayVersion) {
                    tempo_common::cli::GlobalArgs::emit_structured_version(&args);
                    err.exit()
                }

                Self::try_implicit_query(&args).unwrap_or_else(|| err.exit())
            }
        }
    }

    /// Try re-parsing with an implicit `query` subcommand.
    ///
    /// Allows `tempo-mpp https://example.com` as shorthand for
    /// `tempo-mpp query https://example.com`. Returns `None` when
    /// the args don't look like an implicit query.
    fn try_implicit_query(args: &[String]) -> Option<Self> {
        use clap::CommandFactory;

        let mut subcommands: Vec<String> = Self::command()
            .get_subcommands()
            .flat_map(|c| {
                let mut names = vec![c.get_name().to_string()];
                names.extend(c.get_all_aliases().map(String::from));
                names
            })
            .collect();
        subcommands.push("help".to_string());

        let first_positional = args[1..]
            .iter()
            .find(|a| !a.starts_with('-'))
            .map(|s| s.as_str());

        if first_positional.is_some_and(|p| subcommands.iter().any(|s| s == p)) {
            return None;
        }

        let mut with_query = vec![args[0].clone(), "query".to_string()];
        with_query.extend(args[1..].iter().cloned());

        let cli = Self::try_parse_from(with_query).ok()?;

        if let Some(Commands::Query(ref q)) = cli.command {
            let url = &q.url;
            if !url.contains("://") && !url.contains("localhost") && !url.contains('.') {
                eprintln!(
                    "error: '{url}' is not a tempo-mpp command. \
                     See 'tempo-mpp --help' for a list of available commands."
                );
                tempo_common::exit_codes::ExitCode::InvalidUsage.exit();
            }
        }

        Some(cli)
    }

    /// Resolve the effective output format: CLI flag > default (text).
    pub(crate) fn resolve_output_format(&self) -> OutputFormat {
        self.global.resolve_output_format()
    }

    /// Build a `Verbosity` from CLI flags (silent overrides verbose).
    #[cfg(test)]
    pub(crate) fn verbosity(&self) -> tempo_common::util::Verbosity {
        self.global.verbosity()
    }
}
