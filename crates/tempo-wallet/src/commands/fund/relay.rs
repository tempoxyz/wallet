//! Relay bridge API client — deposit address creation and status polling.

use alloy::primitives::Address;
use anyhow::Context;
use serde::{Deserialize, Serialize};

use tempo_common::error::NetworkError;
use tempo_common::network;

/// Truncate a response body for error messages (max 500 chars).
fn truncate_response(text: &str) -> &str {
    const MAX_LEN: usize = 500;
    &text[..text.floor_char_boundary(MAX_LEN)]
}

// ---------------------------------------------------------------------------
// Source chain configuration
// ---------------------------------------------------------------------------

/// A supported source chain for bridging USDC to Tempo.
#[derive(Debug)]
pub(super) struct SourceChain {
    pub(super) name: &'static str,
    pub(super) chain_id: u64,
    pub(super) usdc_address: &'static str,
    pub(super) relay_api: &'static str,
}

const RELAY_API: &str = "https://api.relay.link";

const SOURCE_CHAINS: &[SourceChain] = &[
    SourceChain {
        name: "Base",
        chain_id: 8453,
        usdc_address: "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913",
        relay_api: RELAY_API,
    },
    SourceChain {
        name: "Ethereum",
        chain_id: 1,
        usdc_address: "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
        relay_api: RELAY_API,
    },
    SourceChain {
        name: "Arbitrum",
        chain_id: 42161,
        usdc_address: "0xaf88d065e77c8cC2239327C5EDb3A432268e5831",
        relay_api: RELAY_API,
    },
    SourceChain {
        name: "Optimism",
        chain_id: 10,
        usdc_address: "0x0b2C639c533813f4Aa9D7837CAf62653d097Ff85",
        relay_api: RELAY_API,
    },
];

/// Returns all supported source chains.
pub(super) fn source_chains() -> &'static [SourceChain] {
    SOURCE_CHAINS
}

// ---------------------------------------------------------------------------
// Deposit address
// ---------------------------------------------------------------------------

/// Result of creating a deposit address via the Relay API.
#[derive(Debug)]
pub(super) struct DepositAddressResult {
    pub(super) deposit_address: String,
    pub(super) request_id: String,
}

/// Creates a deposit address for bridging USDC from a source chain to Tempo.
pub(super) async fn create_deposit_address(
    client: &reqwest::Client,
    source_chain: &SourceChain,
    recipient: &str,
    destination_chain_id: u64,
) -> anyhow::Result<DepositAddressResult> {
    let url = format!("{}/quote/v2", source_chain.relay_api);

    let body = serde_json::json!({
        "user": Address::ZERO.to_string(),
        "originChainId": source_chain.chain_id,
        "originCurrency": source_chain.usdc_address,
        "destinationChainId": destination_chain_id,
        "destinationCurrency": network::USDCE_TOKEN,
        "recipient": recipient,
        "amount": "1000000",
        "tradeType": "EXACT_INPUT",
        "usePermit": false,
        "useExternalLiquidity": true,
        "useDepositAddress": true,
        "referrer": "tempo.xyz",
    });

    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .context("Failed to request deposit address from Relay")?;

    let status = resp.status();
    let text = resp.text().await.context("Failed to read Relay response")?;

    if !status.is_success() {
        let truncated = truncate_response(&text);
        anyhow::bail!(NetworkError::Http(format!(
            "Relay API returned {status}: {truncated}"
        )));
    }

    let json: serde_json::Value =
        serde_json::from_str(&text).context("Failed to parse Relay quote response")?;

    let steps = json["steps"]
        .as_array()
        .context("Missing 'steps' in Relay response")?;

    for step in steps {
        if step["id"].as_str() == Some("deposit") {
            let deposit_address = step["depositAddress"]
                .as_str()
                .context("Missing 'depositAddress' in deposit step")?
                .to_string();
            let request_id = step["requestId"]
                .as_str()
                .context("Missing 'requestId' in deposit step")?
                .to_string();

            return Ok(DepositAddressResult {
                deposit_address,
                request_id,
            });
        }
    }

    anyhow::bail!(NetworkError::Http(
        "No deposit step found in Relay response".to_string()
    ))
}

// ---------------------------------------------------------------------------
// Deposit status polling
// ---------------------------------------------------------------------------

/// Status of a cross-chain deposit tracked by Relay.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct DepositStatus {
    /// One of: waiting, pending, submitted, success, failure, refunded.
    pub(crate) status: String,
    /// Transaction hashes on the source chain.
    #[serde(
        default,
        rename = "inTxHashes",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) in_tx_hashes: Option<Vec<String>>,
    /// Transaction hashes on the destination chain.
    #[serde(default, rename = "txHashes", skip_serializing_if = "Option::is_none")]
    pub(crate) out_tx_hashes: Option<Vec<String>>,
}

/// Polls the Relay intent status API for a given request ID.
pub(super) async fn poll_deposit_status(
    client: &reqwest::Client,
    relay_api: &str,
    request_id: &str,
) -> anyhow::Result<Option<DepositStatus>> {
    let url = format!("{}/intents/status/v3?requestId={}", relay_api, request_id);

    let resp = client
        .get(&url)
        .send()
        .await
        .context("Failed to poll Relay deposit status")?;

    let status = resp.status();
    let text = resp.text().await.context("Failed to read Relay response")?;

    if !status.is_success() {
        let truncated = truncate_response(&text);
        anyhow::bail!(NetworkError::Http(format!(
            "Relay API returned {status}: {truncated}"
        )));
    }

    let deposit_status: DepositStatus =
        serde_json::from_str(&text).context("Failed to parse Relay status response")?;

    if deposit_status.status.is_empty() {
        return Ok(None);
    }

    // Filter out empty tx hash vecs to normalize the response.
    Ok(Some(DepositStatus {
        in_tx_hashes: deposit_status.in_tx_hashes.filter(|v| !v.is_empty()),
        out_tx_hashes: deposit_status.out_tx_hashes.filter(|v| !v.is_empty()),
        ..deposit_status
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_chains_includes_base() {
        let chains = source_chains();
        let base = chains.iter().find(|c| c.chain_id == 8453);
        assert!(base.is_some(), "Base should be a supported source chain");
        assert_eq!(base.unwrap().name, "Base");
    }
}
