//! x402 v2 challenge parsing.
//!
//! Decodes the `PAYMENT-REQUIRED` header (base64 → JSON) and selects the
//! first `exact` EVM EIP-3009 payment option.

use base64::{engine::general_purpose::STANDARD, Engine};

use crate::http::HttpResponse;
use tempo_common::error::{PaymentError, TempoError};

use super::types::{X402PaymentOption, X402PaymentRequired};

/// A selected payment option with the parsed destination chain ID.
#[derive(Debug)]
pub(super) struct SelectedOption {
    pub(super) challenge: X402PaymentRequired,
    /// The raw `serde_json::Value` of the selected `accepts[]` entry,
    /// preserved for echoing back in the `PAYMENT-SIGNATURE` header.
    pub(super) accepted_value: serde_json::Value,
    pub(super) option: X402PaymentOption,
    pub(super) dest_chain_id: u64,
}

/// Check whether a 402 response contains an x402 challenge.
///
/// Returns `true` if the response has a `PAYMENT-REQUIRED` header (v2 transport)
/// or a JSON body with an `x402Version` field (v1 transport).
pub(crate) fn is_x402_response(response: &HttpResponse) -> bool {
    if response.header("payment-required").is_some() {
        return true;
    }
    // v1 transport: challenge in response body as raw JSON
    response
        .body_string()
        .ok()
        .and_then(|body| serde_json::from_str::<serde_json::Value>(&body).ok())
        .and_then(|v| v.get("x402Version").cloned())
        .is_some()
}

/// Parse the x402 challenge and select a suitable payment option.
///
/// Tries the `PAYMENT-REQUIRED` header first (v2 transport: base64-encoded JSON),
/// then falls back to the response body (v1 transport: raw JSON).
pub(super) fn parse_and_select(response: &HttpResponse) -> Result<SelectedOption, TempoError> {
    let (challenge, raw) = if let Some(header_value) = response.header("payment-required") {
        // v2 transport: base64-encoded JSON in header
        let decoded = STANDARD
            .decode(header_value)
            .map_err(|_| PaymentError::ChallengeParse {
                context: "PAYMENT-REQUIRED header",
                reason: "invalid base64".to_string(),
            })?;

        let challenge: X402PaymentRequired =
            serde_json::from_slice(&decoded).map_err(|e| PaymentError::ChallengeParse {
                context: "PAYMENT-REQUIRED header",
                reason: format!("invalid JSON: {e}"),
            })?;

        let raw: serde_json::Value =
            serde_json::from_slice(&decoded).map_err(|e| PaymentError::ChallengeParse {
                context: "PAYMENT-REQUIRED header",
                reason: format!("invalid JSON: {e}"),
            })?;

        (challenge, raw)
    } else {
        // v1 transport: raw JSON in response body
        let body = response
            .body_string()
            .map_err(|_| PaymentError::ChallengeParse {
                context: "x402 response body",
                reason: "invalid UTF-8".to_string(),
            })?;

        let challenge: X402PaymentRequired =
            serde_json::from_str(&body).map_err(|e| PaymentError::ChallengeParse {
                context: "x402 response body",
                reason: format!("invalid JSON: {e}"),
            })?;

        let raw: serde_json::Value =
            serde_json::from_str(&body).map_err(|e| PaymentError::ChallengeParse {
                context: "x402 response body",
                reason: format!("invalid JSON: {e}"),
            })?;

        (challenge, raw)
    };

    let accepts_array = raw
        .get("accepts")
        .and_then(|v| v.as_array())
        .ok_or_else(|| PaymentError::ChallengeMissingField {
            context: "x402 challenge",
            field: "accepts",
        })?;

    // Find the first option matching our constraints:
    //   - scheme == "exact"
    //   - network is EVM (CAIP-2 "eip155:*" or v1 short name like "base")
    //   - assetTransferMethod is absent or "eip3009"
    for (i, option) in challenge.accepts.iter().enumerate() {
        if !option.scheme.eq_ignore_ascii_case("exact") {
            continue;
        }

        // Resolve chain ID from network field: v2 CAIP-2 or v1 short name
        let dest_chain_id = match parse_evm_chain_id(&option.network) {
            Some(id) => id,
            None => continue,
        };

        let method = option.extra.asset_transfer_method.as_deref();
        if method.is_some() && method != Some("eip3009") {
            continue;
        }

        // Require name and version for EIP-712 domain
        if option.extra.name.is_none() || option.extra.version.is_none() {
            continue;
        }

        // Require an amount (v2 `amount` or v1 `maxAmountRequired`)
        if option.resolved_amount().is_none() {
            continue;
        }

        let accepted_value =
            accepts_array
                .get(i)
                .cloned()
                .ok_or_else(|| PaymentError::ChallengeSchema {
                    context: "x402 challenge",
                    reason: "accepts array index mismatch".to_string(),
                })?;

        let selected_option = option.clone();

        return Ok(SelectedOption {
            challenge,
            accepted_value,
            option: selected_option,
            dest_chain_id,
        });
    }

    Err(PaymentError::ChallengeSchema {
        context: "x402 challenge",
        reason:
            "no supported payment option found (need scheme=exact, EVM network, eip3009 method)"
                .to_string(),
    }
    .into())
}

/// Parse an EVM chain ID from a network field.
///
/// Supports both x402 v2 CAIP-2 format (`"eip155:8453"`) and v1 short
/// names (`"base"`, `"ethereum"`, etc.). Returns `None` for non-EVM
/// networks (e.g. Solana).
fn parse_evm_chain_id(network: &str) -> Option<u64> {
    // v2: CAIP-2 format
    if let Some(chain_id_str) = network.strip_prefix("eip155:") {
        return chain_id_str.parse().ok();
    }

    // v1: short network names
    match network.to_ascii_lowercase().as_str() {
        "ethereum" | "mainnet" => Some(1),
        "base" => Some(8453),
        "base-sepolia" => Some(84532),
        "optimism" => Some(10),
        "arbitrum" => Some(42161),
        "polygon" => Some(137),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine};

    fn make_challenge_json(accepts: serde_json::Value) -> String {
        serde_json::json!({
            "x402Version": 2,
            "accepts": accepts,
            "resource": { "url": "https://example.com/api" },
        })
        .to_string()
    }

    fn make_response(challenge_json: &str) -> HttpResponse {
        let encoded = STANDARD.encode(challenge_json);
        HttpResponse::for_test_with_headers(402, b"", &[("payment-required", &encoded)])
    }

    #[test]
    fn test_parse_and_select_valid_option() {
        let json = make_challenge_json(serde_json::json!([{
            "scheme": "exact",
            "network": "eip155:8453",
            "amount": "1000000",
            "asset": "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913",
            "payTo": "0x1111111111111111111111111111111111111111",
            "maxTimeoutSeconds": 30,
            "extra": {
                "name": "USD Coin",
                "version": "2"
            }
        }]));
        let response = make_response(&json);
        let result = parse_and_select(&response).unwrap();
        assert_eq!(result.dest_chain_id, 8453);
        assert_eq!(result.option.resolved_amount(), Some("1000000"));
    }

    #[test]
    fn test_skips_non_exact_scheme() {
        let json = make_challenge_json(serde_json::json!([{
            "scheme": "flexible",
            "network": "eip155:8453",
            "amount": "1000000",
            "asset": "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913",
            "payTo": "0x1111111111111111111111111111111111111111",
            "extra": { "name": "USD Coin", "version": "2" }
        }]));
        let response = make_response(&json);
        assert!(parse_and_select(&response).is_err());
    }

    #[test]
    fn test_skips_non_evm_network() {
        let json = make_challenge_json(serde_json::json!([{
            "scheme": "exact",
            "network": "solana:mainnet",
            "amount": "1000000",
            "asset": "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913",
            "payTo": "0x1111111111111111111111111111111111111111",
            "extra": { "name": "USD Coin", "version": "2" }
        }]));
        let response = make_response(&json);
        assert!(parse_and_select(&response).is_err());
    }

    #[test]
    fn test_skips_permit2_method() {
        let json = make_challenge_json(serde_json::json!([{
            "scheme": "exact",
            "network": "eip155:8453",
            "amount": "1000000",
            "asset": "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913",
            "payTo": "0x1111111111111111111111111111111111111111",
            "extra": {
                "name": "USD Coin",
                "version": "2",
                "assetTransferMethod": "permit2"
            }
        }]));
        let response = make_response(&json);
        assert!(parse_and_select(&response).is_err());
    }

    #[test]
    fn test_selects_second_option_when_first_is_invalid() {
        let json = make_challenge_json(serde_json::json!([
            {
                "scheme": "exact",
                "network": "solana:mainnet",
                "amount": "1000000",
                "asset": "0xaaa",
                "payTo": "0xbbb",
                "extra": { "name": "USD Coin", "version": "2" }
            },
            {
                "scheme": "exact",
                "network": "eip155:1",
                "amount": "2000000",
                "asset": "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
                "payTo": "0x2222222222222222222222222222222222222222",
                "extra": { "name": "USD Coin", "version": "2" }
            }
        ]));
        let response = make_response(&json);
        let result = parse_and_select(&response).unwrap();
        assert_eq!(result.dest_chain_id, 1);
        assert_eq!(result.option.resolved_amount(), Some("2000000"));
    }

    #[test]
    fn test_missing_header_and_empty_body() {
        let response = HttpResponse::for_test(402, b"");
        let err = parse_and_select(&response).unwrap_err();
        assert!(err.to_string().contains("x402 response body"));
    }

    #[test]
    fn test_parse_v1_short_network_name() {
        let json = make_challenge_json(serde_json::json!([{
            "scheme": "exact",
            "network": "base",
            "amount": "1000",
            "asset": "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913",
            "payTo": "0x1111111111111111111111111111111111111111",
            "maxTimeoutSeconds": 60,
            "extra": {
                "name": "USD Coin",
                "version": "2"
            }
        }]));
        let response = make_response(&json);
        let result = parse_and_select(&response).unwrap();
        assert_eq!(result.dest_chain_id, 8453);
    }

    #[test]
    fn test_parse_evm_chain_id_caip2() {
        assert_eq!(parse_evm_chain_id("eip155:8453"), Some(8453));
        assert_eq!(parse_evm_chain_id("eip155:1"), Some(1));
        assert_eq!(parse_evm_chain_id("eip155:42161"), Some(42161));
    }

    #[test]
    fn test_parse_evm_chain_id_short_names() {
        assert_eq!(parse_evm_chain_id("base"), Some(8453));
        assert_eq!(parse_evm_chain_id("Base"), Some(8453));
        assert_eq!(parse_evm_chain_id("ethereum"), Some(1));
        assert_eq!(parse_evm_chain_id("polygon"), Some(137));
        assert_eq!(parse_evm_chain_id("optimism"), Some(10));
        assert_eq!(parse_evm_chain_id("arbitrum"), Some(42161));
    }

    #[test]
    fn test_parse_evm_chain_id_non_evm() {
        assert_eq!(
            parse_evm_chain_id("solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp"),
            None
        );
        assert_eq!(parse_evm_chain_id("bitcoin"), None);
    }

    #[test]
    fn test_skips_option_missing_eip712_name() {
        let json = make_challenge_json(serde_json::json!([{
            "scheme": "exact",
            "network": "eip155:8453",
            "amount": "1000000",
            "asset": "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913",
            "payTo": "0x1111111111111111111111111111111111111111",
            "extra": { "version": "2" }
        }]));
        let response = make_response(&json);
        assert!(parse_and_select(&response).is_err());
    }
}
