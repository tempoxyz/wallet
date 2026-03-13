//! Shared error types for Tempo CLI.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Configuration missing: {0}")]
    Missing(String),
    #[error("Invalid configuration: {0}")]
    Invalid(String),
    #[error("Invalid config path: path traversal (..) not allowed")]
    InvalidConfigPathTraversal,
    #[error("Failed to read config file at {path}: {source}")]
    ReadConfigFile {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("Failed to parse config file at {path}: {source}")]
    ParseConfigFile {
        path: String,
        #[source]
        source: toml::de::Error,
    },
    #[error("invalid {context} URL: {source}")]
    InvalidUrl {
        context: &'static str,
        #[source]
        source: url::ParseError,
    },
    #[error("invalid proxy URL: {source}")]
    InvalidProxyUrl {
        #[source]
        source: reqwest::Error,
    },
    #[error("Unsupported chainId: {0}")]
    UnsupportedChainId(u64),
    #[error("invalid {context} address: {value}")]
    InvalidAddress {
        context: &'static str,
        value: String,
    },
    #[error("invalid key authorization")]
    InvalidKeyAuthorization,
    #[error("Failed to initialize {provider}: {reason}")]
    ProviderInit {
        provider: &'static str,
        reason: String,
    },
    #[error("Failed to initialize {provider}: {source}")]
    ProviderInitSource {
        provider: &'static str,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("Failed to determine home directory")]
    NoConfigDir,
}

#[derive(Error, Debug)]
pub enum InputError {
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),
    #[error("Invalid URL: {0}")]
    UrlParse(#[source] url::ParseError),
    #[error("Invalid {context} URL: {source}")]
    UrlParseFor {
        context: &'static str,
        #[source]
        source: url::ParseError,
    },
    #[error("Invalid URL: unsupported scheme '{0}'")]
    UnsupportedUrlScheme(String),
    #[error("Invalid HTTP method: {0}")]
    InvalidMethod(String),
    #[error("Invalid TOON input: {0}")]
    InvalidToonInput(#[source] toon_format::ToonError),
    #[error("Invalid header: {0}")]
    InvalidHeader(String),
    #[error("Invalid header: header contains CR/LF characters")]
    HeaderContainsControlChars,
    #[error("Invalid output path: {0}")]
    InvalidOutputPath(String),
    #[error(
        "Specify a URL, channel ID (0x...), or use --all/--orphaned/--finalize to close sessions"
    )]
    MissingSessionCloseTarget,
    #[error("Invalid channel ID format: expected 0x-prefixed bytes32 hex")]
    InvalidChannelIdFormat,
    #[error("channel ID must be 66 characters (0x + 64 hex digits), got {actual}")]
    InvalidChannelIdLength { actual: usize },
    #[error("invalid channel ID '{value}': expected 0x-prefixed bytes32 hex")]
    InvalidChannelIdValue { value: String },
    #[error("data is not valid UTF-8 for --get: {source}")]
    GetDataNotUtf8 {
        #[source]
        source: std::string::FromUtf8Error,
    },
    #[error("No challenge provided. Use --challenge or pipe via stdin.")]
    MissingChallenge,
    #[error("Missing account_address in authorized response")]
    MissingAuthorizedAccountAddress,
    #[error("Invalid output path: path traversal (..) not allowed")]
    OutputPathTraversal,
    #[error("Invalid output path: resolved path escapes working directory")]
    OutputPathEscapesWorkingDirectory,
    #[error("Request body exceeds maximum size of {0} bytes")]
    BodyTooLarge(usize),
    #[error("Request header exceeds maximum size of {0} bytes")]
    HeaderTooLarge(usize),
    #[error("Invalid hex input: {0}")]
    InvalidHexInput(String),
    #[error("Use --yes for non-interactive mode")]
    NonInteractiveConfirmationRequired,
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
    #[error("Invalid private key format")]
    InvalidKeyFormat,
    #[error("Signing error: {0}")]
    Signing(String),
    #[error("Signing error during {operation}: {reason}")]
    SigningOperation {
        operation: &'static str,
        reason: String,
    },
    #[error("Signing error during {operation}: {source}")]
    SigningOperationSource {
        operation: &'static str,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("Invalid address: {0}")]
    InvalidAddress(String),
    #[error("Login expired. Use `tempo wallet login` to try again.")]
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
    #[error("{}", format_http_status_error(.operation, *.status, .body.as_deref()))]
    HttpStatus {
        operation: &'static str,
        status: u16,
        body: Option<String>,
    },
    #[error("RPC error during {operation}: {reason}")]
    Rpc {
        operation: &'static str,
        reason: String,
    },
    #[error("RPC error during {operation}: {source}")]
    RpcSource {
        operation: &'static str,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("Failed to parse {context}: {source}")]
    ResponseParse {
        context: &'static str,
        #[source]
        source: serde_json::Error,
    },
    #[error("Malformed {context}: {reason}")]
    ResponseSchema {
        context: &'static str,
        reason: String,
    },
    #[error("Malformed {context}: missing {field}")]
    ResponseMissingField {
        context: &'static str,
        field: &'static str,
    },
    #[error("Malformed {context}: no {entry} found")]
    ResponseMissingEntry {
        context: &'static str,
        entry: &'static str,
    },
    #[error("Malformed {context}: {source}")]
    ResponseSchemaSource {
        context: &'static str,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("402 Payment Required (payment is not supported in streaming mode)")]
    StreamingPaymentUnsupported,
    #[error("Network access is disabled (--offline mode)")]
    OfflineMode,
}

fn format_http_status_error(operation: &str, status: u16, body: Option<&str>) -> String {
    match body {
        Some(text) if !text.is_empty() => format!("HTTP {status} during {operation}: {text}"),
        _ => format!("HTTP {status} during {operation}"),
    }
}

#[derive(Error, Debug)]
pub enum PaymentError {
    #[error("Spending limit exceeded: limit is {limit} {token}, need {required} {token}")]
    SpendingLimitExceeded {
        token: String,
        limit: String,
        required: String,
    },
    #[error("Insufficient {token} balance: have {available}, need {required}. Fund with 'tempo wallet fund'.")]
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
    #[error("Failed to parse {context}: {reason}")]
    ChallengeParse {
        context: &'static str,
        reason: String,
    },
    #[error("Failed to parse {context}: {source}")]
    ChallengeParseSource {
        context: &'static str,
        #[source]
        source: Box<mpp::MppError>,
    },
    #[error("Failed to format {context}: {reason}")]
    ChallengeFormat {
        context: &'static str,
        reason: String,
    },
    #[error("Failed to format {context}: {source}")]
    ChallengeFormatSource {
        context: &'static str,
        #[source]
        source: Box<mpp::MppError>,
    },
    #[error("Malformed {context}: {reason}")]
    ChallengeSchema {
        context: &'static str,
        reason: String,
    },
    #[error("Malformed {context}: {source}")]
    ChallengeSchemaSource {
        context: &'static str,
        #[source]
        source: Box<mpp::MppError>,
    },
    #[error("Malformed {context}: unsupported payload")]
    ChallengeUnsupportedPayload { context: &'static str },
    #[error("Malformed {context}: missing {field}")]
    ChallengeMissingField {
        context: &'static str,
        field: &'static str,
    },
    #[error("Malformed {context}: unsupported chainId: {chain_id}")]
    ChallengeUnsupportedChainId {
        context: &'static str,
        chain_id: u64,
    },
    #[error(
        "Malformed {context}: challenge network '{challenge_network}' does not match --network '{configured_network}'"
    )]
    ChallengeNetworkMismatch {
        context: &'static str,
        challenge_network: String,
        configured_network: String,
    },
    #[error(
        "Malformed {context}: untrusted escrow contract: {provided} (expected {expected} for network {network})"
    )]
    ChallengeUntrustedEscrow {
        context: &'static str,
        provided: String,
        expected: String,
        network: String,
    },
    #[error("Malformed {context}: invalid address: {source}")]
    ChallengeAddressParse {
        context: &'static str,
        #[source]
        source: Box<alloy::hex::FromHexError>,
    },
    #[error("Malformed {context}: {source}")]
    ChallengeValueParse {
        context: &'static str,
        #[source]
        source: Box<std::num::ParseIntError>,
    },
    #[error("Missing required header: {0}")]
    MissingHeader(String),
    #[error("Challenge expired: {0}")]
    ChallengeExpired(String),
    #[error("Session persistence error during {operation}: {reason}")]
    SessionPersistence {
        operation: &'static str,
        reason: String,
    },
    #[error("Session persistence error during {operation}: {source}")]
    SessionPersistenceSource {
        operation: &'static str,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("Session persistence error during {operation}: {context}: {source:#}")]
    SessionPersistenceContextSource {
        operation: &'static str,
        context: &'static str,
        #[source]
        source: Box<TempoError>,
    },
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
    #[error("TOON encoding error: {0}")]
    ToonEncode(#[source] toon_format::ToonError),
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
    fn test_provider_init_source_display() {
        let err = ConfigError::ProviderInitSource {
            provider: "tempo payment provider",
            source: Box::new(std::io::Error::other("bad rpc url")),
        };

        assert_eq!(
            err.to_string(),
            "Failed to initialize tempo payment provider: bad rpc url"
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
            hint: "tempo wallet login".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Key is not provisioned on-chain. Retry the request to auto-provision, or run 'tempo wallet login'."
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
            "Insufficient USDC balance: have 1.00, need 5.00. Fund with 'tempo wallet fund'."
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

    #[test]
    fn test_network_response_parse_display() {
        let source = serde_json::from_str::<serde_json::Value>("not-json").unwrap_err();
        let err = NetworkError::ResponseParse {
            context: "relay quote response",
            source,
        };

        assert!(
            err.to_string()
                .contains("Failed to parse relay quote response"),
            "unexpected display: {err}"
        );
    }

    #[test]
    fn test_signing_operation_source_display() {
        let err = KeyError::SigningOperationSource {
            operation: "sign voucher",
            source: Box::new(std::io::Error::other("hardware signer unavailable")),
        };

        assert_eq!(
            err.to_string(),
            "Signing error during sign voucher: hardware signer unavailable"
        );
    }

    #[test]
    fn test_network_rpc_display() {
        let err = NetworkError::Rpc {
            operation: "broadcast transaction",
            reason: "nonce too low".to_string(),
        };

        assert_eq!(
            err.to_string(),
            "RPC error during broadcast transaction: nonce too low"
        );
    }

    #[test]
    fn test_network_rpc_source_display() {
        let err = NetworkError::RpcSource {
            operation: "broadcast transaction",
            source: Box::new(std::io::Error::other("nonce too low")),
        };

        assert_eq!(
            err.to_string(),
            "RPC error during broadcast transaction: nonce too low"
        );
    }

    #[test]
    fn test_network_http_status_display() {
        let err = NetworkError::HttpStatus {
            operation: "poll login status",
            status: 502,
            body: Some("bad gateway".to_string()),
        };

        assert_eq!(
            err.to_string(),
            "HTTP 502 during poll login status: bad gateway"
        );
    }

    #[test]
    fn test_network_response_schema_display() {
        let err = NetworkError::ResponseSchema {
            context: "relay quote response",
            reason: "missing steps field".to_string(),
        };

        assert_eq!(
            err.to_string(),
            "Malformed relay quote response: missing steps field"
        );
    }

    #[test]
    fn test_network_response_missing_field_display() {
        let err = NetworkError::ResponseMissingField {
            context: "relay quote response",
            field: "steps",
        };

        assert_eq!(
            err.to_string(),
            "Malformed relay quote response: missing steps"
        );
    }

    #[test]
    fn test_network_response_missing_entry_display() {
        let err = NetworkError::ResponseMissingEntry {
            context: "relay quote response",
            entry: "deposit step",
        };

        assert_eq!(
            err.to_string(),
            "Malformed relay quote response: no deposit step found"
        );
    }

    #[test]
    fn test_network_response_schema_source_display() {
        let err = NetworkError::ResponseSchemaSource {
            context: "HTTP response body",
            source: Box::new(std::io::Error::other("invalid utf-8 sequence")),
        };

        assert_eq!(
            err.to_string(),
            "Malformed HTTP response body: invalid utf-8 sequence"
        );
    }

    #[test]
    fn test_session_persistence_display() {
        let err = PaymentError::SessionPersistence {
            operation: "load session",
            reason: "database locked".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Session persistence error during load session: database locked"
        );
    }

    #[test]
    fn test_challenge_parse_display() {
        let err = PaymentError::ChallengeParse {
            context: "WWW-Authenticate header",
            reason: "missing realm".to_string(),
        };

        assert_eq!(
            err.to_string(),
            "Failed to parse WWW-Authenticate header: missing realm"
        );
    }

    #[test]
    fn test_challenge_format_display() {
        let err = PaymentError::ChallengeFormat {
            context: "Authorization header",
            reason: "invalid signature encoding".to_string(),
        };

        assert_eq!(
            err.to_string(),
            "Failed to format Authorization header: invalid signature encoding"
        );
    }

    #[test]
    fn test_challenge_schema_display() {
        let err = PaymentError::ChallengeSchema {
            context: "payment request",
            reason: "missing chainId".to_string(),
        };

        assert_eq!(
            err.to_string(),
            "Malformed payment request: missing chainId"
        );
    }

    #[test]
    fn test_challenge_schema_source_display() {
        let err = PaymentError::ChallengeSchemaSource {
            context: "payment request",
            source: Box::new(mpp::MppError::InvalidAmount("not a number".to_string())),
        };

        assert_eq!(
            err.to_string(),
            "Malformed payment request: Invalid amount: not a number"
        );
    }

    #[test]
    fn test_session_persistence_source_display() {
        let err = PaymentError::SessionPersistenceSource {
            operation: "save session",
            source: Box::new(std::io::Error::other("database locked")),
        };

        assert_eq!(
            err.to_string(),
            "Session persistence error during save session: database locked"
        );
    }

    #[test]
    fn test_session_persistence_context_source_display() {
        let source: TempoError = InputError::InvalidMethod("BAD".to_string()).into();
        let err = PaymentError::SessionPersistenceContextSource {
            operation: "session request reuse",
            context: "Session request failed; session state preserved for on-chain dispute",
            source: Box::new(source),
        };

        assert_eq!(
            err.to_string(),
            "Session persistence error during session request reuse: Session request failed; session state preserved for on-chain dispute: Invalid HTTP method: BAD"
        );
    }
}
