//! CLI argument definitions and parsing.

use clap::{Parser, Subcommand, ValueEnum};

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
    Login {
        /// Do not attempt to open a browser
        #[arg(long)]
        no_browser: bool,
    },
    /// Refresh your access key without logging out
    #[command(display_order = 2)]
    Refresh,
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
    /// List keys and their spending limits
    #[command(display_order = 5, name = "keys")]
    Keys,
    /// Transfer tokens to an address
    #[command(display_order = 6, arg_required_else_help = true)]
    #[command(after_help = "\
Examples:
  tempo wallet transfer 1.00 0x20c0...b50 0x70997...9C8
  tempo wallet transfer 50 0x20c0...b50 0x70997...9C8 --dry-run")]
    Transfer {
        /// Amount in human units ("1.00", "50")
        amount: String,
        /// Token contract address (0x...)
        token: String,
        /// Recipient address (0x...)
        to: String,
        /// Pay fees in a different token (default: same token)
        #[arg(long)]
        fee_token: Option<String>,
        /// Show plan + fee estimate, don't send
        #[arg(long)]
        dry_run: bool,
    },
    /// Fund your wallet (testnet faucet or mainnet bridge)
    #[command(display_order = 7, name = "fund")]
    Fund {
        /// Wallet address to fund (defaults to current wallet)
        #[arg(long)]
        address: Option<String>,
        /// Do not attempt to open a browser
        #[arg(long)]
        no_browser: bool,
    },
    /// Manage payment sessions
    #[command(display_order = 8, name = "sessions")]
    #[command(args_conflicts_with_subcommands = true)]
    Sessions {
        #[command(subcommand)]
        command: Option<SessionCommands>,
    },
    /// Browse the MPP service directory
    #[command(display_order = 9, name = "services")]
    Services {
        #[command(subcommand)]
        command: Option<ServicesCommands>,

        /// Service ID to show details for
        #[arg(value_name = "SERVICE_ID")]
        service_id: Option<String>,

        /// Search by name, description, tags, or category
        #[arg(long, value_name = "QUERY")]
        search: Option<String>,
    },
    /// Issue and manage Tempo wallet-backed cards
    #[command(display_order = 10, name = "cards")]
    #[command(arg_required_else_help = true)]
    Cards {
        #[command(subcommand)]
        command: Option<CardsCommands>,
    },

    /// Collect debug info for support
    #[command(display_order = 11)]
    Debug,

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
    /// List payment sessions
    List {
        /// Include on-chain orphaned discovery and persist discovered channels locally
        #[arg(long)]
        orphaned: bool,
        /// Include local sessions and on-chain orphaned discovery in one view
        #[arg(long)]
        all: bool,
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
        /// Use cooperative close only (no on-chain fallback)
        #[arg(long)]
        cooperative: bool,
        /// Show what would be closed without executing
        #[arg(long)]
        dry_run: bool,
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
}

#[derive(Subcommand, Debug)]
pub(crate) enum CardsCommands {
    /// Configure card API keys
    #[command(name = "config", arg_required_else_help = true)]
    Config {
        #[command(subcommand)]
        command: CardsConfigCommands,
    },
    /// Manage Bridge customers for ToS, KYC, and card endorsement onboarding
    #[command(name = "customers", arg_required_else_help = true)]
    Customers {
        #[command(subcommand)]
        command: CardsCustomerCommands,
    },
    /// Create a virtual Stripe Issuing card backed by a Tempo wallet
    Create {
        /// Stripe Issuing cardholder ID returned after Bridge onboarding
        #[arg(long)]
        cardholder: String,
        /// Tempo wallet address backing the card (defaults to current wallet)
        #[arg(long)]
        wallet_address: Option<String>,
        /// Stripe idempotency key for safe retries (defaults to wallet/cardholder pair)
        #[arg(long)]
        idempotency_key: Option<String>,
        /// Bridge customer ID to store in Stripe metadata
        #[arg(long)]
        bridge_customer_id: Option<String>,
    },
    /// List Stripe Issuing cards
    List {
        /// Only return cards belonging to this cardholder
        #[arg(long)]
        cardholder: Option<String>,
        /// Only return cards with this status
        #[arg(long, value_enum)]
        status: Option<IssuingCardStatus>,
        /// Only return cards with this type
        #[arg(long = "type", value_enum)]
        card_type: Option<IssuingCardType>,
        /// Only return cards with these last four digits
        #[arg(long)]
        last4: Option<String>,
        /// Maximum number of cards to return (1-100)
        #[arg(long)]
        limit: Option<u32>,
        /// Pagination cursor
        #[arg(long)]
        starting_after: Option<String>,
        /// Pagination cursor
        #[arg(long)]
        ending_before: Option<String>,
    },
    /// Retrieve a Stripe Issuing card
    Get {
        /// Stripe Issuing card ID
        id: String,
    },
    /// Update a Stripe Issuing card status
    Update {
        /// Stripe Issuing card ID
        id: String,
        /// New card status
        #[arg(long, value_enum)]
        status: IssuingCardStatus,
        /// Required by Stripe when canceling a lost or stolen card
        #[arg(long, value_enum)]
        cancellation_reason: Option<CardCancellationReason>,
    },
    /// Freeze a Stripe Issuing card by setting status to inactive
    Freeze {
        /// Stripe Issuing card ID
        id: String,
    },
    /// Unfreeze a Stripe Issuing card by setting status to active
    Unfreeze {
        /// Stripe Issuing card ID
        id: String,
    },
    /// Cancel a Stripe Issuing card
    Cancel {
        /// Stripe Issuing card ID
        id: String,
        /// Reason when canceling a lost or stolen card
        #[arg(long, value_enum)]
        cancellation_reason: Option<CardCancellationReason>,
    },
    /// Manage Stripe Issuing cardholders
    #[command(name = "cardholders", arg_required_else_help = true)]
    Cardholders {
        #[command(subcommand)]
        command: CardsCardholderCommands,
    },
    /// Manage Stripe Issuing transactions
    #[command(name = "transactions", arg_required_else_help = true)]
    Transactions {
        #[command(subcommand)]
        command: CardsTransactionCommands,
    },
    /// Manage Stripe Issuing authorizations
    #[command(name = "authorizations", arg_required_else_help = true)]
    Authorizations {
        #[command(subcommand)]
        command: CardsAuthorizationCommands,
    },
    /// Generate card statements
    #[command(name = "statements", arg_required_else_help = true)]
    Statements {
        #[command(subcommand)]
        command: CardsStatementCommands,
    },
    /// Approve the card issuer to spend wallet USDC on Tempo
    Approve {
        /// Amount in human units, or "max" for unlimited allowance
        #[arg(long)]
        amount: String,
        /// Issuer spender address (defaults to Tempo cards issuer on mainnet)
        #[arg(long)]
        spender: Option<String>,
        /// Pay fees in a different token (default: USDC)
        #[arg(long)]
        fee_token: Option<String>,
        /// Show plan without sending a transaction
        #[arg(long)]
        dry_run: bool,
    },
    /// Show current card issuer allowance for wallet USDC
    Allowance {
        /// Issuer spender address (defaults to Tempo cards issuer on mainnet)
        #[arg(long)]
        spender: Option<String>,
        /// Wallet address to query (defaults to current wallet)
        #[arg(long)]
        wallet_address: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum CardsConfigCommands {
    /// Save your Bridge API key
    BridgeApiKey {
        /// Bridge API key (sk-live-... or sk-test-...)
        api_key: String,
    },
    /// Save your Stripe API key
    StripeApiKey {
        /// Stripe API key (sk_live_... or sk_test_...)
        api_key: String,
    },
    /// Show current card configuration
    Show,
}

#[derive(Subcommand, Debug)]
pub(crate) enum CardsCustomerCommands {
    /// Create a new Bridge customer
    Create {
        /// Customer type
        #[arg(long = "type", value_enum, default_value_t = BridgeCustomerType::Individual)]
        customer_type: BridgeCustomerType,
        /// First name
        #[arg(short = 'f', long)]
        first_name: String,
        /// Last name
        #[arg(short = 'l', long)]
        last_name: String,
        /// Email address
        #[arg(short = 'e', long)]
        email: String,
    },
    /// Get a Bridge customer by ID
    Get {
        /// Bridge customer ID
        id: String,
    },
    /// List Bridge customers
    List,
    /// Delete a Bridge customer
    Delete {
        /// Bridge customer ID
        id: String,
    },
    /// Create a hosted ToS acceptance link for new customer creation
    TosLink,
    /// Get a hosted ToS acceptance link for an existing customer
    TosAcceptanceLink {
        /// Bridge customer ID
        id: String,
    },
    /// Get a hosted KYC link for an existing customer
    KycLink {
        /// Bridge customer ID
        id: String,
        /// Endorsement type (for cards, use "cards")
        #[arg(long)]
        endorsement: Option<String>,
        /// Redirect URI after KYC completion
        #[arg(long)]
        redirect_uri: Option<String>,
    },
    /// List transfers for a Bridge customer
    Transfers {
        /// Bridge customer ID
        id: String,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum CardsCardholderCommands {
    /// List Stripe Issuing cardholders
    List {
        /// Only return cardholders with this email
        #[arg(long)]
        email: Option<String>,
        /// Only return cardholders with this status
        #[arg(long, value_enum)]
        status: Option<CardholderStatus>,
        /// Only return cardholders with this type
        #[arg(long = "type", value_enum)]
        cardholder_type: Option<CardholderType>,
        /// Maximum number of cardholders to return (1-100)
        #[arg(long)]
        limit: Option<u32>,
        /// Pagination cursor
        #[arg(long)]
        starting_after: Option<String>,
        /// Pagination cursor
        #[arg(long)]
        ending_before: Option<String>,
    },
    /// Retrieve a Stripe Issuing cardholder
    Get {
        /// Stripe Issuing cardholder ID
        id: String,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum CardsTransactionCommands {
    /// List Stripe Issuing transactions
    List {
        /// Only return transactions for this card
        #[arg(long)]
        card: Option<String>,
        /// Only return transactions for this cardholder
        #[arg(long)]
        cardholder: Option<String>,
        /// Only return transactions with this type
        #[arg(long = "type", value_enum)]
        transaction_type: Option<TransactionType>,
        /// Maximum number of transactions to return (1-100)
        #[arg(long)]
        limit: Option<u32>,
        /// Pagination cursor
        #[arg(long)]
        starting_after: Option<String>,
        /// Pagination cursor
        #[arg(long)]
        ending_before: Option<String>,
    },
    /// Retrieve a Stripe Issuing transaction
    Get {
        /// Stripe Issuing transaction ID
        id: String,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum CardsAuthorizationCommands {
    /// List Stripe Issuing authorizations
    List {
        /// Only return authorizations for this card
        #[arg(long)]
        card: Option<String>,
        /// Only return authorizations for this cardholder
        #[arg(long)]
        cardholder: Option<String>,
        /// Only return authorizations with this status
        #[arg(long, value_enum)]
        status: Option<AuthorizationStatus>,
        /// Maximum number of authorizations to return (1-100)
        #[arg(long)]
        limit: Option<u32>,
        /// Pagination cursor
        #[arg(long)]
        starting_after: Option<String>,
        /// Pagination cursor
        #[arg(long)]
        ending_before: Option<String>,
    },
    /// Retrieve a Stripe Issuing authorization
    Get {
        /// Stripe Issuing authorization ID
        id: String,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum CardsStatementCommands {
    /// Generate a card statement PDF
    Create {
        /// Stripe Issuing cardholder ID
        #[arg(long)]
        cardholder: String,
        /// Stripe Issuing card ID
        #[arg(long)]
        card: String,
        /// Statement period in YYYYMM format
        #[arg(long)]
        period: String,
        /// Path to write the statement PDF, or "-" for stdout
        #[arg(long)]
        output: String,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum BridgeCustomerType {
    Individual,
    Business,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum IssuingCardStatus {
    Active,
    Inactive,
    Canceled,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum IssuingCardType {
    Virtual,
    Physical,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum CardCancellationReason {
    Lost,
    Stolen,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum CardholderStatus {
    Active,
    Inactive,
    Blocked,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum CardholderType {
    Individual,
    Company,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum TransactionType {
    Capture,
    Refund,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum AuthorizationStatus {
    Pending,
    Closed,
    Reversed,
    Expired,
}

impl BridgeCustomerType {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Individual => "individual",
            Self::Business => "business",
        }
    }
}

impl IssuingCardStatus {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Inactive => "inactive",
            Self::Canceled => "canceled",
        }
    }
}

impl IssuingCardType {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Virtual => "virtual",
            Self::Physical => "physical",
        }
    }
}

impl CardCancellationReason {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Lost => "lost",
            Self::Stolen => "stolen",
        }
    }
}

impl CardholderStatus {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Inactive => "inactive",
            Self::Blocked => "blocked",
        }
    }
}

impl CardholderType {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Individual => "individual",
            Self::Company => "company",
        }
    }
}

impl TransactionType {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Capture => "capture",
            Self::Refund => "refund",
        }
    }
}

impl AuthorizationStatus {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Closed => "closed",
            Self::Reversed => "reversed",
            Self::Expired => "expired",
        }
    }
}

impl std::fmt::Display for BridgeCustomerType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
