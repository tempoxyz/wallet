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
fn is_channel_id(s: &str) -> bool {
    s.starts_with("0x") && s.len() == 66
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
        } => close::close_sessions(ctx, url, all, orphaned, finalize).await,
        SessionCommands::Sync { origin } => sync::sync_sessions(ctx, origin.as_deref()).await,
    }
}
