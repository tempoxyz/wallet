//! Error types for the presto library.

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
    #[error("Key is not provisioned on-chain. Retry the request to auto-provision, or run 'presto wallet create'.")]
    AccessKeyNotProvisioned,

    /// Browser-based login expired (device code expired or callback window timed out)
    #[error("Login expired. Use presto login to try again.")]
    LoginExpired,

    /// Key spending limit exceeded on-chain
    #[error("Spending limit exceeded: limit is {limit} {token}, need {required} {token}")]
    SpendingLimitExceeded {
        token: String,
        limit: String,
        required: String,
    },

    /// Insufficient token balance for payment
    #[error("Insufficient {token} balance: have {available}, need {required}. Fund with 'presto wallet fund'.")]
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

    /// Offline mode — no network access allowed
    #[error("Network access is disabled (--offline mode)")]
    OfflineMode,

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

/// Build the "no wallet configured" error message with the correct follow-up
/// based on the `PRESTO_WALLET_TYPE` env var (`local` → `presto wallet create`,
/// otherwise → `presto login`).
pub(crate) fn no_wallet_message() -> String {
    let is_local = std::env::var("PRESTO_WALLET_TYPE")
        .ok()
        .is_some_and(|v| v.eq_ignore_ascii_case("local"));

    if is_local {
        "No wallet configured. Create one with 'presto wallet create'.".to_string()
    } else {
        "No wallet configured. Log in with 'presto login'.".to_string()
    }
}

/// Map mpp validation errors to presto error types.
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

/// Try to extract `available`, `required`, and `token` from a raw RPC error
/// string that contains `InsufficientBalance { available: N, required: N, token: 0x... }`.
fn parse_insufficient_balance_fields(raw: &str) -> PrestoError {
    // Example: "...InsufficientBalance(InsufficientBalance { available: 0, required: 64467, token: 0x20c...})"
    let extract = |key: &str| -> Option<String> {
        let needle = format!("{key}: ");
        let start = raw.find(&needle)? + needle.len();
        let rest = &raw[start..];
        let end = rest.find([',', ' ', '}'])?;
        Some(rest[..end].to_string())
    };

    if let (Some(avail), Some(req), Some(tok)) =
        (extract("available"), extract("required"), extract("token"))
    {
        let (symbol, decimals) = resolve_token_symbol(&tok);
        let avail_fmt = format_atomic_amount(&avail, decimals);
        let req_fmt = format_atomic_amount(&req, decimals);

        PrestoError::InsufficientBalance {
            token: symbol,
            available: avail_fmt,
            required: req_fmt,
        }
    } else {
        // Can't parse — return a clean generic message
        PrestoError::InsufficientBalance {
            token: "USDC".to_string(),
            available: "0".to_string(),
            required: raw.to_string(),
        }
    }
}

/// Resolve a token address to a human-readable symbol and its decimals.
///
/// Returns the symbol name and decimal count. Falls back to `("tokens", 6)` for
/// unrecognized addresses and passes through non-address strings as-is.
fn resolve_token_symbol(token: &str) -> (String, u8) {
    use crate::network::tempo_tokens;

    if token.starts_with("0x") || token.starts_with("0X") {
        let symbol = match token {
            s if s.eq_ignore_ascii_case(tempo_tokens::USDCE) => "USDC",
            s if s.eq_ignore_ascii_case(tempo_tokens::PATH_USD) => "pathUSD",
            _ => "tokens",
        };
        (symbol.to_string(), 6)
    } else {
        // Already a symbol name (e.g. "USDC"), pass through
        (token.to_string(), 6)
    }
}

/// Format an amount string from atomic units to human-readable.
///
/// If the string is already a decimal (contains '.') or cannot be parsed as a
/// u128, it is returned unchanged.
fn format_atomic_amount(amount: &str, decimals: u8) -> String {
    if amount.contains('.') {
        return amount.to_string();
    }
    match amount.parse::<u128>() {
        Ok(v) => {
            let divisor = 10u128.pow(decimals as u32);
            let whole = v / divisor;
            let remainder = v % divisor;
            if remainder == 0 {
                format!("{whole}.{:0>width$}", 0, width = decimals as usize)
            } else {
                let frac = format!("{remainder:0>width$}", width = decimals as usize);
                format!("{whole}.{frac}")
            }
        }
        Err(_) => amount.to_string(),
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
            } => {
                // The mpp crate's classify_rpc_error may stuff the raw RPC
                // error into `required` while leaving `token`/`available`
                // empty.  Try to extract the real values from the raw string.
                if token.is_empty() || available.is_empty() {
                    parse_insufficient_balance_fields(&required)
                } else {
                    // Resolve raw token address to human-readable symbol and
                    // format atomic amounts when the mpp crate returns them.
                    let (symbol, decimals) = resolve_token_symbol(&token);
                    let avail_fmt = format_atomic_amount(&available, decimals);
                    let req_fmt = format_atomic_amount(&required, decimals);
                    PrestoError::InsufficientBalance {
                        token: symbol,
                        available: avail_fmt,
                        required: req_fmt,
                    }
                }
            }
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
    fn test_classify_insufficient_balance_raw_address() {
        // When mpp returns raw token address and atomic amounts
        let err = mpp::MppError::Tempo(mpp::client::TempoClientError::InsufficientBalance {
            token: "0x20c000000000000000000000b9537d11c60e8b50".to_string(),
            available: "0".to_string(),
            required: "1000".to_string(),
        });
        match classify_payment_error(err) {
            PrestoError::InsufficientBalance {
                token,
                available,
                required,
            } => {
                assert_eq!(token, "USDC");
                assert_eq!(available, "0.000000");
                assert_eq!(required, "0.001000");
            }
            other => panic!("Expected InsufficientBalance, got: {other}"),
        }
    }

    #[test]
    fn test_classify_insufficient_balance_pathusd_address() {
        let err = mpp::MppError::Tempo(mpp::client::TempoClientError::InsufficientBalance {
            token: "0x20c0000000000000000000000000000000000000".to_string(),
            available: "500000".to_string(),
            required: "1000000".to_string(),
        });
        match classify_payment_error(err) {
            PrestoError::InsufficientBalance {
                token,
                available,
                required,
            } => {
                assert_eq!(token, "pathUSD");
                assert_eq!(available, "0.500000");
                assert_eq!(required, "1.000000");
            }
            other => panic!("Expected InsufficientBalance, got: {other}"),
        }
    }

    #[test]
    fn test_resolve_token_symbol_usdc() {
        let (sym, dec) = resolve_token_symbol("0x20c000000000000000000000b9537d11c60e8b50");
        assert_eq!(sym, "USDC");
        assert_eq!(dec, 6);
    }

    #[test]
    fn test_resolve_token_symbol_passthrough() {
        let (sym, dec) = resolve_token_symbol("pathUSD");
        assert_eq!(sym, "pathUSD");
        assert_eq!(dec, 6);
    }

    #[test]
    fn test_format_atomic_amount_zero() {
        assert_eq!(format_atomic_amount("0", 6), "0.000000");
    }

    #[test]
    fn test_format_atomic_amount_small() {
        assert_eq!(format_atomic_amount("1000", 6), "0.001000");
    }

    #[test]
    fn test_format_atomic_amount_decimal_passthrough() {
        assert_eq!(format_atomic_amount("0.50", 6), "0.50");
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
