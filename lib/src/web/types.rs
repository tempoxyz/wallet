//! Type definitions for the Web Payment Auth protocol

use serde::{Deserialize, Serialize};
use std::fmt;

/// Payment method identifier
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaymentMethod {
    /// Tempo blockchain payment
    Tempo,
    /// Base blockchain payment
    Base,
    /// Custom payment method
    #[serde(untagged)]
    Custom(String),
}

impl fmt::Display for PaymentMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PaymentMethod::Tempo => write!(f, "tempo"),
            PaymentMethod::Base => write!(f, "base"),
            PaymentMethod::Custom(s) => write!(f, "{}", s),
        }
    }
}

/// Payment intent type
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaymentIntent {
    /// One-time charge
    Charge,
    /// Authorization for future payments
    Authorize,
    /// Recurring subscription
    Subscription,
    /// Custom intent
    #[serde(untagged)]
    Custom(String),
}

impl fmt::Display for PaymentIntent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PaymentIntent::Charge => write!(f, "charge"),
            PaymentIntent::Authorize => write!(f, "authorize"),
            PaymentIntent::Subscription => write!(f, "subscription"),
            PaymentIntent::Custom(s) => write!(f, "{}", s),
        }
    }
}

/// Payment challenge from server (WWW-Authenticate header)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentChallenge {
    /// Unique challenge identifier (128+ bits entropy)
    pub id: String,

    /// Protection space / realm
    pub realm: String,

    /// Payment method
    pub method: PaymentMethod,

    /// Payment intent
    pub intent: PaymentIntent,

    /// Method+intent specific request data
    pub request: serde_json::Value,

    /// Challenge expiration time (ISO 8601)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires: Option<String>,

    /// Human-readable description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Charge request (for charge intent)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChargeRequest {
    /// Amount in base units (e.g., wei, satoshi)
    pub amount: String,

    /// Token/asset contract address
    pub asset: String,

    /// Recipient address
    pub destination: String,

    /// Request expiration (ISO 8601)
    pub expires: String,

    /// Whether server pays fees (optional)
    #[serde(rename = "feePayer", skip_serializing_if = "Option::is_none")]
    pub fee_payer: Option<bool>,
}

/// Authorize request (for authorize intent)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizeRequest {
    /// Token/asset contract address
    pub asset: String,

    /// Optional specific recipient
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,

    /// Authorization expiration (ISO 8601)
    pub expires: String,

    /// Spending limit in base units
    pub limit: String,

    /// Valid from time (ISO 8601, optional)
    #[serde(rename = "validFrom", skip_serializing_if = "Option::is_none")]
    pub valid_from: Option<String>,

    /// Whether server pays fees (optional)
    #[serde(rename = "feePayer", skip_serializing_if = "Option::is_none")]
    pub fee_payer: Option<bool>,
}

/// Subscription request (for subscription intent)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionRequest {
    /// Token/asset contract address
    pub asset: String,

    /// Subscription recipient
    pub destination: String,

    /// Amount per interval in base units
    pub amount: String,

    /// Interval in seconds
    pub interval: u64,

    /// Subscription expiration (ISO 8601)
    pub expires: String,

    /// Whether server pays fees (optional)
    #[serde(rename = "feePayer", skip_serializing_if = "Option::is_none")]
    pub fee_payer: Option<bool>,
}

/// Payment credential from client (Authorization header)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentCredential {
    /// Matching challenge ID
    pub id: String,

    /// Payer identifier (DID format: did:pkh:eip155:chainId:address)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,

    /// Payment payload
    pub payload: PaymentPayload,
}

/// Payload type
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PayloadType {
    /// Signed blockchain transaction
    Transaction,
    /// Key authorization signature
    KeyAuthorization,
}

impl fmt::Display for PayloadType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PayloadType::Transaction => write!(f, "transaction"),
            PayloadType::KeyAuthorization => write!(f, "keyAuthorization"),
        }
    }
}

/// Payment payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentPayload {
    /// Payload type
    #[serde(rename = "type")]
    pub payload_type: PayloadType,

    /// Signature (hex-encoded signed transaction or authorization)
    pub signature: String,
}

/// Receipt status
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReceiptStatus {
    /// Payment succeeded
    Success,
    /// Payment failed
    Failed,
}

impl fmt::Display for ReceiptStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReceiptStatus::Success => write!(f, "success"),
            ReceiptStatus::Failed => write!(f, "failed"),
        }
    }
}

/// Payment receipt from server (Payment-Receipt header)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentReceipt {
    /// Receipt status
    pub status: ReceiptStatus,

    /// Payment method used
    pub method: PaymentMethod,

    /// Timestamp (ISO 8601)
    pub timestamp: String,

    /// Transaction hash or reference
    pub reference: String,

    /// Block number (optional)
    #[serde(rename = "blockNumber", skip_serializing_if = "Option::is_none")]
    pub block_number: Option<String>,

    /// Error message if failed (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_payment_method_serialization() {
        assert_eq!(
            serde_json::to_string(&PaymentMethod::Tempo).unwrap(),
            "\"tempo\""
        );
        assert_eq!(
            serde_json::to_string(&PaymentMethod::Base).unwrap(),
            "\"base\""
        );
    }

    #[test]
    fn test_payment_intent_serialization() {
        assert_eq!(
            serde_json::to_string(&PaymentIntent::Charge).unwrap(),
            "\"charge\""
        );
        assert_eq!(
            serde_json::to_string(&PaymentIntent::Authorize).unwrap(),
            "\"authorize\""
        );
    }

    #[test]
    fn test_charge_request_serialization() {
        let req = ChargeRequest {
            amount: "10000".to_string(),
            asset: "0x123".to_string(),
            destination: "0x456".to_string(),
            expires: "2024-01-01T00:00:00Z".to_string(),
            fee_payer: Some(false),
        };

        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"amount\":\"10000\""));
        assert!(json.contains("\"feePayer\":false"));
    }

    #[test]
    fn test_payment_credential_serialization() {
        let cred = PaymentCredential {
            id: "abc123".to_string(),
            source: Some("did:pkh:eip155:88153:0x123".to_string()),
            payload: PaymentPayload {
                payload_type: PayloadType::Transaction,
                signature: "0xabc".to_string(),
            },
        };

        let json = serde_json::to_string(&cred).unwrap();
        assert!(json.contains("\"id\":\"abc123\""));
        assert!(json.contains("\"type\":\"transaction\""));
    }

    #[test]
    fn test_receipt_status_serialization() {
        assert_eq!(
            serde_json::to_string(&ReceiptStatus::Success).unwrap(),
            "\"success\""
        );
        assert_eq!(
            serde_json::to_string(&ReceiptStatus::Failed).unwrap(),
            "\"failed\""
        );
    }
}
