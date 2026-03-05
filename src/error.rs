//! Error types for the  tempo-walletCLI.

use thiserror::Error;

#[derive(Error, Debug)]
pub(crate) enum PrestoError {
    /// Configuration file or value is missing
    #[error("Configuration missing: {0}")]
    ConfigMissing(String),

    /// Configuration is invalid
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    /// Invalid private key format
    #[error("Invalid private key: {0}")]
    InvalidKey(String),

    /// OS keychain operation failed
    #[error("Keychain error: {0}")]
    Keychain(String),

    /// Failed to determine config directory
    #[error("Failed to determine config directory")]
    NoConfigDir,

    /// Unknown network identifier
    #[error("Unknown network: {0}")]
    UnknownNetwork(String),

    /// Key is not provisioned on-chain
    #[error(
        "Key is not provisioned on-chain. Retry the request to auto-provision, or run '{hint}'."
    )]
    AccessKeyNotProvisioned { hint: String },

    /// Browser-based login expired (device code expired or callback window timed out)
    #[error("Login expired. Use  tempo-walletlogin to try again.")]
    LoginExpired,

    /// Key spending limit exceeded on-chain
    #[error("Spending limit exceeded: limit is {limit} {token}, need {required} {token}")]
    SpendingLimitExceeded {
        token: String,
        limit: String,
        required: String,
    },

    /// Insufficient token balance for payment
    #[error("Insufficient {token} balance: have {available}, need {required}. Fund with ' tempo-walletwallet fund'.")]
    InsufficientBalance {
        token: String,
        available: String,
        required: String,
    },

    /// Server rejected the payment after submission
    #[error("Payment rejected by server: {reason}")]
    PaymentRejected { reason: String, status_code: u16 },

    // ==================== Input Validation Errors ====================
    /// Signing error
    #[error("Signing error: {0}")]
    Signing(String),

    /// Address parsing error
    #[error("Invalid address: {0}")]
    InvalidAddress(String),

    /// Invalid URL provided
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    /// Invalid header (potential injection or malformed)
    #[error("Invalid header: {0}")]
    InvalidHeader(String),

    // ==================== Network Errors ====================
    /// HTTP request/response error
    #[error("HTTP error: {0}")]
    Http(String),

    /// Offline mode — no network access allowed
    #[error("Network access is disabled (--offline mode)")]
    OfflineMode,

    // ==================== Serialization Errors ====================
    /// JSON serialization/deserialization error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// TOML parsing error
    #[error("TOML parsing error: {0}")]
    TomlParse(#[from] toml::de::Error),

    /// TOML serialization error
    #[error("TOML serialization error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),

    // ==================== MPP Errors ====================
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

    /// Challenge has expired
    #[error("Challenge expired: {0}")]
    ChallengeExpired(String),

    // ==================== External Library Errors ====================
    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Reqwest error
    #[error("HTTP request error: {0}")]
    Reqwest(#[from] reqwest::Error),

    /// mpp protocol error
    #[error("{0}")]
    Mpp(#[from] mpp::MppError),
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_http_display() {
        let err = PrestoError::Http("404 Not Found".to_string());
        assert_eq!(err.to_string(), "HTTP error: 404 Not Found");
    }

    #[test]
    fn test_signing_simple_display() {
        let err = PrestoError::Signing("Failed to sign transaction".to_string());
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
    fn test_challenge_expired_display() {
        let err = PrestoError::ChallengeExpired("Expired 5 minutes ago".to_string());
        assert_eq!(err.to_string(), "Challenge expired: Expired 5 minutes ago");
    }
}
