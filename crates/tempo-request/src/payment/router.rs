//! Payment routing: route 402 flows to charge or session payment paths.
//!
//! This module is crate-internal and intentionally decoupled from CLI types.

use mpp::PaymentChallenge;

use crate::http::HttpClient;
use tempo_common::{
    config::Config,
    error::{KeyError, PaymentError, TempoError},
    keys::{Keystore, Signer},
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
    let signer = preflight_signer_key_state(config, keys, resolved.network_id, signer).await?;

    if is_session {
        return handle_session_request(http, url, resolved, signer, keys).await;
    }

    handle_charge_request(http, url, resolved, signer).await
}

async fn preflight_signer_key_state(
    config: &Config,
    keys: &Keystore,
    network: NetworkId,
    signer: Signer,
) -> Result<Signer, TempoError> {
    let Some(entry) = keys.key_for_network(network) else {
        return Ok(signer);
    };
    if keys.ephemeral
        || entry.is_direct_eoa_key()
        || entry.provisioned
        || !signer.has_stored_key_authorization()
    {
        return Ok(signer);
    }

    let Some(wallet_address) = entry.wallet_address_parsed() else {
        return Ok(signer);
    };
    let Some(key_address) = entry.key_address_parsed() else {
        return Ok(signer);
    };

    let provider = alloy::providers::ProviderBuilder::new().connect_http(config.rpc_url(network));
    let token = network.token();

    match mpp::client::tempo::signing::keychain::query_key_spending_limit(
        &provider,
        wallet_address,
        key_address,
        token.address,
    )
    .await
    {
        Ok(_) => {
            let mut persisted = keys.clone();
            if persisted.mark_provisioned_address(wallet_address, network.chain_id()) {
                persisted.save()?;
            }
            Ok(signer)
        }
        Err(mpp::MppError::Tempo(mpp::client::TempoClientError::AccessKeyNotProvisioned)) => {
            signer.with_key_authorization().ok_or_else(|| {
                KeyError::SigningOperation {
                    operation: "key provisioning preflight",
                    reason: "stored key authorization could not be applied to signing mode"
                        .to_string(),
                }
                .into()
            })
        }
        Err(err) => {
            tracing::warn!(
                error = %err,
                "key provisioning preflight failed; attaching stored key authorization without marking provisioned"
            );
            signer.with_key_authorization().ok_or_else(|| {
                KeyError::SigningOperation {
                    operation: "key provisioning preflight",
                    reason: "stored key authorization could not be applied to signing mode"
                        .to_string(),
                }
                .into()
            })
        }
    }
}
