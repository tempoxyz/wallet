//! Shared error types for Tempo CLI.

mod config;
mod input;
mod key;
mod network;
mod payment;

pub use config::ConfigError;
pub use input::InputError;
pub use key::KeyError;
pub use network::NetworkError;
pub use payment::PaymentError;

use thiserror::Error;

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
        assert_eq!(err.to_string(), "Failed to determine config directory");
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
