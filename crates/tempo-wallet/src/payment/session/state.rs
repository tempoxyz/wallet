//! Types and helpers for session payment state.

use alloy::primitives::{Address, B256};

use crate::http::HttpClient;
use crate::network::NetworkId;

/// Outcome of an on-chain close attempt.
pub(crate) enum CloseOutcome {
    /// Channel fully closed (withdrawn or cooperatively settled).
    Closed {
        tx_url: Option<String>,
        /// Formatted settlement amount (e.g., "0.002 USDC"), if available.
        amount_display: Option<String>,
    },
    /// `requestClose()` submitted or already pending; waiting for grace period.
    Pending { remaining_secs: u64 },
}

/// State for an active session channel.
pub(super) struct SessionState {
    pub(super) channel_id: B256,
    pub(super) escrow_contract: Address,
    pub(super) chain_id: u64,
    pub(super) cumulative_amount: u128,
}

/// Shared context for session operations (streaming, closing).
pub(super) struct SessionContext<'a> {
    pub(super) signer: &'a alloy::signers::local::PrivateKeySigner,
    pub(super) echo: &'a mpp::ChallengeEcho,
    pub(super) did: &'a str,
    pub(super) http: &'a HttpClient,
    pub(super) url: &'a str,
    pub(super) network_id: NetworkId,
    pub(super) origin: &'a str,
    pub(super) tick_cost: u128,
    pub(super) deposit: u128,
    pub(super) salt: String,
    pub(super) recipient: String,
    pub(super) currency: String,
    /// Shared reqwest client for connection pooling across session requests.
    pub(super) reqwest_client: &'a reqwest::Client,
}

/// Extract the origin (scheme://host\[:port\]) from a URL.
pub(super) fn extract_origin(url: &str) -> String {
    url::Url::parse(url)
        .map(|u| u.origin().ascii_serialization())
        .unwrap_or_else(|_| url.to_string())
}
