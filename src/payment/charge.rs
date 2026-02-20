//! Machine Payments Protocol (MPP) handling for the CLI
//!
//! This module handles the MPP protocol (https://mpp.sh) which uses
//! WWW-Authenticate and Authorization headers for HTTP-native payments.

use anyhow::{Context, Result};

use mpp::{parse_www_authenticate, ChargeRequest};

use crate::config::Config;
use crate::error::{classify_payment_error, map_mpp_validation_error};
use crate::http::{HttpResponse, RequestRuntime};
use crate::network::Network;

/// Prepare an MPP charge payment from a 402 response.
///
/// Parses the challenge, validates it, builds and signs the transaction,
/// and returns the Authorization header value. The caller is responsible
/// for replaying the request with the header (or skipping for dry-run).
pub async fn prepare_charge(
    config: &Config,
    runtime: &RequestRuntime,
    initial_response: &HttpResponse,
) -> Result<String> {
    let www_auth = initial_response
        .get_header("www-authenticate")
        .ok_or_else(|| crate::error::PrestoError::MissingHeader("WWW-Authenticate".to_string()))?;

    let challenge =
        parse_www_authenticate(www_auth).context("Failed to parse WWW-Authenticate header")?;

    let charge_req: ChargeRequest = challenge
        .request
        .decode()
        .context("Failed to parse charge request from challenge")?;
    let network_enum = network_from_charge_request(&charge_req)?;

    if runtime.log_enabled() {
        let explorer = network_enum.info().explorer;
        eprintln!("Challenge ID: {}", challenge.id);
        eprintln!("Payment method: {}", challenge.method);
        eprintln!("Payment intent: {}", challenge.intent);
        if let Some(ref expires) = challenge.expires {
            eprintln!("Expires: {}", expires);
        }
        eprintln!("Amount: {} (atomic units)", charge_req.amount);
        eprintln!(
            "Currency: {}",
            crate::network::format_address_link(&charge_req.currency, explorer.as_ref())
        );
        if let Some(ref recipient) = charge_req.recipient {
            eprintln!(
                "Recipient: {}",
                crate::network::format_address_link(recipient, explorer.as_ref())
            );
        }
    }

    challenge
        .validate_for_charge("tempo")
        .map_err(|e| map_mpp_validation_error(e, &challenge))?;

    // Validate --network constraint if set
    if let Some(ref networks) = runtime.network {
        let allowed: Vec<&str> = networks.split(',').map(|s| s.trim()).collect();
        let network_str = network_enum.as_str();
        anyhow::ensure!(
            allowed.contains(&network_str),
            "Network '{}' not in allowed networks: {:?}",
            network_str,
            allowed
        );
    }

    if runtime.log_enabled() {
        eprintln!("Creating payment credential...");
    }

    // Build the mpp-rs payment provider with presto's wallet and RPC config
    use crate::wallet::signer::load_wallet_signer;
    use mpp::client::PaymentProvider;

    let network_name = network_enum.as_str();
    let signing = load_wallet_signer(network_name)?;
    let network_info = config.resolve_network(network_name)?;

    let provider = mpp::client::TempoProvider::new(signing.signer.clone(), &network_info.rpc_url)
        .map_err(|e| crate::error::PrestoError::InvalidConfig(e.to_string()))?
        .with_signing_mode(signing.signing_mode)
        .with_replace_stuck_transactions(true);

    let credential = provider
        .pay(&challenge)
        .await
        .map_err(classify_payment_error)?;

    let auth_header =
        mpp::format_authorization(&credential).context("Failed to format Authorization header")?;

    if runtime.log_enabled() {
        eprintln!("Authorization header length: {} bytes", auth_header.len());
    }

    Ok(auth_header)
}

/// Derive the network from a charge request's chain ID.
fn network_from_charge_request(req: &ChargeRequest) -> Result<Network> {
    use mpp::protocol::methods::tempo::TempoChargeExt;
    let chain_id = req.chain_id().ok_or_else(|| {
        crate::error::PrestoError::InvalidConfig("Missing chainId in charge request".to_string())
    })?;
    Ok(Network::from_chain_id(chain_id).ok_or_else(|| {
        crate::error::PrestoError::InvalidConfig(format!("Unsupported chainId: {}", chain_id))
    })?)
}
