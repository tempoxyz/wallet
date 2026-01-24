//! Base-specific types for Web Payment Auth.

use serde::{Deserialize, Serialize};

use super::CHAIN_ID;

/// Base method-specific details in payment requests.
///
/// Base uses standard EIP-1559 transactions with minimal extensions.
///
/// # Examples
///
/// ```
/// use purl::protocol::methods::base::BaseMethodDetails;
///
/// let details = BaseMethodDetails {
///     chain_id: Some(84532),
///     fee_payer: Some(false),
/// };
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BaseMethodDetails {
    /// Chain ID (should be 84532 for Base Sepolia)
    #[serde(rename = "chainId", skip_serializing_if = "Option::is_none")]
    pub chain_id: Option<u64>,

    /// Whether server pays transaction fees
    #[serde(rename = "feePayer", skip_serializing_if = "Option::is_none")]
    pub fee_payer: Option<bool>,
}

impl BaseMethodDetails {
    /// Check if server pays transaction fees.
    pub fn fee_payer(&self) -> bool {
        self.fee_payer.unwrap_or(false)
    }

    /// Check if this is for the Base Sepolia network.
    pub fn is_base_sepolia(&self) -> bool {
        self.chain_id == Some(CHAIN_ID)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base_method_details_serialization() {
        let details = BaseMethodDetails {
            chain_id: Some(84532),
            fee_payer: Some(false),
        };

        let json = serde_json::to_string(&details).unwrap();
        assert!(json.contains("\"chainId\":84532"));
        assert!(json.contains("\"feePayer\":false"));

        let parsed: BaseMethodDetails = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.chain_id, Some(84532));
        assert!(!parsed.fee_payer());
        assert!(parsed.is_base_sepolia());
    }

    #[test]
    fn test_fee_payer_default() {
        let details = BaseMethodDetails::default();
        assert!(!details.fee_payer());
    }

    #[test]
    fn test_is_base_sepolia() {
        let base = BaseMethodDetails {
            chain_id: Some(84532),
            ..Default::default()
        };
        assert!(base.is_base_sepolia());

        let other = BaseMethodDetails {
            chain_id: Some(1),
            ..Default::default()
        };
        assert!(!other.is_base_sepolia());
    }
}
