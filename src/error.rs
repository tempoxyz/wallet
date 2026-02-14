//! Error types for the presto library.

use std::error::Error as StdError;
use thiserror::Error;

/// Result type alias for presto operations.
pub type Result<T> = std::result::Result<T, PrestoError>;

/// Context for signing errors
#[derive(Debug, Clone)]
pub struct SigningContext {
    pub network: Option<String>,
    pub address: Option<String>,
    pub operation: &'static str,
}

impl Default for SigningContext {
    fn default() -> Self {
        Self {
            network: None,
            address: None,
            operation: "sign",
        }
    }
}

impl std::fmt::Display for SigningContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "operation: {}", self.operation)?;
        if let Some(ref network) = self.network {
            write!(f, ", network: {}", network)?;
        }
        if let Some(ref address) = self.address {
            write!(f, ", address: {}", address)?;
        }
        Ok(())
    }
}

#[derive(Error, Debug)]
pub enum PrestoError {
    /// Required amount exceeds user's maximum allowed
    #[error("Required amount ({required}) exceeds maximum allowed ({max})")]
    AmountExceedsMax { required: u128, max: u128 },

    /// Invalid payment amount format
    #[error("Invalid amount: {0}")]
    InvalidAmount(String),

    /// Missing required payment field
    #[error("Missing payment requirement: {0}")]
    MissingRequirement(String),

    /// Configuration file or value is missing
    #[error("Configuration missing: {0}")]
    ConfigMissing(String),

    /// Configuration is invalid
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    /// Invalid private key format
    #[error("Invalid private key: {0}")]
    InvalidKey(String),

    /// Failed to determine config directory
    #[error("Failed to determine config directory")]
    NoConfigDir,

    /// Unknown network identifier
    #[error("Unknown network: {0}")]
    UnknownNetwork(String),

    /// Unsupported token type
    #[error("Unsupported token: {0}")]
    UnsupportedToken(String),

    /// Balance query failed
    #[error("Balance query failed: {0}")]
    BalanceQuery(String),

    /// Spending limit query failed
    #[error("Spending limit query failed: {0}")]
    SpendingLimitQuery(String),

    /// Access key spending limit exceeded on-chain
    #[error("Spending limit exceeded: limit is {limit} {token}, need {required} {token}")]
    SpendingLimitExceeded {
        token: String,
        limit: String,
        required: String,
    },

    /// Insufficient token balance for payment
    #[error("Insufficient {token} balance: have {available}, need {required}")]
    InsufficientBalance {
        token: String,
        available: String,
        required: String,
    },

    /// Server rejected the payment after submission
    #[error("Payment rejected by server: {reason}")]
    PaymentRejected { reason: String, status_code: u32 },

    // ==================== HTTP Errors ====================
    /// HTTP request/response error
    #[error("HTTP error: {0}")]
    Http(String),

    /// Unsupported HTTP method
    #[error("Unsupported HTTP method: {0}")]
    UnsupportedHttpMethod(String),

    /// EVM/Alloy signing error with context and source chain
    #[error("signing failed ({context})")]
    Signing {
        #[source]
        source: Box<dyn StdError + Send + Sync>,
        context: SigningContext,
    },

    /// Simple signing error for backwards compatibility
    #[error("Signing error: {0}")]
    SigningSimple(String),

    /// Address parsing error
    #[error("Invalid address: {0}")]
    InvalidAddress(String),

    /// JSON serialization/deserialization error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// TOML parsing error
    #[error("TOML parsing error: {0}")]
    TomlParse(#[from] toml::de::Error),

    /// TOML serialization error
    #[error("TOML serialization error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),

    /// Hex decoding error
    #[error("Hex decoding error: {0}")]
    HexDecode(#[from] hex::FromHexError),

    /// Base64 decoding error
    #[error("Base64 decoding error: {0}")]
    Base64Decode(#[from] base64::DecodeError),

    /// Base58 decoding error
    #[error("Base58 decoding error: {0}")]
    Base58Decode(#[from] bs58::decode::Error),

    // ==================== Web Payment Auth Errors ====================
    /// Unsupported payment method
    #[error("Unsupported payment method: {0}")]
    UnsupportedPaymentMethod(String),

    /// Unsupported payment intent
    #[error("Unsupported payment intent: {0}")]
    UnsupportedPaymentIntent(String),

    /// Invalid challenge format or content
    #[error("Invalid challenge: {0}")]
    InvalidChallenge(String),

    /// Missing required header
    #[error("Missing required header: {0}")]
    MissingHeader(String),

    /// Invalid base64url encoding
    #[error("Invalid base64url: {0}")]
    InvalidBase64Url(String),

    /// Challenge has expired
    #[error("Challenge expired: {0}")]
    ChallengeExpired(String),

    /// Invalid DID format
    #[error("Invalid DID: {0}")]
    InvalidDid(String),

    // ==================== External Library Errors ====================
    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Reqwest error
    #[error("HTTP request error: {0}")]
    Reqwest(#[from] reqwest::Error),

    /// Invalid UTF-8 in response
    #[error("Invalid UTF-8 in response body")]
    InvalidUtf8(#[from] std::string::FromUtf8Error),

    /// System time error
    #[error("System time error: {0}")]
    SystemTime(#[from] std::time::SystemTimeError),

    /// mpay protocol error
    #[error("{0}")]
    Mpay(#[from] mpay::MppError),
}

impl PrestoError {
    /// Create a signing error with context and source chain
    pub fn signing_with_context(
        source: impl StdError + Send + Sync + 'static,
        context: SigningContext,
    ) -> Self {
        Self::Signing {
            source: Box::new(source),
            context,
        }
    }

    /// Create a signing error with just a message (backwards compat)
    pub fn signing(msg: impl Into<String>) -> Self {
        Self::SigningSimple(msg.into())
    }

    /// Add network context to an existing error
    pub fn with_network(self, network: impl Into<String>) -> Self {
        match self {
            Self::Signing {
                source,
                mut context,
            } => {
                context.network = Some(network.into());
                Self::Signing { source, context }
            }
            other => other,
        }
    }

    /// Create an invalid address error
    pub fn invalid_address(msg: impl Into<String>) -> Self {
        Self::InvalidAddress(msg.into())
    }

    /// Create a config missing error
    pub fn config_missing(msg: impl Into<String>) -> Self {
        Self::ConfigMissing(msg.into())
    }

    /// Create an unsupported payment method error
    pub fn unsupported_method(method: &impl std::fmt::Display) -> Self {
        Self::UnsupportedPaymentMethod(format!("Payment method '{}' is not supported", method))
    }
}

/// Extension trait for adding context to Results
pub trait ResultExt<T> {
    /// Add signing context to an error
    fn with_signing_context(self, context: SigningContext) -> Result<T>;

    /// Add network context to an error
    #[allow(dead_code)]
    fn with_network(self, network: &str) -> Result<T>;
}

impl<T, E: StdError + Send + Sync + 'static> ResultExt<T> for std::result::Result<T, E> {
    fn with_signing_context(self, context: SigningContext) -> Result<T> {
        self.map_err(|e| PrestoError::signing_with_context(e, context))
    }

    fn with_network(self, network: &str) -> Result<T> {
        self.map_err(|e| {
            PrestoError::signing_with_context(
                e,
                SigningContext {
                    network: Some(network.to_string()),
                    ..Default::default()
                },
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_amount_exceeds_max_display() {
        let err = PrestoError::AmountExceedsMax {
            required: 1000,
            max: 500,
        };
        let display = err.to_string();
        assert!(display.contains("Required amount (1000) exceeds maximum allowed (500)"));
    }

    #[test]
    fn test_invalid_amount_display() {
        let err = PrestoError::InvalidAmount("not a number".to_string());
        assert_eq!(err.to_string(), "Invalid amount: not a number");
    }

    #[test]
    fn test_missing_requirement_display() {
        let err = PrestoError::MissingRequirement("network".to_string());
        assert_eq!(err.to_string(), "Missing payment requirement: network");
    }

    #[test]
    fn test_config_missing_display() {
        let err = PrestoError::ConfigMissing("wallet not configured".to_string());
        assert_eq!(
            err.to_string(),
            "Configuration missing: wallet not configured"
        );
    }

    #[test]
    fn test_invalid_config_display() {
        let err = PrestoError::InvalidConfig("invalid rpc url".to_string());
        assert_eq!(err.to_string(), "Invalid configuration: invalid rpc url");
    }

    #[test]
    fn test_invalid_key_display() {
        let err = PrestoError::InvalidKey("wrong format".to_string());
        assert_eq!(err.to_string(), "Invalid private key: wrong format");
    }

    #[test]
    fn test_no_config_dir_display() {
        let err = PrestoError::NoConfigDir;
        assert_eq!(err.to_string(), "Failed to determine config directory");
    }

    #[test]
    fn test_unknown_network_display() {
        let err = PrestoError::UnknownNetwork("custom-chain".to_string());
        assert_eq!(err.to_string(), "Unknown network: custom-chain");
    }

    #[test]
    fn test_unsupported_token_display() {
        let err = PrestoError::UnsupportedToken("UNKNOWN".to_string());
        assert_eq!(err.to_string(), "Unsupported token: UNKNOWN");
    }

    #[test]
    fn test_balance_query_display() {
        let err = PrestoError::BalanceQuery("RPC timeout".to_string());
        assert_eq!(err.to_string(), "Balance query failed: RPC timeout");
    }

    #[test]
    fn test_http_display() {
        let err = PrestoError::Http("404 Not Found".to_string());
        assert_eq!(err.to_string(), "HTTP error: 404 Not Found");
    }

    #[test]
    fn test_unsupported_http_method_display() {
        let err = PrestoError::UnsupportedHttpMethod("TRACE".to_string());
        assert_eq!(err.to_string(), "Unsupported HTTP method: TRACE");
    }

    #[test]
    fn test_signing_simple_display() {
        let err = PrestoError::SigningSimple("Failed to sign transaction".to_string());
        assert_eq!(err.to_string(), "Signing error: Failed to sign transaction");
    }

    #[test]
    fn test_invalid_address_display() {
        let err = PrestoError::InvalidAddress("Not a valid address".to_string());
        assert_eq!(err.to_string(), "Invalid address: Not a valid address");
    }

    #[test]
    fn test_unsupported_payment_method_display() {
        let err = PrestoError::UnsupportedPaymentMethod("bitcoin".to_string());
        assert_eq!(err.to_string(), "Unsupported payment method: bitcoin");
    }

    #[test]
    fn test_unsupported_payment_intent_display() {
        let err = PrestoError::UnsupportedPaymentIntent("subscription".to_string());
        assert_eq!(err.to_string(), "Unsupported payment intent: subscription");
    }

    #[test]
    fn test_invalid_challenge_display() {
        let err = PrestoError::InvalidChallenge("Malformed challenge".to_string());
        assert_eq!(err.to_string(), "Invalid challenge: Malformed challenge");
    }

    #[test]
    fn test_missing_header_display() {
        let err = PrestoError::MissingHeader("WWW-Authenticate".to_string());
        assert_eq!(err.to_string(), "Missing required header: WWW-Authenticate");
    }

    #[test]
    fn test_invalid_base64_url_display() {
        let err = PrestoError::InvalidBase64Url("Invalid padding".to_string());
        assert_eq!(err.to_string(), "Invalid base64url: Invalid padding");
    }

    #[test]
    fn test_challenge_expired_display() {
        let err = PrestoError::ChallengeExpired("Expired 5 minutes ago".to_string());
        assert_eq!(err.to_string(), "Challenge expired: Expired 5 minutes ago");
    }

    #[test]
    fn test_invalid_did_display() {
        let err = PrestoError::InvalidDid("Not a valid DID".to_string());
        assert_eq!(err.to_string(), "Invalid DID: Not a valid DID");
    }

    #[test]
    fn test_signing_constructor() {
        let err = PrestoError::signing("test error");
        assert!(matches!(err, PrestoError::SigningSimple(_)));
        assert_eq!(err.to_string(), "Signing error: test error");
    }

    #[test]
    fn test_signing_with_context() {
        use std::io::{Error as IoError, ErrorKind};
        let source = IoError::new(ErrorKind::Other, "underlying error");
        let ctx = SigningContext {
            network: Some("tempo".to_string()),
            address: Some("0x123".to_string()),
            operation: "sign_transaction",
        };
        let err = PrestoError::signing_with_context(source, ctx);
        let display = err.to_string();
        assert!(display.contains("signing failed"));
        assert!(display.contains("sign_transaction"));
        assert!(display.contains("tempo"));
        assert!(display.contains("0x123"));
    }

    #[test]
    fn test_signing_context_display() {
        let ctx = SigningContext {
            network: Some("ethereum".to_string()),
            address: Some("0xabc".to_string()),
            operation: "get_nonce",
        };
        let display = ctx.to_string();
        assert!(display.contains("operation: get_nonce"));
        assert!(display.contains("network: ethereum"));
        assert!(display.contains("address: 0xabc"));
    }

    #[test]
    fn test_signing_context_default() {
        let ctx = SigningContext::default();
        assert_eq!(ctx.operation, "sign");
        assert!(ctx.network.is_none());
        assert!(ctx.address.is_none());
    }

    #[test]
    fn test_result_ext_with_signing_context() {
        use std::io::{Error as IoError, ErrorKind};
        let result: std::result::Result<(), IoError> = Err(IoError::new(ErrorKind::Other, "test"));
        let ctx = SigningContext {
            network: Some("tempo".to_string()),
            address: None,
            operation: "test_op",
        };
        let presto_result = result.with_signing_context(ctx);
        assert!(presto_result.is_err());
        let err = presto_result.unwrap_err();
        assert!(err.to_string().contains("signing failed"));
    }

    #[test]
    fn test_result_ext_with_network() {
        use std::io::{Error as IoError, ErrorKind};
        let result: std::result::Result<(), IoError> = Err(IoError::new(ErrorKind::Other, "test"));
        let presto_result = result.with_network("base-sepolia");
        assert!(presto_result.is_err());
        let err = presto_result.unwrap_err();
        assert!(err.to_string().contains("base-sepolia"));
    }

    #[test]
    fn test_with_network_on_signing_error() {
        use std::io::{Error as IoError, ErrorKind};
        let source = IoError::new(ErrorKind::Other, "test");
        let err = PrestoError::signing_with_context(source, SigningContext::default());
        let err_with_network = err.with_network("optimism");
        assert!(err_with_network.to_string().contains("optimism"));
    }

    #[test]
    fn test_invalid_address_constructor() {
        let err = PrestoError::invalid_address("test address");
        assert!(matches!(err, PrestoError::InvalidAddress(_)));
        assert_eq!(err.to_string(), "Invalid address: test address");
    }

    #[test]
    fn test_config_missing_constructor() {
        let err = PrestoError::config_missing("test config");
        assert!(matches!(err, PrestoError::ConfigMissing(_)));
        assert_eq!(err.to_string(), "Configuration missing: test config");
    }

    #[test]
    fn test_unsupported_method_constructor() {
        let err = PrestoError::unsupported_method(&"bitcoin");
        assert!(matches!(err, PrestoError::UnsupportedPaymentMethod(_)));
        assert!(err.to_string().contains("bitcoin"));
        assert!(err.to_string().contains("not supported"));
    }
}
