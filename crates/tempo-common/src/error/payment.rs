use thiserror::Error;

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
