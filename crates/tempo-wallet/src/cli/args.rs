//! CLI argument definitions and parsing.

use clap::{Parser, Subcommand};

use tempo_common::output::OutputFormat;

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
#[command(name = "tempo wallet")]
#[command(about = "Wallet identity and custody operations", long_about = None)]
#[command(version = LONG_VERSION)]
#[command(override_usage = "\n  tempo wallet <COMMAND> [OPTIONS]")]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    #[command(flatten)]
    pub global: tempo_common::cli::GlobalArgs,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Commands {
    /// Sign up or log in to your Tempo wallet
    #[command(display_order = 1)]
    Login,
    /// Log out and disconnect your wallet
    #[command(display_order = 2)]
    Logout {
        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },
    /// Show who you are: wallet, balances, keys
    #[command(display_order = 3)]
    Whoami,
    /// Manage keys
    #[command(display_order = 4, name = "keys")]
    #[command(
        args_conflicts_with_subcommands = true,
        subcommand_required = true,
        arg_required_else_help = true
    )]
    Keys {
        #[command(subcommand)]
        command: KeyCommands,
    },
    /// List configured wallets
    #[command(display_order = 5, name = "list")]
    List,
    /// Create a new local wallet
    #[command(display_order = 6, name = "create")]
    Create,
    /// Fund your wallet (testnet faucet or mainnet bridge)
    #[command(display_order = 7, name = "fund")]
    Fund {
        /// Wallet address to fund (defaults to current wallet)
        #[arg(long)]
        address: Option<String>,
        /// Skip waiting for balance confirmation
        #[arg(long)]
        no_wait: bool,
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
pub(crate) enum KeyCommands {
    /// List all keys and their spending limits
    List,
    /// Create a new key for a local wallet (generates fresh 30-day key)
    #[command(hide = true)]
    Create {
        /// Wallet address to renew key for (required when multiple local wallets exist)
        #[arg(long)]
        wallet: Option<String>,
    },
    /// Delete keys.toml and reset all local key state
    #[command(hide = true)]
    Clean {
        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
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

    pub(crate) fn resolve_output_format(&self) -> OutputFormat {
        self.global.resolve_output_format()
    }
}
