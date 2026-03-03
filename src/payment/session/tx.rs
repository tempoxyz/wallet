//! Transaction building for session payments.
//!
//! Low-level Tempo type-0x76 transaction construction and receipt polling.
//! All transactions use expiring nonces (nonceKey=MAX, nonce=0) so no
//! on-chain nonce fetch is needed.

use alloy::primitives::{Address, U256};
use alloy::providers::Provider;
use anyhow::Result;

use mpp::client::tempo::{signing, tx_builder};

use crate::config::Config;
use crate::error::PrestoError;
use crate::http::{HttpClient, HttpResponse, RequestContext};
use crate::network::Network;
use crate::wallet::credentials::WalletCredentials;
use crate::wallet::signer::WalletSigner;

/// Static max fee per gas (41 gwei) — Tempo uses a fixed 20 gwei base fee.
const MAX_FEE_PER_GAS: u128 = mpp::client::tempo::MAX_FEE_PER_GAS;

/// Static max priority fee per gas (1 gwei).
const MAX_PRIORITY_FEE_PER_GAS: u128 = mpp::client::tempo::MAX_PRIORITY_FEE_PER_GAS;

/// Expiring nonce key (U256::MAX).
const EXPIRING_NONCE_KEY: U256 = U256::MAX;

/// Validity window (in seconds) for expiring nonce transactions.
const VALID_BEFORE_SECS: u64 = 25;

/// Result of building a Tempo payment from calls.
pub(super) struct TempoPaymentResult {
    pub tx_bytes: Vec<u8>,
}

/// Compute the expiring nonce validity window.
fn expiring_valid_before() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        + VALID_BEFORE_SECS
}

/// Estimate gas, build and sign a Tempo type-0x76 transaction.
///
/// Uses expiring nonces (nonceKey=MAX, nonce=0) and static gas fees
/// (Tempo has a fixed 20 gwei base fee), so only a single RPC call
/// (`eth_estimateGas`) is needed.
async fn resolve_and_sign_tx(
    provider: &alloy::providers::RootProvider<mpp::client::TempoNetwork>,
    wallet: &WalletSigner,
    chain_id: u64,
    fee_token: Address,
    from: Address,
    calls: Vec<tempo_primitives::transaction::Call>,
) -> Result<Vec<u8>> {
    let nonce = 0u64;
    let valid_before = Some(expiring_valid_before());

    let mut key_auth = wallet.signing_mode.key_authorization();

    let gas_result = tx_builder::estimate_gas(
        provider,
        from,
        chain_id,
        nonce,
        fee_token,
        &calls,
        MAX_FEE_PER_GAS,
        MAX_PRIORITY_FEE_PER_GAS,
        key_auth,
        EXPIRING_NONCE_KEY,
        valid_before,
    )
    .await;

    // If gas estimation fails with KeyAlreadyExists, the key is already
    // provisioned on-chain but the local `provisioned` flag is stale.
    // Retry without key_authorization.
    let gas_limit = match gas_result {
        Ok(gas) => gas,
        Err(e) if key_auth.is_some() && e.to_string().contains("KeyAlreadyExists") => {
            key_auth = None;
            // Persist the correction so future transactions skip key_authorization.
            if let Ok(network) = Network::require_chain_id(chain_id) {
                WalletCredentials::mark_provisioned(network.as_str());
            }
            tx_builder::estimate_gas(
                provider,
                from,
                chain_id,
                nonce,
                fee_token,
                &calls,
                MAX_FEE_PER_GAS,
                MAX_PRIORITY_FEE_PER_GAS,
                None,
                EXPIRING_NONCE_KEY,
                valid_before,
            )
            .await
            .map_err(|e| PrestoError::Signing(e.to_string()))?
        }
        Err(e) => return Err(PrestoError::Signing(e.to_string()).into()),
    };

    let tx = tx_builder::build_tempo_tx(tx_builder::TempoTxOptions {
        calls,
        chain_id,
        fee_token,
        nonce,
        nonce_key: EXPIRING_NONCE_KEY,
        gas_limit,
        max_fee_per_gas: MAX_FEE_PER_GAS,
        max_priority_fee_per_gas: MAX_PRIORITY_FEE_PER_GAS,
        fee_payer: false,
        valid_before,
        key_authorization: key_auth.cloned(),
    });

    Ok(
        signing::sign_and_encode_async(tx, &wallet.signer, &wallet.signing_mode)
            .await
            .map_err(|e| PrestoError::Signing(e.to_string()))?,
    )
}

/// Submit a Tempo type-0x76 transaction and return the tx hash.
///
/// Uses expiring nonces so no on-chain nonce fetch is needed.
pub(super) async fn submit_tempo_tx(
    provider: &alloy::providers::RootProvider<mpp::client::TempoNetwork>,
    wallet: &WalletSigner,
    chain_id: u64,
    fee_token: Address,
    from: Address,
    calls: Vec<tempo_primitives::transaction::Call>,
) -> Result<String> {
    let tx_bytes = resolve_and_sign_tx(provider, wallet, chain_id, fee_token, from, calls).await?;

    let pending = provider
        .send_raw_transaction(&tx_bytes)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to broadcast transaction: {e:#}"))?;

    Ok(format!("{:#x}", pending.tx_hash()))
}

/// Create a Tempo payment credential from pre-built calls.
///
/// Used by session payments where the calls (e.g., approve + escrow.open)
/// are built externally. Uses expiring nonces and parallelizes fee
/// resolution with gas estimation.
///
/// Returns both the credential (for sending to the server) and the raw
/// signed transaction bytes (for optional client-side pre-broadcast).
pub(super) async fn create_tempo_payment_from_calls(
    config: &Config,
    signing: &WalletSigner,
    calls: Vec<tempo_primitives::transaction::Call>,
    fee_token: Address,
    chain_id: u64,
) -> Result<TempoPaymentResult> {
    let network = Network::require_chain_id(chain_id)?;
    let network_info = config.resolve_network(network.as_str())?;

    let rpc_url = Network::parse_rpc_url(&network_info.rpc_url)?;
    let provider = alloy::providers::RootProvider::<mpp::client::TempoNetwork>::new_http(rpc_url);

    let from = signing.from;
    let tx_bytes =
        resolve_and_sign_tx(&provider, signing, chain_id, fee_token, from, calls).await?;

    Ok(TempoPaymentResult { tx_bytes })
}

/// Send the Open credential to the server and retry on HTTP 410 while the node indexes.
pub(super) async fn send_open_with_retry(
    request_ctx: &RequestContext,
    http_client: &HttpClient,
    url: &str,
    auth_header: &str,
    delays_ms: &[u64],
) -> Result<HttpResponse> {
    let truncate = |s: String| -> String { s.chars().take(500).collect() };

    let headers = vec![("Authorization".to_string(), auth_header.to_string())];
    let resp = request_ctx
        .execute_with_client(http_client, url, &headers)
        .await?;

    if resp.status_code < 400 {
        return Ok(resp);
    }

    if resp.status_code == 410 {
        let body = resp.body_string().unwrap_or_default();
        if body.contains("channel not funded") || body.contains("Channel Not Found") {
            if request_ctx.log_enabled() {
                eprintln!("Server hasn't indexed channel yet, retrying...");
            }
            for delay in delays_ms {
                tokio::time::sleep(std::time::Duration::from_millis(*delay)).await;
                let next = request_ctx
                    .execute_with_client(http_client, url, &headers)
                    .await?;
                if next.status_code < 400 {
                    return Ok(next);
                }
                if next.status_code != 410 {
                    let nb = next.body_string().unwrap_or_default();
                    anyhow::bail!(
                        "Session open failed: HTTP {} — {}",
                        next.status_code,
                        truncate(nb)
                    );
                }
            }
            anyhow::bail!("Server could not find channel after retries");
        } else {
            anyhow::bail!("Session open failed: HTTP 410 — {}", truncate(body));
        }
    }

    let body = resp.body_string().unwrap_or_default();
    anyhow::bail!(
        "Session open failed: HTTP {} — {}",
        resp.status_code,
        truncate(body)
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expiring_valid_before_is_future() {
        let vb = expiring_valid_before();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        // Must be in the future (now < vb <= now + VALID_BEFORE_SECS)
        assert!(vb > now);
        assert!(vb <= now + VALID_BEFORE_SECS);
    }

    #[test]
    fn test_constants_match_mpp_rs() {
        assert_eq!(MAX_FEE_PER_GAS, 41_000_000_000); // 41 gwei
        assert_eq!(MAX_PRIORITY_FEE_PER_GAS, 1_000_000_000); // 1 gwei
        assert_eq!(EXPIRING_NONCE_KEY, U256::MAX);
    }
}
