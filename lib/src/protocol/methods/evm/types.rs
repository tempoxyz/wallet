//! EVM-specific types for Web Payment Auth methods.
//!
//! This module provides shared types for all EVM-based payment methods
//! (Tempo, Base, Ethereum, Polygon, etc.).

use serde::{Deserialize, Serialize};

/// EVM method-specific details in payment requests.
///
/// These fields are parsed from `ChargeRequest.method_details` for EVM-based methods.
///
/// # Examples
///
/// ```
/// use purl::protocol::methods::evm::EvmMethodDetails;
///
/// let details = EvmMethodDetails {
///     chain_id: Some(88153),
///     fee_payer: Some(true),
///     valid_from: None,
/// };
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvmMethodDetails {
    /// Chain ID for EVM-based methods
    #[serde(rename = "chainId", skip_serializing_if = "Option::is_none")]
    pub chain_id: Option<u64>,

    /// Whether server pays transaction fees
    #[serde(rename = "feePayer", skip_serializing_if = "Option::is_none")]
    pub fee_payer: Option<bool>,

    /// Valid from time (ISO 8601, for authorize/subscription intents)
    #[serde(rename = "validFrom", skip_serializing_if = "Option::is_none")]
    pub valid_from: Option<String>,
}

impl EvmMethodDetails {
    /// Check if server pays transaction fees.
    pub fn fee_payer(&self) -> bool {
        self.fee_payer.unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evm_method_details_serialization() {
        let details = EvmMethodDetails {
            chain_id: Some(88153),
            fee_payer: Some(true),
            valid_from: None,
        };

        let json = serde_json::to_string(&details).unwrap();
        assert!(json.contains("\"chainId\":88153"));
        assert!(json.contains("\"feePayer\":true"));
        assert!(!json.contains("validFrom"));

        let parsed: EvmMethodDetails = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.chain_id, Some(88153));
        assert!(parsed.fee_payer());
    }

    #[test]
    fn test_fee_payer_default() {
        let details = EvmMethodDetails::default();
        assert!(!details.fee_payer());
    }
}
