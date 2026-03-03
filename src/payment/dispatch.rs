//! Payment dispatch: route 402 flows to charge or session payment paths.
//!
//! This module is crate-internal and intentionally decoupled from CLI types.

use anyhow::{Context, Result};
use mpp::protocol::methods::tempo::session::TempoSessionExt;
use mpp::protocol::methods::tempo::TempoChargeExt;

use mpp::PaymentChallenge;

use crate::config::Config;
use crate::error::PrestoError;
use crate::http::{HttpClient, HttpResponse, RequestContext};
use crate::network::{Network, NetworkInfo};
use crate::wallet::signer::load_wallet_signer;

use super::charge::handle_charge_request;
use super::session::handle_session_request;

/// Parsed challenge with resolved network, shared by charge and session flows.
pub struct ResolvedChallenge {
    pub challenge: PaymentChallenge,
    pub network: Network,
    pub network_info: NetworkInfo,
}

/// Result of a successful payment dispatch.
pub struct PaymentResult {
    pub tx_hash: String,
    pub session_id: Option<String>,
    pub status_code: u16,
    pub response: Option<HttpResponse>,
}

/// Dispatch to charge or session payment flow.
pub async fn dispatch_payment(
    config: &Config,
    request_ctx: &RequestContext,
    http_client: &HttpClient,
    is_session: bool,
    url: &str,
    response: &HttpResponse,
) -> Result<PaymentResult> {
    let www_auth = response
        .get_header("www-authenticate")
        .ok_or_else(|| PrestoError::MissingHeader("WWW-Authenticate".to_string()))?;

    let challenge =
        mpp::parse_www_authenticate(www_auth).context("Failed to parse WWW-Authenticate header")?;

    let chain_id = if challenge.intent.is_charge() {
        challenge
            .request
            .decode::<mpp::ChargeRequest>()
            .ok()
            .and_then(|r| r.chain_id())
    } else if challenge.intent.is_session() {
        challenge
            .request
            .decode::<mpp::SessionRequest>()
            .ok()
            .and_then(|r| r.chain_id())
    } else {
        None
    };

    let chain_id = chain_id.ok_or_else(|| {
        PrestoError::InvalidConfig("Missing chainId in payment request".to_string())
    })?;

    let network = Network::require_chain_id(chain_id)?;

    if let Some(ref networks) = request_ctx.runtime.network {
        let allowed: Vec<&str> = networks.split(',').map(|s| s.trim()).collect();
        anyhow::ensure!(
            allowed.contains(&network.as_str()),
            "Network '{}' not in allowed networks: {:?}",
            network.as_str(),
            allowed
        );
    }

    let network_info = config.resolve_network(network.as_str())?;
    let resolved = ResolvedChallenge {
        challenge,
        network,
        network_info,
    };

    let signer = load_wallet_signer(resolved.network.as_str())?;

    if is_session {
        return handle_session_request(request_ctx, http_client, url, resolved, signer).await;
    }

    handle_charge_request(request_ctx, http_client, url, resolved, signer).await
}
