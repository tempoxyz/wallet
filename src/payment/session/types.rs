use alloy::primitives::{Address, B256};
use mpp::ChallengeEcho;

use crate::http::request::RequestContext;
use crate::network::Network;

/// Result of a session request — either streamed (already printed) or a buffered response.
pub enum SessionResult {
    /// SSE tokens were streamed directly to stdout.
    Streamed,
    /// A normal (non-SSE) response that should be handled by the regular output path.
    Response(crate::http::HttpResponse),
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
    pub(super) echo: &'a ChallengeEcho,
    pub(super) did: &'a str,
    pub(super) request_ctx: &'a RequestContext,
    pub(super) url: &'a str,
    pub(super) network_name: &'a str,
    pub(super) origin: &'a str,
    pub(super) tick_cost: u128,
    pub(super) deposit: u128,
    pub(super) salt: String,
    pub(super) recipient: String,
    pub(super) currency: String,
}

impl SessionContext<'_> {
    /// Resolve the token symbol for the current session (e.g., "USDC" or "pathUSD").
    pub(super) fn token_symbol(&self) -> &'static str {
        self.network_name
            .parse::<Network>()
            .ok()
            .and_then(|n| n.token_config_by_address(&self.currency))
            .map(|t| t.currency.symbol)
            .unwrap_or("tokens")
    }
}
