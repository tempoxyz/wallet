use thiserror::Error;

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
