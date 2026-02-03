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
pub fn method_to_network(method: &MethodName) -> Option<&'static str> {
    match method.as_str().to_lowercase().as_str() {
        "tempo" => Some(networks::TEMPO_MODERATO),
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
            "Payment method '{}' is not supported. Supported methods: tempo",
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
/// For core EVM accessors (including `memo()`), use `TempoChargeExt` from mpay.
pub trait ChargeRequestExt {
    /// Create a type-safe `Money` value from this charge request.
    ///
    /// Validates that the currency address matches the network's configured token.
    fn money(&self, network: Network) -> Result<Money>;
}

impl ChargeRequestExt for ChargeRequest {
    fn money(&self, network: Network) -> Result<Money> {
        use mpay::protocol::methods::tempo::TempoChargeExt;

        let currency_addr: Address = self
            .currency_address()
            .map_err(|e| PgetError::InvalidAddress(e.to_string()))?;

        let token_config = network.require_token_config(&self.currency)?;

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
    fn test_method_to_network_case_insensitive() {
        let method = MethodName::new("TEMPO");
        assert_eq!(method_to_network(&method), Some(networks::TEMPO_MODERATO));
    }

    #[test]
    fn test_method_to_network_unsupported() {
        let method = MethodName::new("unknown");
        assert_eq!(method_to_network(&method), None);

        let method = MethodName::new("base");
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
    fn test_charge_request_money() {
        let req = ChargeRequest {
            amount: "1000000".to_string(),
            currency: "0x20c0000000000000000000000000000000000001".to_string(),
            ..Default::default()
        };
        let money = req.money(Network::TempoModerato).expect("valid money");
        assert_eq!(money.atomic(), U256::from(1_000_000u64));
        assert_eq!(money.network(), Network::TempoModerato);
    }

    #[test]
    fn test_charge_request_money_wrong_currency() {
        let req = ChargeRequest {
            amount: "1000000".to_string(),
            currency: "0x1234567890123456789012345678901234567890".to_string(),
            ..Default::default()
        };
        assert!(req.money(Network::TempoModerato).is_err());
    }
}
