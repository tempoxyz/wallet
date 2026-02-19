//! Gas estimation for Tempo AA transactions.

use crate::error::{PrestoError, Result};
use crate::network::GasConfig;
use alloy::primitives::Address;
use tempo_primitives::transaction::{Call, SignedKeyAuthorization};
use tracing::debug;

use super::signing::HttpProvider;

/// Build the JSON request body for eth_estimateGas with Tempo AA fields.
#[allow(clippy::too_many_arguments)]
pub(super) fn build_estimate_gas_request(
    from: Address,
    chain_id: u64,
    nonce: u64,
    fee_token: Address,
    calls: &[Call],
    gas_config: &GasConfig,
    key_authorization: Option<&SignedKeyAuthorization>,
) -> Result<serde_json::Value> {
    let mut req = serde_json::json!({
        "from": format!("{:#x}", from),
        "chainId": format!("{:#x}", chain_id),
        "nonce": format!("{:#x}", nonce),
        "maxFeePerGas": format!("{:#x}", gas_config.max_fee_per_gas),
        "maxPriorityFeePerGas": format!("{:#x}", gas_config.max_priority_fee_per_gas),
        "feeToken": format!("{:#x}", fee_token),
        "nonceKey": "0x0",
        "calls": calls.iter().map(|c| {
            serde_json::json!({
                "to": c.to.to().map(|a| format!("{:#x}", a)),
                "value": format!("{:#x}", c.value),
                "input": format!("0x{}", hex::encode(&c.input)),
            })
        }).collect::<Vec<_>>(),
    });

    if let Some(auth) = key_authorization {
        req["keyAuthorization"] = serde_json::to_value(auth).map_err(|e| {
            PrestoError::InvalidChallenge(format!("Failed to serialize key authorization: {}", e))
        })?;
    }

    Ok(req)
}

/// Parse a hex gas estimate and apply a fixed buffer.
pub(super) fn parse_gas_estimate_with_buffer(gas_hex: &str) -> Result<u64> {
    let gas_limit = u64::from_str_radix(gas_hex.trim_start_matches("0x"), 16).map_err(|e| {
        PrestoError::InvalidChallenge(format!("Failed to parse gas estimate '{}': {}", gas_hex, e))
    })?;

    Ok(gas_limit + 5_000)
}

/// Estimate gas for a Tempo AA transaction via eth_estimateGas RPC.
#[allow(clippy::too_many_arguments)]
pub(super) async fn estimate_tempo_gas(
    provider: &HttpProvider,
    from: Address,
    chain_id: u64,
    nonce: u64,
    fee_token: Address,
    calls: &[Call],
    gas_config: &GasConfig,
    key_authorization: Option<&SignedKeyAuthorization>,
) -> Result<u64> {
    use alloy::providers::Provider;

    let req = build_estimate_gas_request(
        from,
        chain_id,
        nonce,
        fee_token,
        calls,
        gas_config,
        key_authorization,
    )?;

    let gas_hex: String = provider
        .raw_request("eth_estimateGas".into(), [req])
        .await
        .map_err(|e| PrestoError::InvalidChallenge(format!("Gas estimation failed: {}", e)))?;

    let gas_limit = parse_gas_estimate_with_buffer(&gas_hex)?;

    debug!(
        estimated_gas = gas_limit,
        "eth_estimateGas result (with +5000 buffer)"
    );
    Ok(gas_limit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::{TxKind, U256};
    use alloy::signers::{local::PrivateKeySigner, SignerSync};
    use tempo_primitives::transaction::PrimitiveSignature;

    #[test]
    fn test_build_estimate_gas_request_basic_fields() {
        let from = Address::repeat_byte(0x11);
        let chain_id = 42431u64;
        let nonce = 5u64;
        let fee_token = Address::repeat_byte(0x22);
        let gas = GasConfig::DEFAULT;

        let call_to = Address::repeat_byte(0x33);
        let calls = vec![Call {
            to: TxKind::Call(call_to),
            value: U256::ZERO,
            input: alloy::primitives::Bytes::from_static(&[0xaa, 0xbb]),
        }];

        let req = build_estimate_gas_request(from, chain_id, nonce, fee_token, &calls, &gas, None)
            .unwrap();

        assert_eq!(req["from"], format!("{:#x}", from));
        assert_eq!(req["chainId"], format!("{:#x}", chain_id));
        assert_eq!(req["nonce"], format!("{:#x}", nonce));
        assert_eq!(req["maxFeePerGas"], format!("{:#x}", gas.max_fee_per_gas));
        assert_eq!(
            req["maxPriorityFeePerGas"],
            format!("{:#x}", gas.max_priority_fee_per_gas)
        );
        assert_eq!(req["feeToken"], format!("{:#x}", fee_token));
        assert_eq!(req["nonceKey"], "0x0");

        let calls_json = req["calls"].as_array().unwrap();
        assert_eq!(calls_json.len(), 1);
        assert_eq!(calls_json[0]["to"], format!("{:#x}", call_to));
        assert_eq!(calls_json[0]["value"], "0x0");
        assert_eq!(calls_json[0]["input"], "0xaabb");

        assert!(req.get("keyAuthorization").is_none());
    }

    #[test]
    fn test_build_estimate_gas_request_multiple_calls() {
        let from = Address::ZERO;
        let calls = vec![
            Call {
                to: TxKind::Call(Address::repeat_byte(0x01)),
                value: U256::ZERO,
                input: alloy::primitives::Bytes::new(),
            },
            Call {
                to: TxKind::Call(Address::repeat_byte(0x02)),
                value: U256::from(42u64),
                input: alloy::primitives::Bytes::from_static(&[0xff]),
            },
            Call {
                to: TxKind::Call(Address::repeat_byte(0x03)),
                value: U256::ZERO,
                input: alloy::primitives::Bytes::new(),
            },
        ];

        let req = build_estimate_gas_request(
            from,
            4217,
            0,
            Address::ZERO,
            &calls,
            &GasConfig::DEFAULT,
            None,
        )
        .unwrap();

        let calls_json = req["calls"].as_array().unwrap();
        assert_eq!(calls_json.len(), 3);
        assert_eq!(calls_json[1]["value"], format!("{:#x}", 42u64));
        assert_eq!(calls_json[1]["input"], "0xff");
    }

    #[test]
    fn test_build_estimate_gas_request_with_key_authorization() {
        use tempo_primitives::transaction::{KeyAuthorization, SignatureType};

        let signer: PrivateKeySigner =
            "0x1234567890123456789012345678901234567890123456789012345678901234"
                .parse()
                .unwrap();

        let auth = KeyAuthorization {
            chain_id: 42431,
            key_type: SignatureType::Secp256k1,
            key_id: signer.address(),
            expiry: Some(9999999999),
            limits: None,
        };
        let inner_sig = signer.sign_hash_sync(&auth.signature_hash()).unwrap();
        let signed_auth = auth.into_signed(PrimitiveSignature::Secp256k1(inner_sig));

        let calls = vec![Call {
            to: TxKind::Call(Address::ZERO),
            value: U256::ZERO,
            input: alloy::primitives::Bytes::new(),
        }];

        let req = build_estimate_gas_request(
            Address::ZERO,
            42431,
            0,
            Address::ZERO,
            &calls,
            &GasConfig::DEFAULT,
            Some(&signed_auth),
        )
        .unwrap();

        assert!(req.get("keyAuthorization").is_some());
        let ka = &req["keyAuthorization"];
        assert!(ka.is_object(), "keyAuthorization should be a JSON object");
    }

    #[test]
    fn test_parse_gas_estimate_with_buffer_hex_prefix() {
        // 100_000 = 0x186a0 → with +5000 buffer = 105_000
        let result = parse_gas_estimate_with_buffer("0x186a0").unwrap();
        assert_eq!(result, 105_000);
    }

    #[test]
    fn test_parse_gas_estimate_with_buffer_no_prefix() {
        let result = parse_gas_estimate_with_buffer("186a0").unwrap();
        assert_eq!(result, 105_000);
    }

    #[test]
    fn test_parse_gas_estimate_with_buffer_fixed() {
        // 1 gas → 1 + 5000 = 5001
        assert_eq!(parse_gas_estimate_with_buffer("0x1").unwrap(), 5_001);

        // 5 gas → 5 + 5000 = 5005
        assert_eq!(parse_gas_estimate_with_buffer("0x5").unwrap(), 5_005);

        // 250_000 gas → 250000 + 5000 = 255_000
        assert_eq!(parse_gas_estimate_with_buffer("0x3d090").unwrap(), 255_000);
    }

    #[test]
    fn test_parse_gas_estimate_invalid_hex() {
        let result = parse_gas_estimate_with_buffer("0xGGGG");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_gas_estimate_empty_string() {
        let result = parse_gas_estimate_with_buffer("");
        assert!(result.is_err());
    }
}
