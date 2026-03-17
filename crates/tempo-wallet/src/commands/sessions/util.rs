//! Shared helpers for session management commands.

use alloy::primitives::{Address, B256};

use super::ChannelStatus;
use tempo_common::{
    config::Config,
    error::{InputError, TempoError},
    network::NetworkId,
    payment::session::DEFAULT_GRACE_PERIOD_SECS,
};

type ChannelResult<T> = std::result::Result<T, TempoError>;

/// Check whether a string looks like a channel ID (0x-prefixed, 32-byte hex).
///
/// Returns `true` only when the format is strictly valid hex. Callers that need
/// a detailed error message should use [`validate_channel_id`] instead.
pub(super) fn is_channel_id(s: &str) -> bool {
    s.starts_with("0x") && s.len() == 66 && s[2..].chars().all(|c| c.is_ascii_hexdigit())
}

/// Validate a channel ID string, returning a user-friendly error for common mistakes.
pub(super) fn validate_channel_id(s: &str) -> ChannelResult<()> {
    use tempo_common::security::validate_hex_input;
    validate_hex_input(s, "channel ID")?;
    if s.len() != 66 {
        return Err(InputError::InvalidChannelIdLength { actual: s.len() }.into());
    }
    Ok(())
}

/// Parse a validated channel ID string into a canonical typed value.
pub(super) fn parse_channel_id(s: &str) -> ChannelResult<B256> {
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
    tempo_common::session::read_grace_period(&provider, escrow)
        .await
        .unwrap_or(DEFAULT_GRACE_PERIOD_SECS)
}

pub(super) fn grace_ready_at(close_requested_at: u64, grace_period: u64) -> u64 {
    if close_requested_at == 0 {
        0
    } else {
        close_requested_at.saturating_add(grace_period)
    }
}

pub(super) fn status_from_close_timing(
    close_requested_at: u64,
    grace_period: u64,
    now: u64,
) -> ChannelStatus {
    if close_requested_at == 0 {
        ChannelStatus::Orphaned
    } else if grace_ready_at(close_requested_at, grace_period) <= now {
        ChannelStatus::Finalizable
    } else {
        ChannelStatus::Closing
    }
}

pub(super) fn normalize_origin(target: &str) -> String {
    url::Url::parse(target)
        .map_or_else(|_| target.to_string(), |u| u.origin().ascii_serialization())
}

#[cfg(test)]
mod tests {
    use super::{grace_ready_at, status_from_close_timing};
    use crate::commands::sessions::ChannelStatus;

    #[test]
    fn status_from_close_timing_classifies_expected_states() {
        let now = 1_000;
        assert_eq!(
            status_from_close_timing(0, 900, now),
            ChannelStatus::Orphaned
        );
        assert_eq!(
            status_from_close_timing(500, 900, now),
            ChannelStatus::Closing
        );
        assert_eq!(
            status_from_close_timing(1, 900, now),
            ChannelStatus::Finalizable
        );
    }

    #[test]
    fn grace_ready_at_handles_zero_close_requested_timestamp() {
        assert_eq!(grace_ready_at(0, 900), 0);
        assert_eq!(grace_ready_at(10, 900), 910);
    }
}
