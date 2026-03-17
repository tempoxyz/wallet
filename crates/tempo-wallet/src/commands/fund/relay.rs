//! Relay bridge API client — deposit address creation and status polling.

use alloy::primitives::Address;
use serde::{Deserialize, Serialize};

use tempo_common::{
    error::{NetworkError, TempoError},
    network,
};

/// Truncate a response body for error messages (max 500 chars).
fn truncate_response(text: &str) -> &str {
    const MAX_LEN: usize = 500;
    &text[..text.floor_char_boundary(MAX_LEN)]
}

// ---------------------------------------------------------------------------
// VM type
// ---------------------------------------------------------------------------

/// Virtual machine type for a source chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Vm {
    Evm,
    Svm,
}

// ---------------------------------------------------------------------------
// Source chain / token configuration
// ---------------------------------------------------------------------------

/// A supported token on a source chain.
#[derive(Debug)]
pub(super) struct SourceToken {
    pub(super) symbol: &'static str,
    pub(super) address: &'static str,
    pub(super) default: bool,
}

/// A supported source chain for bridging to Tempo.
#[derive(Debug)]
pub(super) struct SourceChain {
    pub(super) name: &'static str,
    pub(super) chain_id: u64,
    pub(super) vm: Vm,
    pub(super) relay_api: &'static str,
    pub(super) tokens: &'static [SourceToken],
}

impl SourceChain {
    /// Returns the default token for this chain (marked `default: true`), or
    /// the first token if none is marked.
    pub(super) fn default_token(&self) -> &SourceToken {
        self.tokens
            .iter()
            .find(|t| t.default)
            .unwrap_or(&self.tokens[0])
    }

    /// Find a token by symbol (case-insensitive).
    pub(super) fn find_token(&self, symbol: &str) -> Option<&SourceToken> {
        let needle = symbol.to_ascii_uppercase();
        self.tokens.iter().find(|t| t.symbol == needle)
    }
}

const RELAY_API: &str = "https://api.relay.link";

/// Solana system program address — used as placeholder `user` for SVM deposit
/// address quotes.
const SVM_ZERO_ADDRESS: &str = "11111111111111111111111111111111";

const SOURCE_CHAINS: &[SourceChain] = &[
    SourceChain {
        name: "Base",
        chain_id: 8453,
        vm: Vm::Evm,
        relay_api: RELAY_API,
        tokens: &[
            SourceToken {
                symbol: "USDC",
                address: "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913",
                default: true,
            },
            SourceToken {
                symbol: "ETH",
                address: "0x0000000000000000000000000000000000000000",
                default: false,
            },
            SourceToken {
                symbol: "WETH",
                address: "0x4200000000000000000000000000000000000006",
                default: false,
            },
            SourceToken {
                symbol: "cbBTC",
                address: "0xcbb7c0000ab88b473b1f5afd9ef808440eed33bf",
                default: false,
            },
            SourceToken {
                symbol: "SOL",
                address: "0x311935cd80b76769bf2ecc9d8ab7635b2139cf82",
                default: false,
            },
            SourceToken {
                symbol: "WBTC",
                address: "0x0555e30da8f98308edb960aa94c0db47230d2b9c",
                default: false,
            },
        ],
    },
    SourceChain {
        name: "Ethereum",
        chain_id: 1,
        vm: Vm::Evm,
        relay_api: RELAY_API,
        tokens: &[
            SourceToken {
                symbol: "USDC",
                address: "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
                default: true,
            },
            SourceToken {
                symbol: "ETH",
                address: "0x0000000000000000000000000000000000000000",
                default: false,
            },
            SourceToken {
                symbol: "USDT",
                address: "0xdac17f958d2ee523a2206206994597c13d831ec7",
                default: false,
            },
            SourceToken {
                symbol: "WETH",
                address: "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2",
                default: false,
            },
            SourceToken {
                symbol: "WBTC",
                address: "0x2260fac5e5542a773aa44fbcfedf7c193bc2c599",
                default: false,
            },
        ],
    },
    SourceChain {
        name: "Solana",
        chain_id: 792703809,
        vm: Vm::Svm,
        relay_api: RELAY_API,
        tokens: &[
            SourceToken {
                symbol: "SOL",
                address: "11111111111111111111111111111111",
                default: false,
            },
            SourceToken {
                symbol: "USDC",
                address: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
                default: true,
            },
        ],
    },
    SourceChain {
        name: "Optimism",
        chain_id: 10,
        vm: Vm::Evm,
        relay_api: RELAY_API,
        tokens: &[
            SourceToken {
                symbol: "USDC",
                address: "0x0b2C639c533813f4Aa9D7837CAf62653d097Ff85",
                default: true,
            },
            SourceToken {
                symbol: "ETH",
                address: "0x0000000000000000000000000000000000000000",
                default: false,
            },
            SourceToken {
                symbol: "WETH",
                address: "0x4200000000000000000000000000000000000006",
                default: false,
            },
            SourceToken {
                symbol: "USDT",
                address: "0x94b008aa00579c1307b0ef2c499ad98a8ce58e58",
                default: false,
            },
        ],
    },
    SourceChain {
        name: "Unichain",
        chain_id: 130,
        vm: Vm::Evm,
        relay_api: RELAY_API,
        tokens: &[
            SourceToken {
                symbol: "USDC",
                address: "0x078d782b760474a361dda0af3839290b0ef57ad6",
                default: true,
            },
            SourceToken {
                symbol: "ETH",
                address: "0x0000000000000000000000000000000000000000",
                default: false,
            },
        ],
    },
    SourceChain {
        name: "Abstract",
        chain_id: 2741,
        vm: Vm::Evm,
        relay_api: RELAY_API,
        tokens: &[
            SourceToken {
                symbol: "USDC",
                address: "0x84a71ccd554cc1b02749b35d22f684cc8ec987e1",
                default: true,
            },
            SourceToken {
                symbol: "ETH",
                address: "0x0000000000000000000000000000000000000000",
                default: false,
            },
        ],
    },
    SourceChain {
        name: "Arbitrum",
        chain_id: 42161,
        vm: Vm::Evm,
        relay_api: RELAY_API,
        tokens: &[
            SourceToken {
                symbol: "USDC",
                address: "0xaf88d065e77c8cC2239327C5EDb3A432268e5831",
                default: true,
            },
            SourceToken {
                symbol: "ETH",
                address: "0x0000000000000000000000000000000000000000",
                default: false,
            },
            SourceToken {
                symbol: "USDT",
                address: "0xfd086bc7cd5c481dcc9c85ebe478a1c0b69fcbb9",
                default: false,
            },
        ],
    },
];

/// Returns all supported source chains.
pub(super) const fn source_chains() -> &'static [SourceChain] {
    SOURCE_CHAINS
}

/// Find a source chain by name (case-insensitive).
pub(super) fn find_chain(name: &str) -> Option<&'static SourceChain> {
    let needle = name.to_ascii_lowercase();
    SOURCE_CHAINS
        .iter()
        .find(|c| c.name.to_ascii_lowercase() == needle)
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

/// Creates a deposit address for bridging tokens from a source chain to Tempo.
pub(super) async fn create_deposit_address(
    client: &reqwest::Client,
    source_chain: &SourceChain,
    origin_currency: &str,
    recipient: &str,
    destination_chain_id: u64,
) -> Result<DepositAddressResult, TempoError> {
    let url = format!("{}/quote/v2", source_chain.relay_api);

    let user = match source_chain.vm {
        Vm::Evm => Address::ZERO.to_string(),
        Vm::Svm => SVM_ZERO_ADDRESS.to_string(),
    };

    let origin_currency_value = match source_chain.vm {
        Vm::Evm => origin_currency.to_ascii_lowercase(),
        Vm::Svm => origin_currency.to_string(),
    };

    let body = serde_json::json!({
        "user": user,
        "originChainId": source_chain.chain_id,
        "originCurrency": origin_currency_value,
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
        .map_err(NetworkError::Reqwest)?;

    let status = resp.status();
    let text = resp.text().await.map_err(NetworkError::Reqwest)?;

    if !status.is_success() {
        let truncated = truncate_response(&text);
        return Err(NetworkError::HttpStatus {
            operation: "request relay quote",
            status: status.as_u16(),
            body: Some(truncated.to_string()),
        }
        .into());
    }

    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|source| NetworkError::ResponseParse {
            context: "relay quote response",
            source,
        })?;

    let steps = json["steps"]
        .as_array()
        .ok_or(NetworkError::ResponseMissingField {
            context: "relay quote response",
            field: "steps",
        })?;

    for step in steps {
        if step["id"].as_str() == Some("deposit") {
            let deposit_address = step["depositAddress"]
                .as_str()
                .ok_or(NetworkError::ResponseMissingField {
                    context: "relay quote response",
                    field: "depositAddress in deposit step",
                })?
                .to_string();
            let request_id = step["requestId"]
                .as_str()
                .ok_or(NetworkError::ResponseMissingField {
                    context: "relay quote response",
                    field: "requestId in deposit step",
                })?
                .to_string();

            return Ok(DepositAddressResult {
                deposit_address,
                request_id,
            });
        }
    }

    Err(NetworkError::ResponseMissingEntry {
        context: "relay quote response",
        entry: "deposit step",
    }
    .into())
}

// ---------------------------------------------------------------------------
// Deposit status polling
// ---------------------------------------------------------------------------

/// Status of a cross-chain deposit tracked by Relay.
#[derive(Debug, Serialize, Deserialize)]
pub(super) struct DepositStatus {
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
) -> Result<Option<DepositStatus>, TempoError> {
    let url = format!("{relay_api}/intents/status/v3?requestId={request_id}");

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(NetworkError::Reqwest)?;

    let status = resp.status();
    let text = resp.text().await.map_err(NetworkError::Reqwest)?;

    if !status.is_success() {
        let truncated = truncate_response(&text);
        return Err(NetworkError::HttpStatus {
            operation: "poll relay status",
            status: status.as_u16(),
            body: Some(truncated.to_string()),
        }
        .into());
    }

    let deposit_status: DepositStatus =
        serde_json::from_str(&text).map_err(|source| NetworkError::ResponseParse {
            context: "relay status response",
            source,
        })?;

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

    #[test]
    fn source_chains_includes_solana() {
        let chains = source_chains();
        let solana = chains.iter().find(|c| c.chain_id == 792703809);
        assert!(
            solana.is_some(),
            "Solana should be a supported source chain"
        );
        assert_eq!(solana.unwrap().vm, Vm::Svm);
    }

    #[test]
    fn find_chain_case_insensitive() {
        assert!(find_chain("base").is_some());
        assert!(find_chain("BASE").is_some());
        assert!(find_chain("Solana").is_some());
        assert!(find_chain("nonexistent").is_none());
    }

    #[test]
    fn default_token_is_usdc_on_base() {
        let base = find_chain("base").unwrap();
        assert_eq!(base.default_token().symbol, "USDC");
    }

    #[test]
    fn find_token_case_insensitive() {
        let base = find_chain("base").unwrap();
        assert!(base.find_token("eth").is_some());
        assert!(base.find_token("ETH").is_some());
        assert!(base.find_token("nonexistent").is_none());
    }
}
