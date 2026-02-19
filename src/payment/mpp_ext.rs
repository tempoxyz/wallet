//! Presto-specific extensions to mpp types.
//!
//! This module provides helper functions that bridge mpp's protocol types
//! to presto's network abstractions.
//!
//! For core EVM accessors (recipient_address, currency_address, amount_u256, chain_id,
//! fee_payer, memo), import `mpp::protocol::methods::tempo::TempoChargeExt` directly.

#[cfg(test)]
use alloy::primitives::{Address, U256};
#[cfg(test)]
use mpp::ChargeRequest;
use mpp::PaymentChallenge;

use crate::error::{PrestoError, Result};
use crate::network::Network;
#[cfg(test)]
use crate::payment::currency::Money;

/// Derive the network from a charge request's chain ID.
pub fn network_from_charge_request(req: &mpp::ChargeRequest) -> crate::error::Result<Network> {
    use mpp::protocol::methods::tempo::TempoChargeExt;
    let chain_id = req.chain_id().ok_or_else(|| {
        crate::error::PrestoError::InvalidConfig("Missing chainId in charge request".to_string())
    })?;
    Network::from_chain_id(chain_id).ok_or_else(|| {
        crate::error::PrestoError::InvalidConfig(format!("Unsupported chainId: {}", chain_id))
    })
}

/// Derive the network from a session request's chain ID.
pub fn network_from_session_request(req: &mpp::SessionRequest) -> crate::error::Result<Network> {
    use mpp::protocol::methods::tempo::session::TempoSessionExt;
    let chain_id = req.chain_id().ok_or_else(|| {
        crate::error::PrestoError::InvalidConfig("Missing chainId in session request".to_string())
    })?;
    Network::from_chain_id(chain_id).ok_or_else(|| {
        crate::error::PrestoError::InvalidConfig(format!("Unsupported chainId: {}", chain_id))
    })
}

/// Validate that a payment challenge can be processed by presto's charge flow.
///
/// Delegates to `PaymentChallenge::validate_for_charge("tempo")` from mpp,
/// mapping mpp errors to presto error types.
pub fn validate_challenge(challenge: &PaymentChallenge) -> Result<()> {
    challenge.validate_for_charge("tempo").map_err(|e| match e {
        mpp::MppError::UnsupportedPaymentMethod(msg) => PrestoError::UnsupportedPaymentMethod(msg),
        mpp::MppError::PaymentExpired(_) => {
            PrestoError::ChallengeExpired(challenge.expires.clone().unwrap_or_default())
        }
        mpp::MppError::InvalidChallenge { reason, .. } => {
            PrestoError::UnsupportedPaymentIntent(reason.unwrap_or_default())
        }
        other => PrestoError::InvalidChallenge(other.to_string()),
    })
}

/// Validate that a payment challenge is a valid session challenge.
///
/// Delegates to `PaymentChallenge::validate_for_session("tempo")` from mpp,
/// mapping mpp errors to presto error types.
pub fn validate_session_challenge(challenge: &PaymentChallenge) -> Result<()> {
    challenge
        .validate_for_session("tempo")
        .map_err(|e| match e {
            mpp::MppError::UnsupportedPaymentMethod(msg) => {
                PrestoError::UnsupportedPaymentMethod(msg)
            }
            mpp::MppError::PaymentExpired(_) => {
                PrestoError::ChallengeExpired(challenge.expires.clone().unwrap_or_default())
            }
            mpp::MppError::InvalidChallenge { reason, .. } => {
                PrestoError::UnsupportedPaymentIntent(reason.unwrap_or_default())
            }
            other => PrestoError::InvalidChallenge(other.to_string()),
        })
}

/// Presto-specific extensions to ChargeRequest.
///
/// For core EVM accessors (including `memo()`), use `TempoChargeExt` from mpp.
#[cfg(test)]
pub trait ChargeRequestExt {
    /// Create a type-safe `Money` value from this charge request.
    ///
    /// Validates that the currency address matches the network's configured token.
    fn money(&self, network: Network) -> Result<Money>;
}

#[cfg(test)]
impl ChargeRequestExt for ChargeRequest {
    fn money(&self, network: Network) -> Result<Money> {
        use crate::payment::currency::{Money, TokenId};
        use mpp::protocol::methods::tempo::TempoChargeExt;

        let currency_addr: Address = self
            .currency_address()
            .map_err(|e| PrestoError::InvalidAddress(e.to_string()))?;

        let token_config = network.require_token_config(&self.currency)?;

        let amount: U256 = self
            .amount_u256()
            .map_err(|e| PrestoError::InvalidAmount(e.to_string()))?;
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
    use mpp::MethodName;

    #[test]
    fn test_validate_challenge_valid() {
        use mpp::Base64UrlJson;
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
        use mpp::Base64UrlJson;
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
    fn test_validate_session_challenge_valid() {
        use mpp::Base64UrlJson;
        let challenge = PaymentChallenge {
            id: "test".to_string(),
            realm: "test.example.com".to_string(),
            method: MethodName::new("tempo"),
            intent: "session".into(),
            request: Base64UrlJson::from_value(&serde_json::json!({})).unwrap(),
            digest: None,
            description: None,
            expires: None,
        };
        assert!(validate_session_challenge(&challenge).is_ok());
    }

    #[test]
    fn test_validate_session_challenge_wrong_intent() {
        use mpp::Base64UrlJson;
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
        assert!(validate_session_challenge(&challenge).is_err());
    }

    #[test]
    fn test_charge_request_money() {
        let req = ChargeRequest {
            amount: "1000000".to_string(),
            currency: "0x20c0000000000000000000000000000000000000".to_string(),
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
