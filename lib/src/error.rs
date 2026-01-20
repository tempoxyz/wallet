//! Error types for the purl library.

use thiserror::Error;

/// Result type alias for purl operations.
pub type Result<T> = std::result::Result<T, PurlError>;

#[derive(Error, Debug)]
pub enum PurlError {
    #[error("Payment provider not found for network: {0}")]
    ProviderNotFound(String),

    /// No payment methods are configured
    #[error("No payment methods configured")]
    NoPaymentMethods,

    /// No compatible payment method for the server's requirements
    #[error("No compatible payment method found. Available networks: {networks:?}")]
    NoCompatibleMethod { networks: Vec<String> },

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

    /// Token not configured for network
    #[error("Token configuration not found for asset {asset} on network {network}")]
    TokenConfigNotFound { asset: String, network: String },

    /// Unsupported token type
    #[error("Unsupported token: {0}")]
    UnsupportedToken(String),

    /// Balance query failed
    #[error("Balance query failed: {0}")]
    BalanceQuery(String),

    // ==================== HTTP Errors ====================
    /// HTTP request/response error
    #[error("HTTP error: {0}")]
    Http(String),

    /// Unsupported HTTP method
    #[error("Unsupported HTTP method: {0}")]
    UnsupportedHttpMethod(String),

    /// EVM/Alloy signing error
    #[error("Signing error: {0}")]
    Signing(String),

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
    #[cfg(feature = "utils")]
    #[error("Hex decoding error: {0}")]
    HexDecode(#[from] hex::FromHexError),

    /// Base64 decoding error
    #[cfg(feature = "utils")]
    #[error("Base64 decoding error: {0}")]
    Base64Decode(#[from] base64::DecodeError),

    /// Base58 decoding error
    #[cfg(feature = "utils")]
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
}

impl PurlError {
    /// Create a signing error
    pub fn signing(msg: impl Into<String>) -> Self {
        Self::Signing(msg.into())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_not_found_display() {
        let err = PurlError::ProviderNotFound("ethereum".to_string());
        assert_eq!(
            err.to_string(),
            "Payment provider not found for network: ethereum"
        );
    }

    #[test]
    fn test_no_payment_methods_display() {
        let err = PurlError::NoPaymentMethods;
        assert_eq!(err.to_string(), "No payment methods configured");
    }

    #[test]
    fn test_no_compatible_method_display() {
        let err = PurlError::NoCompatibleMethod {
            networks: vec!["ethereum".to_string(), "base".to_string()],
        };
        let display = err.to_string();
        assert!(display.contains("No compatible payment method found"));
        assert!(display.contains("ethereum"));
        assert!(display.contains("base"));
    }

    #[test]
    fn test_amount_exceeds_max_display() {
        let err = PurlError::AmountExceedsMax {
            required: 1000,
            max: 500,
        };
        let display = err.to_string();
        assert!(display.contains("Required amount (1000) exceeds maximum allowed (500)"));
    }

    #[test]
    fn test_invalid_amount_display() {
        let err = PurlError::InvalidAmount("not a number".to_string());
        assert_eq!(err.to_string(), "Invalid amount: not a number");
    }

    #[test]
    fn test_missing_requirement_display() {
        let err = PurlError::MissingRequirement("network".to_string());
        assert_eq!(err.to_string(), "Missing payment requirement: network");
    }

    #[test]
    fn test_config_missing_display() {
        let err = PurlError::ConfigMissing("wallet not configured".to_string());
        assert_eq!(
            err.to_string(),
            "Configuration missing: wallet not configured"
        );
    }

    #[test]
    fn test_invalid_config_display() {
        let err = PurlError::InvalidConfig("invalid rpc url".to_string());
        assert_eq!(err.to_string(), "Invalid configuration: invalid rpc url");
    }

    #[test]
    fn test_invalid_key_display() {
        let err = PurlError::InvalidKey("wrong format".to_string());
        assert_eq!(err.to_string(), "Invalid private key: wrong format");
    }

    #[test]
    fn test_no_config_dir_display() {
        let err = PurlError::NoConfigDir;
        assert_eq!(err.to_string(), "Failed to determine config directory");
    }

    #[test]
    fn test_unknown_network_display() {
        let err = PurlError::UnknownNetwork("custom-chain".to_string());
        assert_eq!(err.to_string(), "Unknown network: custom-chain");
    }

    #[test]
    fn test_token_config_not_found_display() {
        let err = PurlError::TokenConfigNotFound {
            asset: "USDC".to_string(),
            network: "ethereum".to_string(),
        };
        let display = err.to_string();
        assert!(
            display.contains("Token configuration not found for asset USDC on network ethereum")
        );
    }

    #[test]
    fn test_unsupported_token_display() {
        let err = PurlError::UnsupportedToken("UNKNOWN".to_string());
        assert_eq!(err.to_string(), "Unsupported token: UNKNOWN");
    }

    #[test]
    fn test_balance_query_display() {
        let err = PurlError::BalanceQuery("RPC timeout".to_string());
        assert_eq!(err.to_string(), "Balance query failed: RPC timeout");
    }

    #[test]
    fn test_http_display() {
        let err = PurlError::Http("404 Not Found".to_string());
        assert_eq!(err.to_string(), "HTTP error: 404 Not Found");
    }

    #[test]
    fn test_unsupported_http_method_display() {
        let err = PurlError::UnsupportedHttpMethod("TRACE".to_string());
        assert_eq!(err.to_string(), "Unsupported HTTP method: TRACE");
    }

    #[test]
    fn test_signing_display() {
        let err = PurlError::Signing("Failed to sign transaction".to_string());
        assert_eq!(err.to_string(), "Signing error: Failed to sign transaction");
    }

    #[test]
    fn test_invalid_address_display() {
        let err = PurlError::InvalidAddress("Not a valid address".to_string());
        assert_eq!(err.to_string(), "Invalid address: Not a valid address");
    }

    #[test]
    fn test_unsupported_payment_method_display() {
        let err = PurlError::UnsupportedPaymentMethod("bitcoin".to_string());
        assert_eq!(err.to_string(), "Unsupported payment method: bitcoin");
    }

    #[test]
    fn test_unsupported_payment_intent_display() {
        let err = PurlError::UnsupportedPaymentIntent("subscription".to_string());
        assert_eq!(err.to_string(), "Unsupported payment intent: subscription");
    }

    #[test]
    fn test_invalid_challenge_display() {
        let err = PurlError::InvalidChallenge("Malformed challenge".to_string());
        assert_eq!(err.to_string(), "Invalid challenge: Malformed challenge");
    }

    #[test]
    fn test_missing_header_display() {
        let err = PurlError::MissingHeader("WWW-Authenticate".to_string());
        assert_eq!(err.to_string(), "Missing required header: WWW-Authenticate");
    }

    #[test]
    fn test_invalid_base64_url_display() {
        let err = PurlError::InvalidBase64Url("Invalid padding".to_string());
        assert_eq!(err.to_string(), "Invalid base64url: Invalid padding");
    }

    #[test]
    fn test_challenge_expired_display() {
        let err = PurlError::ChallengeExpired("Expired 5 minutes ago".to_string());
        assert_eq!(err.to_string(), "Challenge expired: Expired 5 minutes ago");
    }

    #[test]
    fn test_invalid_did_display() {
        let err = PurlError::InvalidDid("Not a valid DID".to_string());
        assert_eq!(err.to_string(), "Invalid DID: Not a valid DID");
    }

    #[test]
    fn test_signing_constructor() {
        let err = PurlError::signing("test error");
        assert!(matches!(err, PurlError::Signing(_)));
        assert_eq!(err.to_string(), "Signing error: test error");
    }

    #[test]
    fn test_invalid_address_constructor() {
        let err = PurlError::invalid_address("test address");
        assert!(matches!(err, PurlError::InvalidAddress(_)));
        assert_eq!(err.to_string(), "Invalid address: test address");
    }

    #[test]
    fn test_config_missing_constructor() {
        let err = PurlError::config_missing("test config");
        assert!(matches!(err, PurlError::ConfigMissing(_)));
        assert_eq!(err.to_string(), "Configuration missing: test config");
    }

    #[test]
    fn test_unsupported_method_constructor() {
        let err = PurlError::unsupported_method(&"bitcoin");
        assert!(matches!(err, PurlError::UnsupportedPaymentMethod(_)));
        assert!(err.to_string().contains("bitcoin"));
        assert!(err.to_string().contains("not supported"));
    }
}
