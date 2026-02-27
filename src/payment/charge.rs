//! MPP charge payment handling.
//!
//! This module handles the MPP protocol (https://mpp.dev) which uses
//! WWW-Authenticate and Authorization headers for HTTP-native payments.

use alloy::primitives::Address;
use anyhow::{Context, Result};
use mpp::client::PaymentProvider;
use mpp::protocol::methods::tempo::TempoChargeExt;
use mpp::{parse_www_authenticate, ChargeRequest};
use tracing::debug;
use zeroize::Zeroizing;

use crate::config::Config;
use crate::error::{classify_payment_error, map_mpp_validation_error, PrestoError};
use crate::http::{HttpResponse, RequestRuntime};
use crate::network::Network;
use crate::wallet::signer::load_wallet_signer;

/// Prepare an MPP charge payment from a 402 response.
///
/// Parses the challenge, validates it, builds and signs the transaction,
/// and returns the Authorization header value. The caller is responsible
/// for replaying the request with the header (or skipping for dry-run).
pub async fn prepare_charge(
    config: &Config,
    runtime: &RequestRuntime,
    initial_response: &HttpResponse,
) -> Result<Zeroizing<String>> {
    let www_auth = initial_response
        .get_header("www-authenticate")
        .ok_or_else(|| PrestoError::MissingHeader("WWW-Authenticate".to_string()))?;

    let challenge =
        parse_www_authenticate(www_auth).context("Failed to parse WWW-Authenticate header")?;

    let charge_req: ChargeRequest = challenge
        .request
        .decode()
        .context("Failed to parse charge request from challenge")?;
    let chain_id = charge_req.chain_id().ok_or_else(|| {
        PrestoError::InvalidConfig("Missing chainId in charge request".to_string())
    })?;
    let network = Network::require_chain_id(chain_id)?;

    challenge
        .validate_for_charge("tempo")
        .map_err(|e| map_mpp_validation_error(e, &challenge))?;

    // Validate --network constraint if set
    if let Some(ref networks) = runtime.network {
        let allowed: Vec<&str> = networks.split(',').map(|s| s.trim()).collect();
        anyhow::ensure!(
            allowed.contains(&network.as_str()),
            "Network '{}' not in allowed networks: {:?}",
            network.as_str(),
            allowed
        );
    }

    if runtime.debug_enabled() {
        eprintln!("Creating payment credential...");
    }

    let network_name = network.as_str();
    let signing = load_wallet_signer(network_name)?;
    let network_info = config.resolve_network(network_name)?;

    let provider = mpp::client::TempoProvider::new(signing.signer.clone(), &network_info.rpc_url)
        .map_err(|e| PrestoError::InvalidConfig(e.to_string()))?
        .with_signing_mode(signing.signing_mode)
        .with_replace_stuck_transactions(true);

    let credential = provider.pay(&challenge).await.map_err(|err| {
        let presto_err = classify_payment_error(err);
        // When we get an InsufficientBalance error from gas estimation,
        // cross-check against a direct balanceOf query to help diagnose
        // whether the wallet truly has 0 balance or the simulation is wrong.
        if matches!(presto_err, PrestoError::InsufficientBalance { .. }) {
            let wallet_addr = signing.from;
            let currency = charge_req.currency_address().ok();
            debug!(
                wallet = %wallet_addr,
                currency = ?currency,
                "InsufficientBalance from gas estimation; \
                 run 'presto whoami' to verify actual on-chain balance"
            );
            if runtime.debug_enabled() {
                eprintln!(
                    "Balance check failed for wallet {} on {}",
                    wallet_addr, network_name,
                );
                if let Some(token_addr) = currency {
                    let rpc_url = network_info.rpc_url.clone();
                    // Fire a quick balanceOf to cross-check
                    let bal = tokio::task::block_in_place(|| {
                        tokio::runtime::Handle::current().block_on(async {
                            query_balance_of(&rpc_url, token_addr, wallet_addr).await
                        })
                    });
                    if let Ok(balance) = bal {
                        eprintln!("Direct balanceOf({}) = {} atomic", wallet_addr, balance);
                    }
                }
            }
        }
        presto_err
    })?;

    let auth_header = Zeroizing::new(
        mpp::format_authorization(&credential).context("Failed to format Authorization header")?,
    );

    if runtime.debug_enabled() {
        eprintln!("Authorization header length: {} bytes", auth_header.len());
    }

    Ok(auth_header)
}

/// Direct `balanceOf` query for diagnostic cross-checking.
async fn query_balance_of(
    rpc_url: &str,
    token: Address,
    account: Address,
) -> anyhow::Result<alloy::primitives::U256> {
    use alloy::providers::ProviderBuilder;
    use alloy::sol;

    sol! {
        #[sol(rpc)]
        interface ITIP20 {
            function balanceOf(address account) external view returns (uint256);
        }
    }

    let url: url::Url = rpc_url.parse()?;
    let provider = ProviderBuilder::new().connect_http(url);
    let contract = ITIP20::new(token, &provider);
    let balance = contract.balanceOf(account).call().await?;
    Ok(balance)
}
