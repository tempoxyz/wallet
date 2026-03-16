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

use super::store as session_store;
use crate::{
    analytics::{events, Analytics},
    cli::format::format_token_amount,
    config::Config,
    error::{NetworkError, PaymentError, TempoError},
    keys::Keystore,
};

type ChannelResult<T> = Result<T, TempoError>;

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
    record: &session_store::ChannelRecord,
    config: &Config,
    analytics: Option<&Analytics>,
    keys: &Keystore,
) -> ChannelResult<CloseOutcome> {
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

    let cumulative_amount: u128 = record.cumulative_amount_u128();

    // Try cooperative close via the server first
    let client = reqwest::Client::new();
    match cooperative::try_server_close(
        record,
        &echo,
        &wallet.signer,
        channel_id,
        escrow_contract,
        record.chain_id,
        cumulative_amount,
        &client,
    )
    .await
    {
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
            let amount_display = Some(format_token_amount(
                record.cumulative_amount_u128(),
                network_id,
            ));
            return Ok(CloseOutcome::Closed {
                tx_url,
                amount_display,
            });
        }
        Err(coop_err) => {
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
            let amount_display = Some(format_token_amount(
                record.cumulative_amount_u128(),
                network_id,
            ));
            Ok(CloseOutcome::Closed {
                tx_url,
                amount_display,
            })
        }
        other @ CloseOutcome::Pending { .. } => Ok(other),
    }
}
