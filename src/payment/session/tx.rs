//! Transaction building for session payments.
//!
//! Low-level Tempo type-0x76 transaction construction, nonce management,
//! and receipt polling.

use alloy::primitives::{Address, B256, U256};
use alloy::providers::Provider;
use anyhow::{Context, Result};

use mpp::client::tempo::{gas, signing, tx_builder};

use crate::config::Config;
use crate::error::PrestoError;
use crate::http::{HttpResponse, RequestContext};
use crate::network::Network;
use crate::wallet::signer::WalletSigner;

/// Expiring nonce key for Tempo transactions (TIP-1009): `maxUint256`.
/// When using this key, set `valid_before` to a short window in the future
/// (<= ~30s) and a fixed nonce (we use 0) — no on-chain nonce tracking needed.
const EXPIRING_NONCE_KEY: U256 = U256::MAX;

/// Default gas price: 1 gwei.
const DEFAULT_GAS_PRICE: u128 = 1_000_000_000;

/// Default validity window for expiring nonce transactions (in seconds).
const VALID_BEFORE_WINDOW_SECS: u64 = 25;

/// Result of building a Tempo payment from calls.
pub(super) struct TempoPaymentResult {
    pub credential: mpp::PaymentCredential,
    pub tx_bytes: Vec<u8>,
}

/// Parameters controlling nonce behavior for signing Tempo transactions.
struct SignTxParams {
    nonce: u64,
    nonce_key: U256,
    valid_before: Option<u64>,
}

impl SignTxParams {
    /// Build parameters for an expiring-nonce transaction.
    /// Uses `nonce_key = maxUint256`, `nonce = offset`, and `valid_before = now + window`.
    fn expiring(offset: u64) -> Self {
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            nonce: offset,
            nonce_key: EXPIRING_NONCE_KEY,
            valid_before: Some(now_secs + VALID_BEFORE_WINDOW_SECS),
        }
    }
}

/// Resolve gas, estimate, build and sign a Tempo type-0x76 transaction.
async fn resolve_and_sign_tx(
    provider: &alloy::providers::RootProvider<alloy::network::Ethereum>,
    wallet: &WalletSigner,
    chain_id: u64,
    fee_token: Address,
    from: Address,
    calls: Vec<tempo_primitives::transaction::Call>,
    params: SignTxParams,
) -> Result<Vec<u8>> {
    let resolved = gas::resolve_gas(provider, from, DEFAULT_GAS_PRICE, DEFAULT_GAS_PRICE)
        .await
        .map_err(|e| PrestoError::Http(e.to_string()))?;

    let key_auth = wallet.signing_mode.key_authorization();
    let gas_limit = tx_builder::estimate_gas(
        provider,
        from,
        chain_id,
        params.nonce,
        fee_token,
        &calls,
        resolved.max_fee_per_gas,
        resolved.max_priority_fee_per_gas,
        key_auth,
    )
    .await
    .map_err(|e| PrestoError::Signing(e.to_string()))?;

    let tx = tx_builder::build_tempo_tx(tx_builder::TempoTxOptions {
        calls,
        chain_id,
        fee_token,
        nonce: params.nonce,
        nonce_key: params.nonce_key,
        gas_limit,
        max_fee_per_gas: resolved.max_fee_per_gas,
        max_priority_fee_per_gas: resolved.max_priority_fee_per_gas,
        fee_payer: false,
        valid_before: params.valid_before,
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
/// `nonce_offset` is added to the on-chain nonce to allow callers to sequence
/// multiple transactions without waiting for each to confirm.
pub(super) async fn submit_tempo_tx(
    provider: &alloy::providers::RootProvider<alloy::network::Ethereum>,
    wallet: &WalletSigner,
    chain_id: u64,
    fee_token: Address,
    from: Address,
    calls: Vec<tempo_primitives::transaction::Call>,
    nonce_offset: u64,
) -> Result<String> {
    // Use expiring nonces by default to avoid nonce tracking.
    let params = SignTxParams::expiring(nonce_offset);
    let tx_bytes =
        resolve_and_sign_tx(provider, wallet, chain_id, fee_token, from, calls, params).await?;

    let pending = provider
        .send_raw_transaction(&tx_bytes)
        .await
        .context("Failed to broadcast transaction")?;

    Ok(format!("{:#x}", pending.tx_hash()))
}

/// Wait for a Tempo type-0x76 transaction receipt by polling `eth_getTransactionReceipt`.
///
/// Alloy's built-in `get_receipt()` can't deserialize type-0x76 receipts, so we
/// poll the raw JSON and check the `status` field directly.
pub(super) async fn wait_for_tempo_receipt(
    provider: &alloy::providers::RootProvider<alloy::network::Ethereum>,
    tx_hash: B256,
) -> Result<()> {
    let poll_interval = std::time::Duration::from_secs(2);
    let timeout = std::time::Duration::from_secs(120);
    let start = std::time::Instant::now();

    loop {
        let raw: serde_json::Value = provider
            .raw_request(
                "eth_getTransactionReceipt".into(),
                [format!("{:#x}", tx_hash)],
            )
            .await
            .context("Failed to query transaction receipt")?;

        if !raw.is_null() {
            let status = raw.get("status").and_then(|s| s.as_str()).unwrap_or("0x0");
            if status == "0x1" {
                return Ok(());
            } else {
                anyhow::bail!("Channel open transaction reverted on-chain (status={status})");
            }
        }

        if start.elapsed() > timeout {
            anyhow::bail!(
                "Timed out waiting for channel open tx {:#x} after {}s",
                tx_hash,
                timeout.as_secs()
            );
        }

        tokio::time::sleep(poll_interval).await;
    }
}

/// Create a Tempo payment credential from pre-built calls.
///
/// Used by session payments where the calls (e.g., approve + escrow.open)
/// are built externally. Resolves nonce/gas at signing time inside mpp-rs
/// (including stuck-tx detection) and signs with keychain-aware signing mode.
///
/// Returns both the credential (for sending to the server) and the raw
/// signed transaction bytes (for optional client-side pre-broadcast).
pub(super) async fn create_tempo_payment_from_calls(
    config: &Config,
    signing: &WalletSigner,
    challenge: &mpp::PaymentChallenge,
    calls: Vec<tempo_primitives::transaction::Call>,
    fee_token: Address,
    chain_id: u64,
) -> Result<TempoPaymentResult> {
    let network = Network::require_chain_id(chain_id)?;
    let network_info = config.resolve_network(network.as_str())?;

    let rpc_url = Network::parse_rpc_url(&network_info.rpc_url)?;
    let provider = alloy::providers::RootProvider::new_http(rpc_url);

    let from = signing.from;
    // Use expiring nonces for session channel open: no nonce tracking, short validity window.
    let params = SignTxParams::expiring(0);
    let tx_bytes =
        resolve_and_sign_tx(&provider, signing, chain_id, fee_token, from, calls, params).await?;

    let credential = tx_builder::build_charge_credential(challenge, &tx_bytes, chain_id, from);

    Ok(TempoPaymentResult {
        credential,
        tx_bytes,
    })
}

/// Send the Open credential to the server and retry on HTTP 410 while the node indexes.
pub(super) async fn send_open_with_retry(
    request_ctx: &RequestContext,
    url: &str,
    auth_header: &str,
    delays_ms: &[u64],
) -> Result<HttpResponse> {
    let truncate = |s: String| -> String { s.chars().take(500).collect() };

    let headers = vec![("Authorization".to_string(), auth_header.to_string())];
    let resp = request_ctx.execute(url, Some(&headers)).await?;

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
                let next = request_ctx.execute(url, Some(&headers)).await?;
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
