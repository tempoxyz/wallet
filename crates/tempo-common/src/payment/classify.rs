//! Payment error classification and extraction.
//!
//! Maps mpp library errors into Tempo error types with actionable
//! context, and extracts error messages from JSON response bodies.

use alloy::primitives::utils::format_units;
use serde::Deserialize;
use std::collections::BTreeMap;

use crate::{
    error::{NetworkError, PaymentError},
    network::NetworkId,
};

pub const SESSION_PROBLEM_CHANNEL_NOT_FOUND: &str =
    "https://paymentauth.org/problems/session/channel-not-found";
pub const SESSION_PROBLEM_INSUFFICIENT_BALANCE: &str =
    "https://paymentauth.org/problems/session/insufficient-balance";
pub const SESSION_PROBLEM_CHALLENGE_NOT_FOUND: &str =
    "https://paymentauth.org/problems/session/challenge-not-found";
pub const SESSION_PROBLEM_DELTA_TOO_SMALL: &str =
    "https://paymentauth.org/problems/session/delta-too-small";
pub const SESSION_PROBLEM_AMOUNT_EXCEEDS_DEPOSIT: &str =
    "https://paymentauth.org/problems/session/amount-exceeds-deposit";
pub const SESSION_PROBLEM_CHANNEL_FINALIZED: &str =
    "https://paymentauth.org/problems/session/channel-finalized";
pub const SESSION_PROBLEM_INVALID_SIGNATURE: &str =
    "https://paymentauth.org/problems/session/invalid-signature";
pub const SESSION_PROBLEM_SIGNER_MISMATCH: &str =
    "https://paymentauth.org/problems/session/signer-mismatch";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionProblemType {
    ChannelNotFound,
    InsufficientBalance,
    ChallengeNotFound,
    DeltaTooSmall,
    AmountExceedsDeposit,
    ChannelFinalized,
    InvalidSignature,
    SignerMismatch,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ProblemDetails {
    #[serde(rename = "type")]
    pub problem_type: String,
    pub title: Option<String>,
    pub status: Option<u16>,
    pub detail: Option<String>,
    #[serde(rename = "requiredTopUp")]
    pub required_top_up: Option<String>,
    #[serde(rename = "channelId")]
    pub channel_id: Option<String>,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, serde_json::Value>,
}

impl ProblemDetails {
    #[must_use]
    pub fn classify(&self) -> SessionProblemType {
        match self.problem_type.as_str() {
            SESSION_PROBLEM_CHANNEL_NOT_FOUND => SessionProblemType::ChannelNotFound,
            SESSION_PROBLEM_INSUFFICIENT_BALANCE => SessionProblemType::InsufficientBalance,
            SESSION_PROBLEM_CHALLENGE_NOT_FOUND => SessionProblemType::ChallengeNotFound,
            SESSION_PROBLEM_DELTA_TOO_SMALL => SessionProblemType::DeltaTooSmall,
            SESSION_PROBLEM_AMOUNT_EXCEEDS_DEPOSIT => SessionProblemType::AmountExceedsDeposit,
            SESSION_PROBLEM_CHANNEL_FINALIZED => SessionProblemType::ChannelFinalized,
            SESSION_PROBLEM_INVALID_SIGNATURE => SessionProblemType::InvalidSignature,
            SESSION_PROBLEM_SIGNER_MISMATCH => SessionProblemType::SignerMismatch,
            _ => SessionProblemType::Unknown,
        }
    }

    #[must_use]
    pub fn message(&self) -> String {
        let detail = self
            .detail
            .as_deref()
            .or(self.title.as_deref())
            .unwrap_or("payment request rejected");
        format!("{}: {detail}", self.problem_type)
    }
}

/// Parse RFC 9457 Problem Details JSON.
#[must_use]
pub fn parse_problem_details(body: &str) -> Option<ProblemDetails> {
    let problem: ProblemDetails = serde_json::from_str(body).ok()?;
    if problem.problem_type.trim().is_empty() {
        return None;
    }
    Some(problem)
}

/// Extract the first meaningful error string from a JSON response body.
///
/// Checks `error`, `message`, and `detail` fields in order.
pub fn extract_json_error(body: &str) -> Option<String> {
    if let Some(problem) = parse_problem_details(body) {
        return Some(problem.message());
    }

    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    json.get("error")
        .or_else(|| json.get("message"))
        .or_else(|| json.get("detail"))
        .and_then(|v| v.as_str())
        .map(String::from)
}

/// Map mpp validation errors to tempo-wallet error types.
#[must_use]
pub fn map_mpp_validation_error(
    e: mpp::MppError,
    challenge: &mpp::PaymentChallenge,
) -> PaymentError {
    match e {
        mpp::MppError::UnsupportedPaymentMethod(msg) => PaymentError::UnsupportedPaymentMethod(msg),
        mpp::MppError::PaymentExpired(_) => {
            PaymentError::ChallengeExpired(challenge.expires.clone().unwrap_or_default())
        }
        mpp::MppError::InvalidChallenge { reason, .. } => {
            PaymentError::UnsupportedPaymentIntent(reason.unwrap_or_default())
        }
        other => PaymentError::ChallengeSchemaSource {
            context: "payment challenge",
            source: Box::new(other),
        },
    }
}

/// Classify an mpp provider error into a `TempoError` with actionable context.
#[must_use]
pub fn classify_payment_error(err: mpp::MppError, network: &NetworkId) -> crate::error::TempoError {
    use mpp::client::TempoClientError;

    match err {
        mpp::MppError::Tempo(tempo_err) => match tempo_err {
            TempoClientError::AccessKeyNotProvisioned => PaymentError::AccessKeyNotProvisioned {
                hint: "tempo wallet login".to_string(),
            }
            .into(),
            TempoClientError::SpendingLimitExceeded {
                token,
                limit,
                required,
            } => PaymentError::SpendingLimitExceeded {
                token,
                limit,
                required,
            }
            .into(),
            TempoClientError::InsufficientBalance {
                token,
                available,
                required,
            } => {
                let tc = network.token();
                let (symbol, decimals) =
                    if format!("{:#x}", tc.address).eq_ignore_ascii_case(&token) {
                        (tc.symbol, tc.decimals)
                    } else {
                        ("tokens", 6)
                    };
                let fmt = |s: &str| {
                    s.parse::<u128>()
                        .ok()
                        .and_then(|v| format_units(v, decimals).ok())
                        .unwrap_or_else(|| s.to_string())
                };
                let avail_fmt = fmt(&available);
                let req_fmt = fmt(&required);
                PaymentError::InsufficientBalance {
                    token: symbol.to_string(),
                    available: avail_fmt,
                    required: req_fmt,
                }
                .into()
            }
            TempoClientError::TransactionReverted(msg) => {
                if msg.contains("revoked") {
                    PaymentError::AccessKeyRevoked.into()
                } else {
                    PaymentError::TransactionReverted(msg).into()
                }
            }
        },
        other => {
            let raw = other.to_string();
            let msg = raw.strip_prefix("HTTP error: ").unwrap_or(&raw).to_string();
            classify_mpp_http_error(msg).into()
        }
    }
}

fn classify_mpp_http_error(message: String) -> NetworkError {
    let trimmed = message.trim();
    let mut parts = trimmed.splitn(2, ' ');

    if let Some(status_str) = parts.next() {
        if let Ok(status) = status_str.parse::<u16>() {
            if (400..=599).contains(&status) {
                let body = parts
                    .next()
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(String::from);
                return NetworkError::HttpStatus {
                    operation: "process payment",
                    status,
                    body,
                };
            }
        }
    }

    NetworkError::Http(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::TempoError;

    #[test]
    fn test_parse_problem_details_session_type() {
        let body = r#"{"type":"https://paymentauth.org/problems/session/channel-not-found","detail":"missing channel","status":410,"channelId":"0xabc"}"#;
        let problem = parse_problem_details(body).expect("problem details should parse");
        assert_eq!(problem.classify(), SessionProblemType::ChannelNotFound);
        assert_eq!(problem.detail.as_deref(), Some("missing channel"));
        assert_eq!(problem.channel_id.as_deref(), Some("0xabc"));
    }

    #[test]
    fn test_parse_problem_details_requires_type() {
        let body = r#"{"title":"oops"}"#;
        assert!(parse_problem_details(body).is_none());
    }

    #[test]
    fn test_parse_problem_details_preserves_extension_fields() {
        let body = r#"{"type":"https://paymentauth.org/problems/session/insufficient-balance","detail":"need top-up","requiredTopUp":"42","serverHint":"retry-after-head"}"#;
        let problem = parse_problem_details(body).expect("problem details should parse");
        assert_eq!(problem.required_top_up.as_deref(), Some("42"));
        assert_eq!(
            problem
                .extensions
                .get("serverHint")
                .and_then(|v| v.as_str()),
            Some("retry-after-head")
        );
    }

    #[test]
    fn test_classify_spending_limit() {
        let err = mpp::MppError::Tempo(mpp::client::TempoClientError::SpendingLimitExceeded {
            token: "pathUSD".to_string(),
            limit: "0.000000".to_string(),
            required: "0.010000".to_string(),
        });
        match classify_payment_error(err, &NetworkId::TempoModerato) {
            TempoError::Payment(PaymentError::SpendingLimitExceeded {
                token,
                limit,
                required,
            }) => {
                assert_eq!(token, "pathUSD");
                assert_eq!(limit, "0.000000");
                assert_eq!(required, "0.010000");
            }
            other => panic!("Expected SpendingLimitExceeded, got: {other}"),
        }
    }

    #[test]
    fn test_classify_insufficient_balance_non_address() {
        let err = mpp::MppError::Tempo(mpp::client::TempoClientError::InsufficientBalance {
            token: "pathUSD".to_string(),
            available: "0.50".to_string(),
            required: "1.00".to_string(),
        });
        match classify_payment_error(err, &NetworkId::TempoModerato) {
            TempoError::Payment(PaymentError::InsufficientBalance {
                token,
                available,
                required,
            }) => {
                // "pathUSD" is not an address, so falls back to "tokens"
                assert_eq!(token, "tokens");
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
            classify_payment_error(err, &NetworkId::Tempo),
            TempoError::Payment(PaymentError::AccessKeyNotProvisioned { .. })
        ));
    }

    #[test]
    fn test_classify_insufficient_balance_usdc_address() {
        let err = mpp::MppError::Tempo(mpp::client::TempoClientError::InsufficientBalance {
            token: "0x20c000000000000000000000b9537d11c60e8b50".to_string(),
            available: "0".to_string(),
            required: "1000".to_string(),
        });
        match classify_payment_error(err, &NetworkId::Tempo) {
            TempoError::Payment(PaymentError::InsufficientBalance {
                token,
                available,
                required,
            }) => {
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
        match classify_payment_error(err, &NetworkId::TempoModerato) {
            TempoError::Payment(PaymentError::InsufficientBalance {
                token,
                available,
                required,
            }) => {
                assert_eq!(token, "pathUSD");
                assert_eq!(available, "0.500000");
                assert_eq!(required, "1.000000");
            }
            other => panic!("Expected InsufficientBalance, got: {other}"),
        }
    }

    #[test]
    fn test_classify_unrecognized_falls_through() {
        let err = mpp::MppError::Http("something unexpected".to_string());
        match classify_payment_error(err, &NetworkId::Tempo) {
            TempoError::Network(NetworkError::Http(msg)) => {
                assert_eq!(msg, "something unexpected");
            }
            other => panic!("Expected Http passthrough, got: {other}"),
        }
    }

    #[test]
    fn test_classify_transaction_reverted() {
        let err = mpp::MppError::Tempo(mpp::client::TempoClientError::TransactionReverted(
            "execution reverted".to_string(),
        ));
        assert!(matches!(
            classify_payment_error(err, &NetworkId::Tempo),
            TempoError::Payment(PaymentError::TransactionReverted(msg)) if msg == "execution reverted"
        ));
    }

    #[test]
    fn test_classify_transaction_reverted_with_revoked_key() {
        let err = mpp::MppError::Tempo(mpp::client::TempoClientError::TransactionReverted(
            "Keychain signature validation failed: access key has been revoked".to_string(),
        ));
        assert!(matches!(
            classify_payment_error(err, &NetworkId::Tempo),
            TempoError::Payment(PaymentError::AccessKeyRevoked)
        ));
    }

    #[test]
    fn test_classify_non_tempo_mpp_error_strips_prefix() {
        // MppError::Http("x").to_string() → "HTTP error: x"
        // classify_payment_error strips the "HTTP error: " prefix
        let err = mpp::MppError::Http("503 Service Unavailable".to_string());
        match classify_payment_error(err, &NetworkId::Tempo) {
            TempoError::Network(NetworkError::HttpStatus {
                operation,
                status,
                body,
            }) => {
                assert_eq!(operation, "process payment");
                assert_eq!(status, 503);
                assert_eq!(body.as_deref(), Some("Service Unavailable"));
            }
            other => panic!("Expected HttpStatus with parsed code, got: {other}"),
        }
    }

    // --- extract_json_error tests ---

    #[test]
    fn test_extract_json_error_error_field() {
        let body = r#"{"error": "something went wrong"}"#;
        assert_eq!(
            extract_json_error(body),
            Some("something went wrong".to_string())
        );
    }

    #[test]
    fn test_extract_json_error_problem_details_preferred() {
        let body = r#"{"type":"https://paymentauth.org/problems/session/insufficient-balance","detail":"need top-up","message":"fallback"}"#;
        assert_eq!(
            extract_json_error(body),
            Some(
                "https://paymentauth.org/problems/session/insufficient-balance: need top-up"
                    .to_string()
            )
        );
    }

    #[test]
    fn test_extract_json_error_message_field() {
        let body = r#"{"message": "bad request"}"#;
        assert_eq!(extract_json_error(body), Some("bad request".to_string()));
    }

    #[test]
    fn test_extract_json_error_detail_field() {
        let body = r#"{"detail": "not found"}"#;
        assert_eq!(extract_json_error(body), Some("not found".to_string()));
    }

    #[test]
    fn test_extract_json_error_error_takes_precedence() {
        let body = r#"{"error": "the error", "message": "the message"}"#;
        assert_eq!(extract_json_error(body), Some("the error".to_string()));
    }

    #[test]
    fn test_extract_json_error_no_known_fields() {
        let body = r#"{"status": 500, "code": "INTERNAL"}"#;
        assert_eq!(extract_json_error(body), None);
    }

    #[test]
    fn test_extract_json_error_invalid_json() {
        assert_eq!(extract_json_error("not json at all"), None);
    }

    #[test]
    fn test_extract_json_error_empty_string() {
        assert_eq!(extract_json_error(""), None);
    }

    #[test]
    fn test_extract_json_error_non_string_field() {
        let body = r#"{"error": 42}"#;
        assert_eq!(extract_json_error(body), None);
    }

    // --- map_mpp_validation_error tests ---

    fn make_test_challenge() -> mpp::PaymentChallenge {
        let request = mpp::Base64UrlJson::from_value(
            &serde_json::json!({"amount": "1000", "currency": "USDC"}),
        )
        .unwrap();
        mpp::PaymentChallenge::new("test-id", "test-realm", "tempo", "charge", request)
    }

    #[test]
    fn test_map_unsupported_payment_method() {
        let challenge = make_test_challenge();
        let err = mpp::MppError::UnsupportedPaymentMethod("bitcoin".to_string());
        assert!(matches!(
            map_mpp_validation_error(err, &challenge),
            PaymentError::UnsupportedPaymentMethod(m) if m == "bitcoin"
        ));
    }

    #[test]
    fn test_map_payment_expired() {
        let mut challenge = make_test_challenge();
        challenge.expires = Some("2025-01-01T00:00:00Z".to_string());
        let err = mpp::MppError::PaymentExpired(None);
        assert!(matches!(
            map_mpp_validation_error(err, &challenge),
            PaymentError::ChallengeExpired(exp) if exp == "2025-01-01T00:00:00Z"
        ));
    }

    #[test]
    fn test_map_invalid_challenge() {
        let challenge = make_test_challenge();
        let err = mpp::MppError::InvalidChallenge {
            id: None,
            reason: Some("bad intent".to_string()),
        };
        assert!(matches!(
            map_mpp_validation_error(err, &challenge),
            PaymentError::UnsupportedPaymentIntent(r) if r == "bad intent"
        ));
    }

    #[test]
    fn test_map_other_error_to_invalid_challenge() {
        let challenge = make_test_challenge();
        let err = mpp::MppError::InvalidAmount("not a number".to_string());
        assert!(matches!(
            map_mpp_validation_error(err, &challenge),
            PaymentError::ChallengeSchemaSource { context: "payment challenge", source } if source.to_string().contains("not a number")
        ));
    }
}
