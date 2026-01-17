//! Type definitions for the Web Payment Auth protocol

use crate::error::{PurlError, Result};
use crate::network::networks;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

// ==================== Payment Protocol Detection ====================

/// Payment protocol detected from HTTP 402 response.
///
/// Used to determine how to handle a payment-required response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaymentProtocol {
    /// x402 protocol - uses JSON body with payment requirements
    X402,
    /// Web Payment Auth (IETF draft) - uses WWW-Authenticate/Authorization headers
    WebPaymentAuth,
}

impl PaymentProtocol {
    /// Detect the payment protocol from HTTP response headers.
    ///
    /// Returns `WebPaymentAuth` if the response has a `WWW-Authenticate: Payment ...` header,
    /// otherwise assumes `X402` (the default protocol).
    ///
    /// Detection is case-insensitive and tolerant of leading whitespace per RFC 7235.
    ///
    /// # Arguments
    /// * `www_authenticate` - The value of the WWW-Authenticate header, if present
    pub fn detect(www_authenticate: Option<&str>) -> Self {
        match www_authenticate {
            Some(header) => {
                let trimmed = header.trim_start();
                if trimmed.len() >= 8 && trimmed[..8].eq_ignore_ascii_case("payment ") {
                    Self::WebPaymentAuth
                } else {
                    Self::X402
                }
            }
            None => Self::X402,
        }
    }

    /// Check if this is the Web Payment Auth protocol.
    pub fn is_web_payment_auth(&self) -> bool {
        matches!(self, Self::WebPaymentAuth)
    }

    /// Check if this is the x402 protocol.
    pub fn is_x402(&self) -> bool {
        matches!(self, Self::X402)
    }
}

impl fmt::Display for PaymentProtocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::X402 => write!(f, "x402"),
            Self::WebPaymentAuth => write!(f, "Web Payment Auth"),
        }
    }
}

// ==================== Payment Method ====================

/// Payment method identifier for Web Payment Auth protocol.
///
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaymentMethod {
    /// Tempo blockchain payment (targets tempo-moderato testnet)
    Tempo,
    /// Base blockchain payment (currently targets Base Sepolia testnet)
    Base,
    /// Custom/unknown payment method (not supported for payments)
    #[serde(untagged)]
    Custom(String),
}

impl PaymentMethod {
    pub fn network_name(&self) -> Option<&'static str> {
        match self {
            PaymentMethod::Tempo => Some(networks::TEMPO_MODERATO),
            PaymentMethod::Base => Some(networks::BASE_SEPOLIA),
            PaymentMethod::Custom(_) => None,
        }
    }

    /// Check if this payment method is supported for web payments.
    ///
    /// A method is supported if it has an associated network.
    pub fn is_supported(&self) -> bool {
        self.network_name().is_some()
    }
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

impl FromStr for PaymentMethod {
    type Err = PurlError;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "tempo" => Ok(PaymentMethod::Tempo),
            "base" => Ok(PaymentMethod::Base),
            other => Ok(PaymentMethod::Custom(other.to_string())),
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

impl PaymentChallenge {
    /// Validate that the challenge can be processed for payment.
    ///
    /// This validates:
    /// - The payment method is supported
    /// - The payment intent is supported (currently only 'charge')
    ///
    /// Call this early in payment processing to fail fast with clear errors.
    pub fn validate(&self) -> Result<()> {
        // Validate payment method
        if !self.method.is_supported() {
            return Err(PurlError::unsupported_method(&self.method));
        }

        // Validate payment intent (only charge is supported currently)
        if self.intent != PaymentIntent::Charge {
            return Err(PurlError::UnsupportedPaymentIntent(format!(
                "Only 'charge' intent is supported, got: {}",
                self.intent
            )));
        }

        Ok(())
    }

    /// Get the network name for this challenge's payment method.
    ///
    /// Returns an error for unsupported/custom payment methods.
    pub fn network_name(&self) -> Result<&'static str> {
        self.method.network_name().ok_or_else(|| {
            PurlError::UnsupportedPaymentMethod(format!(
                "Payment method '{}' has no associated network",
                self.method
            ))
        })
    }
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
    fn test_payment_protocol_detect() {
        // Web Payment Auth detection
        assert_eq!(
            PaymentProtocol::detect(Some("Payment id=\"abc\", realm=\"api\"")),
            PaymentProtocol::WebPaymentAuth
        );

        // Case-insensitive detection (RFC 7235 allows case-insensitive auth schemes)
        assert_eq!(
            PaymentProtocol::detect(Some("payment id=\"abc\"")),
            PaymentProtocol::WebPaymentAuth
        );
        assert_eq!(
            PaymentProtocol::detect(Some("PAYMENT id=\"abc\"")),
            PaymentProtocol::WebPaymentAuth
        );
        assert_eq!(
            PaymentProtocol::detect(Some("PaYmEnT id=\"abc\"")),
            PaymentProtocol::WebPaymentAuth
        );

        // Tolerant of leading whitespace
        assert_eq!(
            PaymentProtocol::detect(Some("  Payment id=\"abc\"")),
            PaymentProtocol::WebPaymentAuth
        );
        assert_eq!(
            PaymentProtocol::detect(Some("\t Payment id=\"abc\"")),
            PaymentProtocol::WebPaymentAuth
        );

        // x402 fallback (no header)
        assert_eq!(PaymentProtocol::detect(None), PaymentProtocol::X402);

        // x402 fallback (different auth scheme)
        assert_eq!(
            PaymentProtocol::detect(Some("Bearer token123")),
            PaymentProtocol::X402
        );
        assert_eq!(
            PaymentProtocol::detect(Some("Basic dXNlcjpwYXNz")),
            PaymentProtocol::X402
        );
    }

    #[test]
    fn test_payment_protocol_display() {
        assert_eq!(PaymentProtocol::X402.to_string(), "x402");
        assert_eq!(
            PaymentProtocol::WebPaymentAuth.to_string(),
            "Web Payment Auth"
        );
    }

    #[test]
    fn test_payment_protocol_helpers() {
        assert!(PaymentProtocol::X402.is_x402());
        assert!(!PaymentProtocol::X402.is_web_payment_auth());
        assert!(PaymentProtocol::WebPaymentAuth.is_web_payment_auth());
        assert!(!PaymentProtocol::WebPaymentAuth.is_x402());
    }

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
    fn test_payment_method_from_str() {
        assert_eq!(
            PaymentMethod::from_str("tempo").unwrap(),
            PaymentMethod::Tempo
        );
        assert_eq!(
            PaymentMethod::from_str("TEMPO").unwrap(),
            PaymentMethod::Tempo
        );
        assert_eq!(
            PaymentMethod::from_str("base").unwrap(),
            PaymentMethod::Base
        );
        assert_eq!(
            PaymentMethod::from_str("Base").unwrap(),
            PaymentMethod::Base
        );
        // Custom methods are accepted (but not supported for payments)
        assert_eq!(
            PaymentMethod::from_str("custom-method").unwrap(),
            PaymentMethod::Custom("custom-method".to_string())
        );
    }

    #[test]
    fn test_payment_method_network_name() {
        assert_eq!(
            PaymentMethod::Tempo.network_name(),
            Some(networks::TEMPO_MODERATO)
        );
        assert_eq!(
            PaymentMethod::Base.network_name(),
            Some(networks::BASE_SEPOLIA)
        );
        // Custom methods return None
        assert_eq!(
            PaymentMethod::Custom("unknown".to_string()).network_name(),
            None
        );
    }

    #[test]
    fn test_payment_method_is_supported() {
        assert!(PaymentMethod::Tempo.is_supported());
        assert!(PaymentMethod::Base.is_supported());
        assert!(!PaymentMethod::Custom("unknown".to_string()).is_supported());
    }

    #[test]
    fn test_payment_challenge_validate() {
        let valid_challenge = PaymentChallenge {
            id: "test123".to_string(),
            realm: "api".to_string(),
            method: PaymentMethod::Tempo,
            intent: PaymentIntent::Charge,
            request: serde_json::json!({}),
            expires: None,
            description: None,
        };
        assert!(valid_challenge.validate().is_ok());

        // Unsupported method
        let unsupported_method = PaymentChallenge {
            method: PaymentMethod::Custom("unknown".to_string()),
            ..valid_challenge.clone()
        };
        assert!(unsupported_method.validate().is_err());

        // Unsupported intent
        let unsupported_intent = PaymentChallenge {
            intent: PaymentIntent::Authorize,
            ..valid_challenge.clone()
        };
        assert!(unsupported_intent.validate().is_err());
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
