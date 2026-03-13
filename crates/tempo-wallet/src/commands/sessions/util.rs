//! Shared helpers for session management commands.

use alloy::primitives::{Address, B256};

use tempo_common::{
    config::Config,
    error::{InputError, TempoError},
    network::NetworkId,
    payment::session::DEFAULT_GRACE_PERIOD_SECS,
};

type SessionResult<T> = std::result::Result<T, TempoError>;

/// Check whether a string looks like a channel ID (0x-prefixed, 32-byte hex).
///
/// Returns `true` only when the format is strictly valid hex. Callers that need
/// a detailed error message should use [`validate_channel_id`] instead.
pub(super) fn is_channel_id(s: &str) -> bool {
    s.starts_with("0x") && s.len() == 66 && s[2..].chars().all(|c| c.is_ascii_hexdigit())
}

/// Validate a channel ID string, returning a user-friendly error for common mistakes.
pub(super) fn validate_channel_id(s: &str) -> SessionResult<()> {
    use tempo_common::security::validate_hex_input;
    validate_hex_input(s, "channel ID")?;
    if s.len() != 66 {
        return Err(InputError::InvalidChannelIdLength { actual: s.len() }.into());
    }
    Ok(())
}

/// Parse a validated channel ID string into a canonical typed value.
pub(super) fn parse_channel_id(s: &str) -> SessionResult<B256> {
    validate_channel_id(s)?;
    s.parse::<B256>()
        .map_err(|_| InputError::InvalidChannelIdFormat.into())
}

/// Build an Ethereum RPC provider for the given network.
pub(super) fn make_provider(
    config: &Config,
    network: NetworkId,
) -> alloy::providers::RootProvider<alloy::network::Ethereum> {
    alloy::providers::RootProvider::<alloy::network::Ethereum>::new_http(config.rpc_url(network))
}

/// Resolve the grace period for an escrow contract, falling back to the default.
pub(super) async fn resolve_grace_period(
    config: &Config,
    network: NetworkId,
    escrow: Address,
) -> u64 {
    let provider = make_provider(config, network);
    tempo_common::payment::session::read_grace_period(&provider, escrow)
        .await
        .unwrap_or(DEFAULT_GRACE_PERIOD_SECS)
}
