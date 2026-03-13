//! Payment routing: route 402 flows to charge or session payment paths.
//!
//! This module is crate-internal and intentionally decoupled from CLI types.

use mpp::PaymentChallenge;

use crate::http::HttpClient;
use tempo_common::{
    config::Config,
    error::{PaymentError, TempoError},
    keys::Keystore,
    network::NetworkId,
};

use super::{
    charge::handle_charge_request,
    session::handle_session_request,
    types::{PaymentResult, ResolvedChallenge},
};

/// Dispatch to charge or session payment flow.
///
/// `network` is the already-resolved network from the 402 challenge.
/// The caller is responsible for parsing the challenge and extracting
/// the network before calling this function (see `query/challenge.rs`).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn dispatch_payment(
    config: &Config,
    http: &HttpClient,
    is_session: bool,
    url: &str,
    challenge: PaymentChallenge,
    network: NetworkId,
    keys: &Keystore,
    max_pay: Option<u128>,
) -> Result<PaymentResult, TempoError> {
    if let Some(allowed) = http.network {
        if allowed != network {
            return Err(PaymentError::ChallengeSchema {
                context: "payment challenge network",
                reason: format!(
                    "Server requested network '{network}' but --network is '{allowed}'"
                ),
            }
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
        return handle_session_request(http, url, resolved, signer, keys, max_pay).await;
    }

    handle_charge_request(http, url, resolved, signer).await
}
