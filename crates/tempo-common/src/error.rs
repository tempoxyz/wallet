//! Shared error types for Tempo CLI.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Configuration missing: {0}")]
    Missing(String),
    #[error("Invalid configuration: {0}")]
    Invalid(String),
    #[error("Failed to determine home directory")]
    NoConfigDir,
}

#[derive(Error, Debug)]
pub enum InputError {
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),
    #[error("Invalid header: {0}")]
    InvalidHeader(String),
    #[error("Invalid output path: {0}")]
    InvalidOutputPath(String),
    #[error("Request body exceeds maximum size of {0} bytes")]
    BodyTooLarge(usize),
    #[error("Request header exceeds maximum size of {0} bytes")]
    HeaderTooLarge(usize),
    #[error("failed to read stdin: {0}")]
    ReadStdin(#[source] std::io::Error),
    #[error("failed to read file '{path}': {source}")]
    ReadFile {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Error, Debug)]
pub enum KeyError {
    #[error("Invalid private key: {0}")]
    InvalidKey(String),
    #[error("Keychain error: {0}")]
    Keychain(String),
    #[error("Signing error: {0}")]
    Signing(String),
    #[error("Invalid address: {0}")]
    InvalidAddress(String),
    #[error("Login expired. Use tempo-wallet login to try again.")]
    LoginExpired,
}

#[derive(Error, Debug)]
pub enum NetworkError {
    #[error("Unknown network: {0}")]
    UnknownNetwork(String),
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("HTTP request error: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("402 Payment Required (payment is not supported in streaming mode)")]
    StreamingPaymentUnsupported,
    #[error("Network access is disabled (--offline mode)")]
    OfflineMode,
}

#[derive(Error, Debug)]
pub enum PaymentError {
    #[error("Spending limit exceeded: limit is {limit} {token}, need {required} {token}")]
    SpendingLimitExceeded {
        token: String,
        limit: String,
        required: String,
    },
    #[error("Insufficient {token} balance: have {available}, need {required}. Fund with 'tempo-wallet wallets fund'.")]
    InsufficientBalance {
        token: String,
        available: String,
        required: String,
    },
    #[error("Payment rejected by server: {reason}")]
    PaymentRejected { reason: String, status_code: u16 },
    #[error("Transaction reverted: {0}")]
    TransactionReverted(String),
    #[error("Channel {channel_id} not found on {network}")]
    ChannelNotFound { channel_id: String, network: String },
    #[error(
        "Key is not provisioned on-chain. Retry the request to auto-provision, or run '{hint}'."
    )]
    AccessKeyNotProvisioned { hint: String },
    #[error("Unsupported payment method: {0}")]
    UnsupportedPaymentMethod(String),
    #[error("Unsupported payment intent: {0}")]
    UnsupportedPaymentIntent(String),
    #[error("Invalid challenge: {0}")]
    InvalidChallenge(String),
    #[error("Missing required header: {0}")]
    MissingHeader(String),
    #[error("Challenge expired: {0}")]
    ChallengeExpired(String),
    #[error("{0}")]
    Mpp(#[from] mpp::MppError),
}

#[derive(Error, Debug)]
pub enum TempoError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Key(#[from] KeyError),
    #[error(transparent)]
    Input(#[from] InputError),
    #[error(transparent)]
    Network(#[from] NetworkError),
    #[error(transparent)]
    Payment(#[from] PaymentError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("TOML parsing error: {0}")]
    TomlParse(#[from] toml::de::Error),
    #[error("TOML serialization error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_missing_display() {
        let err: TempoError = ConfigError::Missing("wallet not configured".to_string()).into();
        assert_eq!(
            err.to_string(),
            "Configuration missing: wallet not configured"
        );
    }

    #[test]
    fn test_invalid_config_display() {
        let err: TempoError = ConfigError::Invalid("invalid rpc url".to_string()).into();
        assert_eq!(err.to_string(), "Invalid configuration: invalid rpc url");
    }

    #[test]
    fn test_invalid_key_display() {
        let err: TempoError = KeyError::InvalidKey("wrong format".to_string()).into();
        assert_eq!(err.to_string(), "Invalid private key: wrong format");
    }

    #[test]
    fn test_no_config_dir_display() {
        let err: TempoError = ConfigError::NoConfigDir.into();
        assert_eq!(err.to_string(), "Failed to determine home directory");
    }

    #[test]
    fn test_unknown_network_display() {
        let err: TempoError = NetworkError::UnknownNetwork("custom-chain".to_string()).into();
        assert_eq!(err.to_string(), "Unknown network: custom-chain");
    }

    #[test]
    fn test_http_display() {
        let err: TempoError = NetworkError::Http("404 Not Found".to_string()).into();
        assert_eq!(err.to_string(), "HTTP error: 404 Not Found");
    }

    #[test]
    fn test_signing_simple_display() {
        let err: TempoError = KeyError::Signing("Failed to sign transaction".to_string()).into();
        assert_eq!(err.to_string(), "Signing error: Failed to sign transaction");
    }

    #[test]
    fn test_invalid_address_display() {
        let err: TempoError = KeyError::InvalidAddress("Not a valid address".to_string()).into();
        assert_eq!(err.to_string(), "Invalid address: Not a valid address");
    }

    #[test]
    fn test_unsupported_payment_method_display() {
        let err: TempoError = PaymentError::UnsupportedPaymentMethod("bitcoin".to_string()).into();
        assert_eq!(err.to_string(), "Unsupported payment method: bitcoin");
    }

    #[test]
    fn test_unsupported_payment_intent_display() {
        let err: TempoError =
            PaymentError::UnsupportedPaymentIntent("subscription".to_string()).into();
        assert_eq!(err.to_string(), "Unsupported payment intent: subscription");
    }

    #[test]
    fn test_invalid_challenge_display() {
        let err: TempoError =
            PaymentError::InvalidChallenge("Malformed challenge".to_string()).into();
        assert_eq!(err.to_string(), "Invalid challenge: Malformed challenge");
    }

    #[test]
    fn test_missing_header_display() {
        let err: TempoError = PaymentError::MissingHeader("WWW-Authenticate".to_string()).into();
        assert_eq!(err.to_string(), "Missing required header: WWW-Authenticate");
    }

    #[test]
    fn test_challenge_expired_display() {
        let err: TempoError =
            PaymentError::ChallengeExpired("Expired 5 minutes ago".to_string()).into();
        assert_eq!(err.to_string(), "Challenge expired: Expired 5 minutes ago");
    }
}
