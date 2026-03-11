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
    #[error("Invalid hex input: {0}")]
    InvalidHexInput(String),
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
    fn test_payment_rejected_display() {
        let err = PaymentError::PaymentRejected {
            reason: "invalid signature".to_string(),
            status_code: 403,
        };
        assert_eq!(
            err.to_string(),
            "Payment rejected by server: invalid signature"
        );
    }

    #[test]
    fn test_transaction_reverted_display() {
        let err = PaymentError::TransactionReverted("out of gas".to_string());
        assert_eq!(err.to_string(), "Transaction reverted: out of gas");
    }

    #[test]
    fn test_channel_not_found_display() {
        let err = PaymentError::ChannelNotFound {
            channel_id: "0x123".to_string(),
            network: "tempo".to_string(),
        };
        assert_eq!(err.to_string(), "Channel 0x123 not found on tempo");
    }

    #[test]
    fn test_access_key_not_provisioned_display() {
        let err = PaymentError::AccessKeyNotProvisioned {
            hint: "tempo-wallet keys provision".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Key is not provisioned on-chain. Retry the request to auto-provision, or run 'tempo-wallet keys provision'."
        );
    }

    #[test]
    fn test_insufficient_balance_display() {
        let err = PaymentError::InsufficientBalance {
            token: "USDC".to_string(),
            available: "1.00".to_string(),
            required: "5.00".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Insufficient USDC balance: have 1.00, need 5.00. Fund with 'tempo-wallet wallets fund'."
        );
    }

    #[test]
    fn test_spending_limit_exceeded_display() {
        let err = PaymentError::SpendingLimitExceeded {
            token: "USDC".to_string(),
            limit: "10.00".to_string(),
            required: "20.00".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Spending limit exceeded: limit is 10.00 USDC, need 20.00 USDC"
        );
    }
}
