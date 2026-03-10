//! Session management commands.

mod close;
mod info;
mod list;
mod render;
mod sync;

use alloy::primitives::Address;
use anyhow::Result;

use crate::args::SessionCommands;
use tempo_common::cli::context::Context;
use tempo_common::config::Config;
use tempo_common::network::NetworkId;

// Common imports shared by submodules
use tempo_common::payment::session::store as session_store;
use tempo_common::payment::session::store::SessionStatus;
use tempo_common::payment::session::DEFAULT_GRACE_PERIOD_SECS;

/// Check whether a string looks like a channel ID (0x-prefixed, 32-byte hex).
///
/// Returns `true` only when the format is strictly valid hex. Callers that need
/// a detailed error message should use [`validate_channel_id`] instead.
fn is_channel_id(s: &str) -> bool {
    s.starts_with("0x") && s.len() == 66 && s[2..].chars().all(|c| c.is_ascii_hexdigit())
}

/// Validate a channel ID string, returning a user-friendly error for common mistakes.
fn validate_channel_id(s: &str) -> anyhow::Result<()> {
    use tempo_common::security::validate_hex_input;
    validate_hex_input(s, "channel ID")?;
    if s.len() != 66 {
        anyhow::bail!(tempo_common::error::InputError::InvalidHexInput(format!(
            "channel ID must be 66 characters (0x + 64 hex digits), got {}",
            s.len()
        )));
    }
    Ok(())
}

/// Build an Ethereum RPC provider for the given network.
fn make_provider(
    config: &Config,
    network: NetworkId,
) -> alloy::providers::RootProvider<alloy::network::Ethereum> {
    alloy::providers::RootProvider::<alloy::network::Ethereum>::new_http(config.rpc_url(network))
}

/// Resolve the grace period for an escrow contract, falling back to the default.
async fn resolve_grace_period(config: &Config, network: NetworkId, escrow_hex: &str) -> u64 {
    let provider = make_provider(config, network);
    let escrow: Address = match escrow_hex.parse() {
        Ok(a) => a,
        Err(_) => return DEFAULT_GRACE_PERIOD_SECS,
    };
    tempo_common::payment::session::channel::read_grace_period(&provider, escrow)
        .await
        .unwrap_or(DEFAULT_GRACE_PERIOD_SECS)
}

pub(crate) async fn run(ctx: &Context, command: SessionCommands) -> Result<()> {
    match command {
        SessionCommands::List { state } => list::list_sessions(ctx, state).await,
        SessionCommands::Info { target } => info::show_session_info(ctx, &target).await,
        SessionCommands::Close {
            url,
            all,
            orphaned,
            finalize,
            dry_run,
        } => close::close_sessions(ctx, url, all, orphaned, finalize, dry_run).await,
        SessionCommands::Sync { origin } => sync::sync_sessions(ctx, origin.as_deref()).await,
    }
}
