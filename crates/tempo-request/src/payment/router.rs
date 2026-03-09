//! Payment routing: route 402 flows to charge or session payment paths.
//!
//! This module is crate-internal and intentionally decoupled from CLI types.

use anyhow::Result;

use mpp::PaymentChallenge;

use crate::http::{HttpClient, HttpResponse};
use tempo_common::config::Config;
use tempo_common::error::PaymentError;
use tempo_common::keys::Keystore;
use tempo_common::network::NetworkId;

use super::charge::handle_charge_request;
use super::session::handle_session_request;

/// Parsed challenge with resolved network, shared by charge and session flows.
pub(crate) struct ResolvedChallenge {
    pub(crate) challenge: PaymentChallenge,
    pub(crate) network_id: NetworkId,
    pub(crate) rpc_url: url::Url,
}

/// Result of a successful payment dispatch.
pub(crate) struct PaymentResult {
    pub(crate) tx_hash: String,
    pub(crate) session_id: Option<String>,
    pub(crate) status_code: u16,
    pub(crate) response: Option<HttpResponse>,
}

/// Dispatch to charge or session payment flow.
///
/// `network` is the already-resolved network from the 402 challenge.
/// The caller is responsible for parsing the challenge and extracting
/// the network before calling this function (see `query/payment_challenge.rs`).
pub(crate) async fn dispatch_payment(
    config: &Config,
    http: &HttpClient,
    is_session: bool,
    url: &str,
    challenge: PaymentChallenge,
    network: NetworkId,
    keys: &Keystore,
) -> Result<PaymentResult> {
    if let Some(allowed) = http.network {
        if allowed != network {
            return Err(PaymentError::InvalidChallenge(format!(
                "Server requested network '{}' but --network is '{}'",
                network, allowed
            ))
            .into());
        }
    }

    let rpc_url = config.rpc_url(network);
    let resolved = ResolvedChallenge {
        challenge,
        network_id: network,
        rpc_url,
    };

    let signer = keys.signer(resolved.network_id)?;

    if is_session {
        return handle_session_request(http, url, resolved, signer, keys).await;
    }

    handle_charge_request(http, url, resolved, signer).await
}
