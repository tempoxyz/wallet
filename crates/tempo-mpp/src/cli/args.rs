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
#[command(about = "MPP session and service operations", long_about = None)]
#[command(version = LONG_VERSION)]
#[command(override_usage = "\n  tempo-mpp <COMMAND> [OPTIONS]")]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    #[command(flatten)]
    pub global: tempo_common::cli::GlobalArgs,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Commands {
    /// Manage payment sessions
    #[command(display_order = 1, name = "sessions")]
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
    #[command(display_order = 2, name = "services")]
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
    pub(crate) fn parse() -> Self {
        match Self::try_parse() {
            Ok(cli) => cli,
            Err(err) => {
                if matches!(err.kind(), clap::error::ErrorKind::DisplayHelp) {
                    err.exit()
                }
                if matches!(err.kind(), clap::error::ErrorKind::DisplayVersion) {
                    let args: Vec<String> = std::env::args().collect();
                    tempo_common::cli::GlobalArgs::emit_structured_version(&args);
                    err.exit()
                }
                err.exit()
            }
        }
    }

    /// Resolve the effective output format: CLI flag > default (text).
    pub(crate) fn resolve_output_format(&self) -> OutputFormat {
        self.global.resolve_output_format()
    }
}
