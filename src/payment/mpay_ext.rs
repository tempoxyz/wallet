//! Purl-specific extensions to mpay types.
//!
//! This module provides helper functions that bridge mpay's protocol types
//! to pget's network abstractions. Import mpay types directly from `mpay::*`.

#![allow(dead_code)]

use alloy::primitives::{Address, U256};
use mpay::evm::{parse_address, parse_amount};
use mpay::{ChargeRequest, MethodName, PaymentChallenge};

use crate::error::{PgetError, Result};
use crate::network::{networks, Network};
use crate::payment::money::{Money, TokenId};

/// Map an mpay `MethodName` to a pget network name.
///
/// Returns the canonical network name for supported payment methods,
/// or `None` for unsupported methods.
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

/// Check if a payment method is supported by pget.
pub fn is_method_supported(method: &MethodName) -> bool {
    method_to_network(method).is_some()
}

/// Check if a method is "tempo".
pub fn is_tempo(method: &MethodName) -> bool {
    method.eq_ignore_ascii_case("tempo")
}

/// Check if a method is "base".
pub fn is_base(method: &MethodName) -> bool {
    method.eq_ignore_ascii_case("base")
}

/// Validate that a payment challenge can be processed by pget.
///
/// # Validation Checks
///
/// - The payment method is supported (has a network mapping)
/// - The intent is "charge" (only supported intent currently)
///
/// # Errors
///
/// Returns `UnsupportedPaymentMethod` if the method has no network mapping.
/// Returns `UnsupportedPaymentIntent` if the intent is not "charge".
pub fn validate_challenge(challenge: &PaymentChallenge) -> Result<()> {
    if !is_method_supported(&challenge.method) {
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

/// Extension trait for mpay's `ChargeRequest` with EVM-specific accessors.
///
/// Provides typed accessors for parsing string fields into EVM primitives
/// and pget money types.
pub trait ChargeRequestExt {
    /// Get the recipient address as a typed `Address`.
    ///
    /// # Errors
    ///
    /// Returns an error if no recipient is specified or if the address
    /// cannot be parsed as a valid EVM address.
    fn recipient_address(&self) -> Result<Address>;

    /// Get the currency/asset address as a typed `Address`.
    ///
    /// # Errors
    ///
    /// Returns an error if the currency cannot be parsed as a valid EVM address.
    fn currency_address(&self) -> Result<Address>;

    /// Get the amount as a typed `U256`.
    ///
    /// # Errors
    ///
    /// Returns an error if the amount cannot be parsed as a valid U256.
    fn amount_u256(&self) -> Result<U256>;

    /// Check if server pays transaction fees.
    ///
    /// Reads from `methodDetails.feePayer`, defaults to `false` if not present.
    fn fee_payer(&self) -> bool;

    /// Get the chain ID from method details.
    ///
    /// Returns `None` if not specified in `methodDetails`.
    fn chain_id(&self) -> Option<u64>;

    /// Get the memo from method details as a bytes32 value.
    ///
    /// Returns `None` if not specified in `methodDetails`.
    /// The memo should be a hex-encoded 32-byte value (with or without 0x prefix).
    fn memo(&self) -> Option<[u8; 32]>;

    /// Create a type-safe `Money` value from this charge request.
    ///
    /// Validates that the currency address matches the network's configured token.
    ///
    /// # Arguments
    ///
    /// * `network` - The network this charge is for (used for token lookup)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The network has no token configuration
    /// - The currency address doesn't match the network's configured token
    /// - The amount cannot be parsed as U256
    fn money(&self, network: Network) -> Result<Money>;
}

impl ChargeRequestExt for ChargeRequest {
    fn recipient_address(&self) -> Result<Address> {
        let recipient = self.recipient.as_ref().ok_or_else(|| {
            PgetError::InvalidChallenge("No recipient specified in charge request".to_string())
        })?;
        parse_address(recipient).map_err(|e| PgetError::InvalidAddress(e.to_string()))
    }

    fn currency_address(&self) -> Result<Address> {
        parse_address(&self.currency).map_err(|e| PgetError::InvalidAddress(e.to_string()))
    }

    fn amount_u256(&self) -> Result<U256> {
        parse_amount(&self.amount).map_err(|e| PgetError::InvalidAmount(e.to_string()))
    }

    fn fee_payer(&self) -> bool {
        self.method_details
            .as_ref()
            .and_then(|v| v.get("feePayer"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    fn chain_id(&self) -> Option<u64> {
        self.method_details
            .as_ref()
            .and_then(|v| v.get("chainId"))
            .and_then(|v| v.as_u64())
    }

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
        let token_config = network.usdc_config().ok_or_else(|| {
            PgetError::UnsupportedToken(format!("No token configuration for network '{}'", network))
        })?;

        let currency_addr = self.currency_address()?;
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

        let amount = self.amount_u256()?;
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
    fn test_is_method_supported() {
        assert!(is_method_supported(&MethodName::new("tempo")));
        assert!(is_method_supported(&MethodName::new("base")));
        assert!(!is_method_supported(&MethodName::new("ethereum")));
    }

    #[test]
    fn test_charge_request_amount_u256() {
        let req = ChargeRequest {
            amount: "1000000".to_string(),
            currency: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".to_string(),
            ..Default::default()
        };
        let amount = req.amount_u256().expect("valid amount");
        assert_eq!(amount, U256::from(1_000_000u64));
    }

    #[test]
    fn test_charge_request_currency_address() {
        let req = ChargeRequest {
            amount: "1000000".to_string(),
            currency: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".to_string(),
            ..Default::default()
        };
        let addr = req.currency_address().expect("valid address");
        assert_eq!(
            format!("{:?}", addr).to_lowercase(),
            "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"
        );
    }

    #[test]
    fn test_charge_request_recipient_address() {
        let req = ChargeRequest {
            amount: "1000000".to_string(),
            currency: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".to_string(),
            recipient: Some("0x742d35Cc6634C0532925a3b844Bc9e7595f1B0F2".to_string()),
            ..Default::default()
        };
        let addr = req.recipient_address().expect("valid address");
        assert_eq!(
            format!("{:?}", addr).to_lowercase(),
            "0x742d35cc6634c0532925a3b844bc9e7595f1b0f2"
        );
    }

    #[test]
    fn test_charge_request_recipient_missing() {
        let req = ChargeRequest {
            amount: "1000000".to_string(),
            currency: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".to_string(),
            ..Default::default()
        };
        assert!(req.recipient_address().is_err());
    }

    #[test]
    fn test_charge_request_fee_payer() {
        let req = ChargeRequest {
            amount: "1000000".to_string(),
            currency: "0x123".to_string(),
            method_details: Some(serde_json::json!({
                "feePayer": true
            })),
            ..Default::default()
        };
        assert!(req.fee_payer());

        let req_no_fee = ChargeRequest {
            amount: "1000000".to_string(),
            currency: "0x123".to_string(),
            ..Default::default()
        };
        assert!(!req_no_fee.fee_payer());
    }

    #[test]
    fn test_charge_request_chain_id() {
        let req = ChargeRequest {
            amount: "1000000".to_string(),
            currency: "0x123".to_string(),
            method_details: Some(serde_json::json!({
                "chainId": 42431
            })),
            ..Default::default()
        };
        assert_eq!(req.chain_id(), Some(42431));

        let req_no_chain = ChargeRequest {
            amount: "1000000".to_string(),
            currency: "0x123".to_string(),
            ..Default::default()
        };
        assert_eq!(req_no_chain.chain_id(), None);
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

    #[test]
    fn test_charge_request_memo_with_0x_prefix() {
        // 32 bytes = 64 hex chars
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
        // 32 bytes = 64 hex chars
        let memo_hex = "c70864128216764ddcf3cc9b9fc1edb49c453e615e904f2847ba79dd0ec71001";
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
                "memo": "0x1234"  // Too short
            })),
            ..Default::default()
        };
        assert!(req.memo().is_none());
    }

    #[test]
    fn test_charge_request_memo_invalid_hex() {
        let req = ChargeRequest {
            amount: "1000".to_string(),
            currency: "0x123".to_string(),
            method_details: Some(serde_json::json!({
                "memo": "0xZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ"
            })),
            ..Default::default()
        };
        assert!(req.memo().is_none());
    }
}
