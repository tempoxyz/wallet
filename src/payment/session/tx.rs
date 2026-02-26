//! Transaction building for session payments.
//!
//! Low-level Tempo type-0x76 transaction construction, nonce management,
//! and receipt polling.

use alloy::primitives::{Address, B256};
use anyhow::{Context, Result};

use crate::config::Config;
use crate::error::PrestoError;
use crate::http::{HttpResponse, RequestContext};
use crate::network::Network;

/// The nonceKey used for client-side session transactions.
pub(super) const SESSION_NONCE_KEY: u64 = 0;

/// NONCE precompile address for querying 2D nonce spaces.
const NONCE_PRECOMPILE: &str = "0x4e4f4e4345000000000000000000000000000000";

/// Result of building a Tempo payment from calls.
pub(super) struct TempoPaymentResult {
    pub credential: mpp::PaymentCredential,
    pub tx_bytes: Vec<u8>,
}

/// Query the on-chain nonce for a specific nonceKey via the NONCE precompile.
///
/// Calls `getNonce(address, uint256)` on the NONCE precompile to get the
/// current nonce for the given account in the specified nonceKey space.
pub(super) async fn get_nonce_for_key(
    provider: &alloy::providers::RootProvider<alloy::network::Ethereum>,
    account: Address,
    nonce_key: u64,
) -> Result<u64> {
    // For nonceKey=0, use the standard account transaction count.
    if nonce_key == 0 {
        use alloy::providers::Provider;
        // Prefer a raw request to avoid type coupling; returns hex string like "0x..."
        let count_hex: String = provider
            .raw_request(
                "eth_getTransactionCount".into(),
                [format!("{:#x}", account), "latest".to_string()],
            )
            .await
            .context("Failed to query transaction count")?;
        let trimmed = count_hex.trim_start_matches("0x");
        let nonce = u64::from_str_radix(trimmed, 16).unwrap_or(0);
        return Ok(nonce);
    }
    use alloy::primitives::{Bytes, U256};
    use alloy::providers::Provider;
    use alloy::sol;
    use alloy::sol_types::SolCall;

    sol! {
        interface INonce {
            function getNonce(address account, uint256 nonceKey) external view returns (uint256);
        }
    }

    let call_data = INonce::getNonceCall {
        account,
        nonceKey: U256::from(nonce_key),
    }
    .abi_encode();

    let nonce_precompile: Address = NONCE_PRECOMPILE
        .parse()
        .context("invalid NONCE precompile address")?;

    let tx = alloy::rpc::types::TransactionRequest::default()
        .to(nonce_precompile)
        .input(Bytes::from(call_data).into());

    let result = provider
        .call(tx)
        .await
        .context("Failed to query nonce precompile")?;
    // Response is a single ABI-encoded uint256
    if result.len() < 32 {
        anyhow::bail!("Nonce precompile returned too few bytes: {}", result.len());
    }
    let nonce_u256 = alloy::primitives::U256::from_be_slice(&result[result.len() - 32..]);

    Ok(nonce_u256.to::<u64>())
}

/// Submit a Tempo type-0x76 transaction and return the tx hash.
///
/// `nonce_offset` is added to the on-chain nonce to allow callers to sequence
/// multiple transactions without waiting for each to confirm.
pub(super) async fn submit_tempo_tx(
    provider: &alloy::providers::RootProvider<alloy::network::Ethereum>,
    wallet: &crate::wallet::signer::WalletSigner,
    chain_id: u64,
    fee_token: Address,
    from: Address,
    calls: Vec<tempo_primitives::transaction::Call>,
    nonce_offset: u64,
) -> Result<String> {
    use alloy::primitives::U256;
    use alloy::providers::Provider;

    // Use simple gas resolution for gas prices only.
    let resolved =
        mpp::client::tempo::gas::resolve_gas(provider, from, 1_000_000_000, 1_000_000_000)
            .await
            .map_err(|e| PrestoError::Http(e.to_string()))?;

    // Query the correct nonce for our nonceKey space via the NONCE precompile.
    let nonce = get_nonce_for_key(provider, from, SESSION_NONCE_KEY).await? + nonce_offset;

    let key_auth = wallet.signing_mode.key_authorization();
    let gas_limit = mpp::client::tempo::tx_builder::estimate_gas(
        provider,
        from,
        chain_id,
        nonce,
        fee_token,
        &calls,
        resolved.max_fee_per_gas,
        resolved.max_priority_fee_per_gas,
        key_auth,
    )
    .await
    .map_err(|e| PrestoError::Signing(e.to_string()))?;

    let tx = mpp::client::tempo::tx_builder::build_tempo_tx(
        mpp::client::tempo::tx_builder::TempoTxOptions {
            calls,
            chain_id,
            fee_token,
            nonce,
            nonce_key: U256::from(SESSION_NONCE_KEY),
            gas_limit,
            max_fee_per_gas: resolved.max_fee_per_gas,
            max_priority_fee_per_gas: resolved.max_priority_fee_per_gas,
            fee_payer: false,
            valid_before: None,
            key_authorization: key_auth.cloned(),
        },
    );

    let tx_bytes = mpp::client::tempo::signing::sign_and_encode_async(
        tx,
        &wallet.signer,
        &wallet.signing_mode,
    )
    .await
    .map_err(|e| PrestoError::Signing(e.to_string()))?;

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
    use alloy::providers::Provider;

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
    signing: &crate::wallet::signer::WalletSigner,
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

    // Resolve gas prices (not nonce — resolve_gas queries nonceKey=0).
    let resolved = mpp::client::tempo::gas::resolve_gas(
        &provider,
        from,
        1_000_000_000, // 1 gwei default max fee
        1_000_000_000, // 1 gwei default priority fee
    )
    .await
    .map_err(|e| PrestoError::Http(e.to_string()))?;

    // Query the correct nonce for our nonceKey space via the NONCE precompile.
    let nonce = get_nonce_for_key(&provider, from, SESSION_NONCE_KEY).await?;

    let key_auth = signing.signing_mode.key_authorization();
    let gas_limit = mpp::client::tempo::tx_builder::estimate_gas(
        &provider,
        from,
        chain_id,
        nonce,
        fee_token,
        &calls,
        resolved.max_fee_per_gas,
        resolved.max_priority_fee_per_gas,
        key_auth,
    )
    .await
    .map_err(|e| PrestoError::Signing(e.to_string()))?;

    // Build and sign the transaction
    let tx = mpp::client::tempo::tx_builder::build_tempo_tx(
        mpp::client::tempo::tx_builder::TempoTxOptions {
            calls,
            chain_id,
            fee_token,
            nonce,
            nonce_key: alloy::primitives::U256::from(SESSION_NONCE_KEY),
            gas_limit,
            max_fee_per_gas: resolved.max_fee_per_gas,
            max_priority_fee_per_gas: resolved.max_priority_fee_per_gas,
            fee_payer: false,
            valid_before: None,
            key_authorization: key_auth.cloned(),
        },
    );

    let tx_bytes = mpp::client::tempo::signing::sign_and_encode_async(
        tx,
        &signing.signer,
        &signing.signing_mode,
    )
    .await
    .map_err(|e| PrestoError::Signing(e.to_string()))?;

    let credential = mpp::client::tempo::tx_builder::build_charge_credential(
        challenge, &tx_bytes, chain_id, from,
    );

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
    let mut headers = vec![("Authorization".to_string(), auth_header.to_string())];
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
                headers[0].1 = auth_header.to_string();
                let next = request_ctx.execute(url, Some(&headers)).await?;
                if next.status_code < 400 {
                    return Ok(next);
                }
                if next.status_code != 410 {
                    let nb = next.body_string().unwrap_or_default();
                    anyhow::bail!(
                        "Session open failed: HTTP {} — {}",
                        next.status_code,
                        nb.chars().take(500).collect::<String>()
                    );
                }
                // else: keep retrying 410s
            }
            anyhow::bail!("Server could not find channel after retries");
        } else {
            anyhow::bail!(
                "Session open failed: HTTP 410 — {}",
                body.chars().take(500).collect::<String>()
            );
        }
    }

    let body = resp.body_string().unwrap_or_default();
    anyhow::bail!(
        "Session open failed: HTTP {} — {}",
        resp.status_code,
        body.chars().take(500).collect::<String>()
    );
}
