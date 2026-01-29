//! Pget-specific extensions to mpay types.
//!
//! This module provides helper functions that bridge mpay's protocol types
//! to pget's network abstractions.
//!
//! For core EVM accessors (recipient_address, currency_address, amount_u256,
//! chain_id, fee_payer), use `mpay::protocol::methods::tempo::TempoChargeExt`.

use alloy::primitives::{Address, U256};
use mpay::{ChargeRequest, MethodName, PaymentChallenge};

use crate::error::{PgetError, Result};
use crate::network::{networks, Network};
use crate::payment::money::{Money, TokenId};

// Re-export TempoChargeExt for convenience
pub use mpay::protocol::methods::tempo::TempoChargeExt;

/// Map an mpay `MethodName` to a pget network name.
///
/// # Supported Mappings
///
/// - "tempo" → "tempo-moderato"
/// - "base" → "base-sepolia"
pub fn method_to_network(method: &MethodName) -> Option<&'static str> {
    match method.as_str().to_lowercase().as_str() {
        "tempo" => Some(networks::TEMPO_MODERATO),
        "base" => Some(networks::BASE_SEPOLIA),
        _ => None,
    }
}

/// Validate that a payment challenge can be processed by pget.
///
/// # Validation Checks
///
/// - The payment method is supported (has a network mapping)
/// - The intent is "charge" (only supported intent currently)
pub fn validate_challenge(challenge: &PaymentChallenge) -> Result<()> {
    if method_to_network(&challenge.method).is_none() {
        return Err(PgetError::UnsupportedPaymentMethod(format!(
            "Payment method '{}' is not supported. Supported methods: tempo, base",
            challenge.method
        )));
    }

    if !challenge.intent.is_charge() {
        return Err(PgetError::UnsupportedPaymentIntent(format!(
            "Only 'charge' intent is supported, got: {}",
            challenge.intent
        )));
    }

    Ok(())
}

/// Pget-specific extensions to ChargeRequest.
///
/// For core EVM accessors, use `TempoChargeExt` from mpay.
pub trait ChargeRequestExt {
    /// Get the memo from method details as a bytes32 value.
    ///
    /// Returns `None` if not specified in `methodDetails`.
    /// The memo should be a hex-encoded 32-byte value (with or without 0x prefix).
    fn memo(&self) -> Option<[u8; 32]>;

    /// Create a type-safe `Money` value from this charge request.
    ///
    /// Validates that the currency address matches the network's configured token.
    fn money(&self, network: Network) -> Result<Money>;
}

impl ChargeRequestExt for ChargeRequest {
    fn memo(&self) -> Option<[u8; 32]> {
        self.method_details
            .as_ref()
            .and_then(|v| v.get("memo"))
            .and_then(|v| v.as_str())
            .and_then(|s| {
                let hex_str = s.strip_prefix("0x").unwrap_or(s);
                let bytes = hex::decode(hex_str).ok()?;
                if bytes.len() == 32 {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(&bytes);
                    Some(arr)
                } else {
                    None
                }
            })
    }

    fn money(&self, network: Network) -> Result<Money> {
        use mpay::protocol::methods::tempo::TempoChargeExt;

        let token_config = network.usdc_config().ok_or_else(|| {
            PgetError::UnsupportedToken(format!("No token configuration for network '{}'", network))
        })?;

        let currency_addr: Address = self
            .currency_address()
            .map_err(|e| PgetError::InvalidAddress(e.to_string()))?;
        let expected_addr: Address = token_config.address.parse().map_err(|e| {
            PgetError::InvalidAddress(format!(
                "Invalid configured token address for {}: {}",
                network, e
            ))
        })?;

        if currency_addr != expected_addr {
            return Err(PgetError::UnsupportedToken(format!(
                "Currency {} does not match configured token {} for network {}",
                self.currency, token_config.address, network
            )));
        }

        let amount: U256 = self
            .amount_u256()
            .map_err(|e| PgetError::InvalidAmount(e.to_string()))?;
        let token = TokenId::new(network, currency_addr);

        Ok(Money::new(
            token,
            amount,
            token_config.currency.decimals,
            token_config.currency.symbol,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_method_to_network_tempo() {
        let method = MethodName::new("tempo");
        assert_eq!(method_to_network(&method), Some(networks::TEMPO_MODERATO));
    }

    #[test]
    fn test_method_to_network_base() {
        let method = MethodName::new("base");
        assert_eq!(method_to_network(&method), Some(networks::BASE_SEPOLIA));
    }

    #[test]
    fn test_method_to_network_case_insensitive() {
        let method = MethodName::new("TEMPO");
        assert_eq!(method_to_network(&method), Some(networks::TEMPO_MODERATO));
    }

    #[test]
    fn test_method_to_network_unsupported() {
        let method = MethodName::new("unknown");
        assert_eq!(method_to_network(&method), None);
    }

    #[test]
    fn test_validate_challenge_valid() {
        use mpay::Base64UrlJson;
        let challenge = PaymentChallenge {
            id: "test".to_string(),
            realm: "test.example.com".to_string(),
            method: MethodName::new("tempo"),
            intent: "charge".into(),
            request: Base64UrlJson::from_value(&serde_json::json!({})).unwrap(),
            digest: None,
            description: None,
            expires: None,
        };
        assert!(validate_challenge(&challenge).is_ok());
    }

    #[test]
    fn test_validate_challenge_unsupported_method() {
        use mpay::Base64UrlJson;
        let challenge = PaymentChallenge {
            id: "test".to_string(),
            realm: "test.example.com".to_string(),
            method: MethodName::new("bitcoin"),
            intent: "charge".into(),
            request: Base64UrlJson::from_value(&serde_json::json!({})).unwrap(),
            digest: None,
            description: None,
            expires: None,
        };
        assert!(validate_challenge(&challenge).is_err());
    }

    #[test]
    fn test_charge_request_memo_with_0x_prefix() {
        let memo_hex = "0xc70864128216764ddcf3cc9b9fc1edb49c453e615e904f2847ba79dd0ec71001";
        let req = ChargeRequest {
            amount: "1000".to_string(),
            currency: "0x123".to_string(),
            method_details: Some(serde_json::json!({
                "memo": memo_hex
            })),
            ..Default::default()
        };
        let memo = req.memo();
        assert!(memo.is_some());
        let memo_bytes = memo.unwrap();
        assert_eq!(memo_bytes[0], 0xc7);
        assert_eq!(memo_bytes[1], 0x08);
    }

    #[test]
    fn test_charge_request_memo_without_prefix() {
        let memo_hex = "c70864128216764ddcf3cc9b9fc1edb49c453e615e904f2847ba79dd0ec71001";
        let req = ChargeRequest {
            amount: "1000".to_string(),
            currency: "0x123".to_string(),
            method_details: Some(serde_json::json!({
                "memo": memo_hex
            })),
            ..Default::default()
        };
        assert!(req.memo().is_some());
    }

    #[test]
    fn test_charge_request_memo_missing() {
        let req = ChargeRequest {
            amount: "1000".to_string(),
            currency: "0x123".to_string(),
            ..Default::default()
        };
        assert!(req.memo().is_none());
    }

    #[test]
    fn test_charge_request_memo_wrong_length() {
        let req = ChargeRequest {
            amount: "1000".to_string(),
            currency: "0x123".to_string(),
            method_details: Some(serde_json::json!({
                "memo": "0x1234"
            })),
            ..Default::default()
        };
        assert!(req.memo().is_none());
    }

    #[test]
    fn test_charge_request_money() {
        let req = ChargeRequest {
            amount: "1000000".to_string(),
            currency: "0x036CbD53842c5426634e7929541eC2318f3dCF7e".to_string(),
            ..Default::default()
        };
        let money = req.money(Network::BaseSepolia).expect("valid money");
        assert_eq!(money.atomic(), U256::from(1_000_000u64));
        assert_eq!(money.network(), Network::BaseSepolia);
    }

    #[test]
    fn test_charge_request_money_wrong_currency() {
        let req = ChargeRequest {
            amount: "1000000".to_string(),
            currency: "0x1234567890123456789012345678901234567890".to_string(),
            ..Default::default()
        };
        assert!(req.money(Network::BaseSepolia).is_err());
    }
}
