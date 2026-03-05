//! Relay bridge client for funding Tempo wallets from other chains.
//!
//! Provides a lightweight client for the [Relay](https://relay.link) bridge API,
//! used by `tempo-wallet fund` to generate deposit addresses and poll for cross-chain
//! transfer status.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::network::tempo_tokens;

// ---------------------------------------------------------------------------
// Source chain definitions
// ---------------------------------------------------------------------------

/// A supported source chain for bridging USDC to Tempo.
#[derive(Debug)]
pub struct SourceChain {
    pub name: &'static str,
    pub chain_id: u64,
    pub usdc_address: &'static str,
    pub relay_api: &'static str,
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
pub fn source_chains() -> &'static [SourceChain] {
    SOURCE_CHAINS
}

// ---------------------------------------------------------------------------
// Deposit address creation
// ---------------------------------------------------------------------------

/// Result of creating a deposit address via the Relay API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepositAddressResult {
    pub deposit_address: String,
    pub request_id: String,
}

/// Creates a deposit address for bridging USDC from a source chain to Tempo.
pub async fn create_deposit_address(
    client: &reqwest::Client,
    source_chain: &SourceChain,
    recipient: &str,
    destination_chain_id: u64,
) -> Result<DepositAddressResult> {
    let url = format!("{}/quote/v2", source_chain.relay_api);

    let body = serde_json::json!({
        "user": "0x0000000000000000000000000000000000000000",
        "originChainId": source_chain.chain_id,
        "originCurrency": source_chain.usdc_address,
        "destinationChainId": destination_chain_id,
        // Use the canonical USDC address constant from the network module
        "destinationCurrency": tempo_tokens::USDCE,
        "recipient": recipient,
        // 1 USDC (6 decimals) — nominal amount for address generation; the actual
        // bridge amount is determined by how much the user deposits.
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
        let truncated = if text.len() > 500 {
            &text[..500]
        } else {
            &text
        };
        anyhow::bail!("Relay API returned {status}: {truncated}");
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

    anyhow::bail!("No deposit step found in Relay response")
}

// ---------------------------------------------------------------------------
// Deposit status polling
// ---------------------------------------------------------------------------

/// Status of a cross-chain deposit tracked by Relay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepositStatus {
    /// One of: waiting, pending, submitted, success, failure, refunded.
    pub status: String,
    /// Transaction hashes on the source chain.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_tx_hashes: Option<Vec<String>>,
    /// Transaction hashes on the destination chain.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub out_tx_hashes: Option<Vec<String>>,
}

/// Polls the Relay intent status API for a given request ID.
///
/// Uses `/intents/status/v3?requestId=...` as recommended by the Relay docs
/// for deposit-address bridges (where tx hash lookup is not supported).
pub async fn poll_deposit_status(
    client: &reqwest::Client,
    relay_api: &str,
    request_id: &str,
) -> Result<Option<DepositStatus>> {
    let url = format!("{}/intents/status/v3?requestId={}", relay_api, request_id);

    let resp = client
        .get(&url)
        .send()
        .await
        .context("Failed to poll Relay deposit status")?;

    let status = resp.status();
    let text = resp.text().await.context("Failed to read Relay response")?;

    if !status.is_success() {
        let truncated = if text.len() > 500 {
            &text[..500]
        } else {
            &text
        };
        anyhow::bail!("Relay API returned {status}: {truncated}");
    }

    let json: serde_json::Value =
        serde_json::from_str(&text).context("Failed to parse Relay status response")?;

    // Response shape: { status, inTxHashes: [...], txHashes: [...], ... }
    let status_str = match json["status"].as_str() {
        Some(s) => s.to_string(),
        None => return Ok(None),
    };

    let in_tx_hashes = json["inTxHashes"]
        .as_array()
        .map(|txs| {
            txs.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .filter(|v| !v.is_empty());

    // The v3 status API uses `txHashes` for outgoing transaction hashes.
    let out_tx_hashes = json["txHashes"]
        .as_array()
        .map(|txs| {
            txs.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .filter(|v| !v.is_empty());

    Ok(Some(DepositStatus {
        status: status_str,
        in_tx_hashes,
        out_tx_hashes,
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

    #[test]
    fn deposit_status_roundtrip_full() {
        let status = DepositStatus {
            status: "success".to_string(),
            in_tx_hashes: Some(vec!["0xabc".to_string()]),
            out_tx_hashes: Some(vec!["0xdef".to_string()]),
        };
        let json = serde_json::to_string(&status).unwrap();
        let roundtrip: DepositStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtrip.status, "success");
        assert_eq!(roundtrip.in_tx_hashes.as_ref().unwrap(), &["0xabc"]);
        assert_eq!(roundtrip.out_tx_hashes.as_ref().unwrap(), &["0xdef"]);
    }

    #[test]
    fn deposit_status_roundtrip_partial() {
        let status = DepositStatus {
            status: "waiting".to_string(),
            in_tx_hashes: None,
            out_tx_hashes: None,
        };
        let json = serde_json::to_string(&status).unwrap();
        let roundtrip: DepositStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtrip.status, "waiting");
        assert!(roundtrip.in_tx_hashes.is_none());
        assert!(roundtrip.out_tx_hashes.is_none());
    }
}
