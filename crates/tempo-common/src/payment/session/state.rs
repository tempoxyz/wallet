//! Types and helpers for session payment state.

use alloy::primitives::{Address, B256};

use crate::http::HttpClient;
use crate::network::NetworkId;

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

/// State for an active session channel.
pub struct SessionState {
    pub channel_id: B256,
    pub escrow_contract: Address,
    pub chain_id: u64,
    pub cumulative_amount: u128,
}

/// Shared context for session operations (streaming, closing).
pub struct SessionContext<'a> {
    pub signer: &'a alloy::signers::local::PrivateKeySigner,
    pub echo: &'a mpp::ChallengeEcho,
    pub did: &'a str,
    pub http: &'a HttpClient,
    pub url: &'a str,
    pub network_id: NetworkId,
    pub origin: &'a str,
    pub tick_cost: u128,
    pub deposit: u128,
    pub salt: String,
    pub recipient: String,
    pub currency: String,
    /// Shared reqwest client for connection pooling across session requests.
    pub reqwest_client: &'a reqwest::Client,
}

/// Extract the origin (scheme://host\[:port\]) from a URL.
pub fn extract_origin(url: &str) -> String {
    url::Url::parse(url)
        .map(|u| u.origin().ascii_serialization())
        .unwrap_or_else(|_| url.to_string())
}
