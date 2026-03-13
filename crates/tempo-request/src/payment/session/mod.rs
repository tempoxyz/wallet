//! Session payment request handling.
//!
//! Contains the request-time session logic: flow orchestration,
//! streaming, transaction building, and voucher construction.
//! Session persistence, channel queries, and close operations
//! remain in `tempo_common::payment::session`.

mod flow;
mod open;
mod persist;
mod streaming;
mod voucher;

pub(super) use flow::handle_session_request;

use alloy::primitives::{Address, B256};

use crate::http::HttpClient;
use tempo_common::network::NetworkId;

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
    pub(super) max_pay: Option<u128>,
    pub(super) salt: String,
    pub(super) recipient: Address,
    pub(super) currency: Address,
    /// Shared reqwest client for connection pooling across session requests.
    pub(super) reqwest_client: &'a reqwest::Client,
}

/// Extract the origin (<scheme://host>\[:port\]) from a URL.
fn extract_origin(url: &str) -> String {
    url::Url::parse(url).map_or_else(|_| url.to_string(), |u| u.origin().ascii_serialization())
}
