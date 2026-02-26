//! Error types for the  tempo-walletlibrary.

use thiserror::Error;

#[derive(Error, Debug)]
#[allow(dead_code)]
pub(crate) enum PrestoError {
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
    #[error("Key is not provisioned on-chain. Run ' tempo-walletlogin' to set up your key.")]
    AccessKeyNotProvisioned,

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
    #[error("Insufficient {token} balance: have {available}, need {required}")]
    InsufficientBalance {
        token: String,
        available: String,
        required: String,
    },

    /// Server rejected the payment after submission
    #[error("Payment rejected by server: {reason}")]
    PaymentRejected { reason: String, status_code: u16 },

    // ==================== HTTP Errors ====================
    /// HTTP request/response error
    #[error("HTTP error: {0}")]
    Http(String),

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

    /// Invalid UTF-8 in response
    #[error("Invalid UTF-8 in response body")]
    InvalidUtf8(#[from] std::string::FromUtf8Error),

    /// System time error
    #[error("System time error: {0}")]
    SystemTime(#[from] std::time::SystemTimeError),

    /// mpp protocol error
    #[error("{0}")]
    Mpp(#[from] mpp::MppError),
}

/// Map mpp validation errors to  tempo-walleterror types.
pub(crate) fn map_mpp_validation_error(
    e: mpp::MppError,
    challenge: &mpp::PaymentChallenge,
) -> PrestoError {
    match e {
        mpp::MppError::UnsupportedPaymentMethod(msg) => PrestoError::UnsupportedPaymentMethod(msg),
        mpp::MppError::PaymentExpired(_) => {
            PrestoError::ChallengeExpired(challenge.expires.clone().unwrap_or_default())
        }
        mpp::MppError::InvalidChallenge { reason, .. } => {
            PrestoError::UnsupportedPaymentIntent(reason.unwrap_or_default())
        }
        other => PrestoError::InvalidChallenge(other.to_string()),
    }
}

/// Classify an mpp provider error into a PrestoError with actionable context.
pub(crate) fn classify_payment_error(err: mpp::MppError) -> PrestoError {
    use mpp::client::TempoClientError;

    match err {
        mpp::MppError::Tempo(tempo_err) => match tempo_err {
            TempoClientError::AccessKeyNotProvisioned => PrestoError::AccessKeyNotProvisioned,
            TempoClientError::SpendingLimitExceeded {
                token,
                limit,
                required,
            } => PrestoError::SpendingLimitExceeded {
                token,
                limit,
                required,
            },
            TempoClientError::InsufficientBalance {
                token,
                available,
                required,
            } => PrestoError::InsufficientBalance {
                token,
                available,
                required,
            },
            TempoClientError::TransactionReverted(msg) => PrestoError::Http(msg),
        },
        other => {
            let raw = other.to_string();
            let msg = raw.strip_prefix("HTTP error: ").unwrap_or(&raw).to_string();
            PrestoError::Http(msg)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_classify_spending_limit() {
        let err = mpp::MppError::Tempo(mpp::client::TempoClientError::SpendingLimitExceeded {
            token: "pathUSD".to_string(),
            limit: "0.000000".to_string(),
            required: "0.010000".to_string(),
        });
        match classify_payment_error(err) {
            PrestoError::SpendingLimitExceeded {
                token,
                limit,
                required,
            } => {
                assert_eq!(token, "pathUSD");
                assert_eq!(limit, "0.000000");
                assert_eq!(required, "0.010000");
            }
            other => panic!("Expected SpendingLimitExceeded, got: {other}"),
        }
    }

    #[test]
    fn test_classify_insufficient_balance() {
        let err = mpp::MppError::Tempo(mpp::client::TempoClientError::InsufficientBalance {
            token: "pathUSD".to_string(),
            available: "0.50".to_string(),
            required: "1.00".to_string(),
        });
        match classify_payment_error(err) {
            PrestoError::InsufficientBalance {
                token,
                available,
                required,
            } => {
                assert_eq!(token, "pathUSD");
                assert_eq!(available, "0.50");
                assert_eq!(required, "1.00");
            }
            other => panic!("Expected InsufficientBalance, got: {other}"),
        }
    }

    #[test]
    fn test_classify_key_not_provisioned() {
        let err = mpp::MppError::Tempo(mpp::client::TempoClientError::AccessKeyNotProvisioned);
        assert!(matches!(
            classify_payment_error(err),
            PrestoError::AccessKeyNotProvisioned
        ));
    }

    #[test]
    fn test_classify_unrecognized_falls_through() {
        let err = mpp::MppError::Http("something unexpected".to_string());
        match classify_payment_error(err) {
            PrestoError::Http(msg) => assert_eq!(msg, "something unexpected"),
            other => panic!("Expected Http passthrough, got: {other}"),
        }
    }
}
