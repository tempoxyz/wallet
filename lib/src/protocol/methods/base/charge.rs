//! Base extensions for ChargeRequest.
//!
//! Provides Base-specific accessors for ChargeRequest. Base uses standard
//! EIP-1559 transactions, so most functionality comes from the EVM layer.

use super::types::BaseMethodDetails;
use crate::error::{PurlError, Result};
use crate::protocol::intents::ChargeRequest;
use crate::protocol::methods::evm::EvmChargeExt;

/// Extension trait for ChargeRequest with Base-specific accessors.
///
/// # Examples
///
/// ```ignore
/// use purl::protocol::intents::ChargeRequest;
/// use purl::protocol::methods::base::BaseChargeExt;
///
/// let req: ChargeRequest = challenge.request.decode()?;
/// if req.is_base_sepolia() {
///     // Handle Base-specific logic
/// }
/// ```
pub trait BaseChargeExt: EvmChargeExt {
    /// Parse the method_details as Base-specific details.
    fn base_method_details(&self) -> Result<BaseMethodDetails>;

    /// Check if this request is for Base Sepolia network.
    fn is_base_sepolia(&self) -> bool;
}

impl BaseChargeExt for ChargeRequest {
    fn base_method_details(&self) -> Result<BaseMethodDetails> {
        match &self.method_details {
            Some(value) => serde_json::from_value(value.clone()).map_err(|e| {
                PurlError::InvalidChallenge(format!("Invalid Base method details: {}", e))
            }),
            None => Ok(BaseMethodDetails::default()),
        }
    }

    fn is_base_sepolia(&self) -> bool {
        self.chain_id() == Some(super::CHAIN_ID)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_charge_request() -> ChargeRequest {
        ChargeRequest {
            amount: "1000000".to_string(),
            currency: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".to_string(),
            recipient: Some("0x742d35Cc6634C0532925a3b844Bc9e7595f1B0F2".to_string()),
            expires: Some("2024-01-01T00:00:00Z".to_string()),
            description: None,
            external_id: None,
            method_details: Some(serde_json::json!({
                "chainId": 84532,
                "feePayer": false
            })),
        }
    }

    #[test]
    fn test_base_method_details() {
        let req = test_charge_request();
        let details = req.base_method_details().unwrap();
        assert_eq!(details.chain_id, Some(84532));
        assert!(!details.fee_payer());
    }

    #[test]
    fn test_is_base_sepolia() {
        let req = test_charge_request();
        assert!(req.is_base_sepolia());

        let req_other_chain = ChargeRequest {
            method_details: Some(serde_json::json!({"chainId": 1})),
            ..test_charge_request()
        };
        assert!(!req_other_chain.is_base_sepolia());
    }
}
