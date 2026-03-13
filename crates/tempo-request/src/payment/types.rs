//! Shared types for payment dispatch.

use mpp::PaymentChallenge;
use tempo_common::network::NetworkId;

use crate::http::HttpResponse;

/// Parsed challenge with resolved network, shared by charge and session flows.
pub(crate) struct ResolvedChallenge {
    pub(crate) challenge: PaymentChallenge,
    pub(crate) network_id: NetworkId,
    pub(crate) rpc_url: url::Url,
}

/// Result of a successful payment dispatch.
pub(crate) struct PaymentResult {
    pub(crate) tx_hash: Option<String>,
    pub(crate) session_id: Option<String>,
    pub(crate) status_code: u16,
    pub(crate) response: Option<HttpResponse>,
}
