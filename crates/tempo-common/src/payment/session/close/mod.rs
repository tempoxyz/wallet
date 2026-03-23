//! Channel close operations.
//!
//! Handles closing payment channels via cooperative server close,
//! payer-initiated on-chain close (requestClose → withdraw), and
//! direct channel-by-ID close.

mod cooperative;
mod onchain;

pub use onchain::{close_channel_by_id, close_discovered_channel};

use alloy::primitives::{Address, B256};

use mpp::ChallengeEcho;
use url::Url;

use super::store;
use crate::{
    analytics::{events, Analytics},
    cli::format::format_token_amount,
    config::Config,
    error::{NetworkError, PaymentError, TempoError},
    keys::Keystore,
};

type ChannelResult<T> = Result<T, TempoError>;

fn parse_trusted_close_url(raw_url: &str) -> Option<Url> {
    let trimmed = raw_url.trim();
    if trimmed.is_empty() {
        return None;
    }

    let parsed = Url::parse(trimmed).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    parsed.host_str()?;

    Some(parsed)
}

fn should_attempt_cooperative_close(record: &store::ChannelRecord) -> bool {
    let Some(origin) = parse_trusted_close_url(&record.origin) else {
        return false;
    };

    if record.request_url.trim().is_empty() {
        return true;
    }

    let Some(request_url) = parse_trusted_close_url(&record.request_url) else {
        return false;
    };

    // Keep cooperative close pinned to the persisted origin; if request_url has
    // drifted to a different host, treat metadata as untrusted and use on-chain close.
    request_url.origin().ascii_serialization() == origin.origin().ascii_serialization()
}

/// Outcome of an on-chain close attempt.
pub enum CloseOutcome {
    /// Channel fully closed (withdrawn or cooperatively settled).
    Closed {
        tx_url: Option<String>,
        /// Formatted settlement amount (e.g., "0.002 USDC"), if available.
        amount_display: Option<String>,
    },
    /// `requestClose()` submitted or already pending; waiting for grace period.
    Pending { remaining_secs: u64 },
}

/// Close a channel from a persisted record.
///
/// Used by `tempo-wallet sessions close` to send a close credential to the server.
/// Tries cooperative (server-side) close first, then falls back to on-chain close.
///
/// # Errors
///
/// Returns an error when persisted challenge/session fields are malformed,
/// signer resolution fails, or both cooperative and on-chain close attempts fail.
pub async fn close_channel_from_record(
    record: &store::ChannelRecord,
    config: &Config,
    analytics: Option<&Analytics>,
    keys: &Keystore,
) -> ChannelResult<CloseOutcome> {
    let network_id = record.network_id();
    let wallet = keys.signer(network_id)?;

    let channel_id: B256 = record.channel_id;

    let escrow_contract: Address = record.escrow_contract;

    // Use the server-confirmed accepted amount for the close voucher, not the
    // signing ceiling, to avoid overcharging the payer.
    let close_amount: u128 = record.accepted_cumulative_u128();

    if should_attempt_cooperative_close(record) {
        let echo: ChallengeEcho =
            serde_json::from_str(&record.challenge_echo).map_err(|source| {
                NetworkError::ResponseParse {
                    context: "persisted challenge echo",
                    source,
                }
            })?;

        // Try cooperative close via the server first
        let client = reqwest::Client::new();
        let coop_result = cooperative::try_server_close(
            record,
            &echo,
            &wallet,
            channel_id,
            escrow_contract,
            record.chain_id,
            close_amount,
            &client,
        )
        .await;

        if let Ok(tx_url) = coop_result {
            if let Some(a) = analytics {
                a.track(
                    events::COOP_CLOSE_SUCCESS,
                    crate::analytics::CoopClosePayload {
                        network: network_id.as_str().to_string(),
                        channel_id: record.channel_id_hex(),
                    },
                );
            }
            let amount_display = Some(format_token_amount(close_amount, network_id));
            return Ok(CloseOutcome::Closed {
                tx_url,
                amount_display,
            });
        }

        if let Err(ref coop_err) = coop_result {
            if let Some(a) = analytics {
                a.track(
                    events::COOP_CLOSE_FAILURE,
                    crate::analytics::CoopClosePayload {
                        network: network_id.as_str().to_string(),
                        channel_id: record.channel_id_hex(),
                    },
                );
            }
            tracing::info!("Cooperative close failed: {coop_err:#}");
        }
    } else {
        tracing::info!(
            "Skipping cooperative close for {} due to untrusted persisted endpoint metadata",
            record.channel_id_hex()
        );
    }

    let fee_token: Address =
        record
            .token
            .parse()
            .map_err(|source| PaymentError::ChannelPersistenceSource {
                operation: "parse session token",
                source: Box::new(source),
            })?;

    // Fallback: payer-initiated close (requestClose → withdraw)
    let outcome = onchain::close_on_chain(
        config,
        &wallet,
        channel_id,
        escrow_contract,
        record.chain_id,
        fee_token,
    )
    .await?;

    match outcome {
        CloseOutcome::Closed { tx_url, .. } => {
            let amount_display = Some(format_token_amount(close_amount, network_id));
            Ok(CloseOutcome::Closed {
                tx_url,
                amount_display,
            })
        }
        other @ CloseOutcome::Pending { .. } => Ok(other),
    }
}

/// Close a channel from a persisted record using cooperative close only.
///
/// This mode intentionally does not fall back to on-chain close.
///
/// # Errors
///
/// Returns an error when cooperative close cannot be completed.
pub async fn close_channel_from_record_cooperative(
    record: &store::ChannelRecord,
    analytics: Option<&Analytics>,
    keys: &Keystore,
) -> ChannelResult<CloseOutcome> {
    if !should_attempt_cooperative_close(record) {
        return Err(PaymentError::ChannelPersistence {
            operation: "cooperative close",
            reason: "persisted session does not contain a trusted close URL".to_string(),
        }
        .into());
    }

    let echo: ChallengeEcho = serde_json::from_str(&record.challenge_echo).map_err(|source| {
        NetworkError::ResponseParse {
            context: "persisted challenge echo",
            source,
        }
    })?;

    let network_id = record.network_id();
    let wallet = keys.signer(network_id)?;

    let channel_id: B256 = record.channel_id;
    let escrow_contract: Address = record.escrow_contract;
    let close_amount: u128 = record.accepted_cumulative_u128();

    let client = reqwest::Client::new();
    let tx_url = cooperative::try_server_close(
        record,
        &echo,
        &wallet,
        channel_id,
        escrow_contract,
        record.chain_id,
        close_amount,
        &client,
    )
    .await;

    match tx_url {
        Ok(tx_url) => {
            if let Some(a) = analytics {
                a.track(
                    events::COOP_CLOSE_SUCCESS,
                    crate::analytics::CoopClosePayload {
                        network: network_id.as_str().to_string(),
                        channel_id: record.channel_id_hex(),
                    },
                );
            }
            let amount_display = Some(format_token_amount(close_amount, network_id));
            Ok(CloseOutcome::Closed {
                tx_url,
                amount_display,
            })
        }
        Err(err) => {
            if let Some(a) = analytics {
                a.track(
                    events::COOP_CLOSE_FAILURE,
                    crate::analytics::CoopClosePayload {
                        network: network_id.as_str().to_string(),
                        channel_id: record.channel_id_hex(),
                    },
                );
            }
            Err(err)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::payment::session::store::{ChannelRecord, ChannelStatus};

    fn sample_record(origin: &str, request_url: &str) -> ChannelRecord {
        ChannelRecord {
            version: 1,
            origin: origin.to_string(),
            request_url: request_url.to_string(),
            chain_id: 4217,
            escrow_contract: Address::ZERO,
            token: "0x0000000000000000000000000000000000000000".to_string(),
            payee: "0x0000000000000000000000000000000000000000".to_string(),
            payer: "did:pkh:eip155:4217:0x0000000000000000000000000000000000000000".to_string(),
            authorized_signer: Address::ZERO,
            salt: "0x00".to_string(),
            channel_id: B256::ZERO,
            deposit: 0,
            cumulative_amount: 0,
            accepted_cumulative: 0,
            challenge_echo: "{}".to_string(),
            state: ChannelStatus::Active,
            close_requested_at: 0,
            grace_ready_at: 0,
            created_at: 0,
            last_used_at: 0,
        }
    }

    #[test]
    fn cooperative_close_allowed_for_matching_origin_and_request_url() {
        let record = sample_record("https://api.example.com", "https://api.example.com/v1/chat");
        assert!(should_attempt_cooperative_close(&record));
    }

    #[test]
    fn cooperative_close_rejected_for_empty_origin() {
        let record = sample_record("", "https://api.example.com/v1/chat");
        assert!(!should_attempt_cooperative_close(&record));
    }

    #[test]
    fn cooperative_close_rejected_for_cross_origin_request_url() {
        let record = sample_record(
            "https://api.example.com",
            "https://evil.example.org/v1/chat",
        );
        assert!(!should_attempt_cooperative_close(&record));
    }

    #[test]
    fn cooperative_close_allows_origin_only_records() {
        let record = sample_record("https://api.example.com", "");
        assert!(should_attempt_cooperative_close(&record));
    }
}
