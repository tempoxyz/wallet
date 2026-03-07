//! Payment error classification and extraction.
//!
//! Maps mpp library errors into tempo-wallet error types with actionable
//! context, and extracts error messages from JSON response bodies.

use alloy::primitives::utils::format_units;

use crate::error::TempoWalletError;
use crate::network::NetworkId;

/// Extract the first meaningful error string from a JSON response body.
///
/// Checks `error`, `message`, and `detail` fields in order.
pub(crate) fn extract_json_error(body: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    json.get("error")
        .or_else(|| json.get("message"))
        .or_else(|| json.get("detail"))
        .and_then(|v| v.as_str())
        .map(String::from)
}

/// Map mpp validation errors to tempo-wallet error types.
pub(super) fn map_mpp_validation_error(
    e: mpp::MppError,
    challenge: &mpp::PaymentChallenge,
) -> TempoWalletError {
    match e {
        mpp::MppError::UnsupportedPaymentMethod(msg) => {
            TempoWalletError::UnsupportedPaymentMethod(msg)
        }
        mpp::MppError::PaymentExpired(_) => {
            TempoWalletError::ChallengeExpired(challenge.expires.clone().unwrap_or_default())
        }
        mpp::MppError::InvalidChallenge { reason, .. } => {
            TempoWalletError::UnsupportedPaymentIntent(reason.unwrap_or_default())
        }
        other => TempoWalletError::InvalidChallenge(other.to_string()),
    }
}

/// Classify an mpp provider error into a TempoWalletError with actionable context.
pub(super) fn classify_payment_error(err: mpp::MppError, network: &NetworkId) -> TempoWalletError {
    use mpp::client::TempoClientError;

    match err {
        mpp::MppError::Tempo(tempo_err) => match tempo_err {
            TempoClientError::AccessKeyNotProvisioned => {
                TempoWalletError::AccessKeyNotProvisioned {
                    hint: "tempo-wallet login".to_string(),
                }
            }
            TempoClientError::SpendingLimitExceeded {
                token,
                limit,
                required,
            } => TempoWalletError::SpendingLimitExceeded {
                token,
                limit,
                required,
            },
            TempoClientError::InsufficientBalance {
                token,
                available,
                required,
            } => {
                let tc = network.token();
                let (symbol, decimals) = if tc.address.eq_ignore_ascii_case(&token) {
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
                TempoWalletError::InsufficientBalance {
                    token: symbol.to_string(),
                    available: avail_fmt,
                    required: req_fmt,
                }
            }
            TempoClientError::TransactionReverted(msg) => {
                TempoWalletError::TransactionReverted(msg)
            }
        },
        other => {
            let raw = other.to_string();
            let msg = raw.strip_prefix("HTTP error: ").unwrap_or(&raw).to_string();
            TempoWalletError::Http(msg)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_spending_limit() {
        let err = mpp::MppError::Tempo(mpp::client::TempoClientError::SpendingLimitExceeded {
            token: "pathUSD".to_string(),
            limit: "0.000000".to_string(),
            required: "0.010000".to_string(),
        });
        match classify_payment_error(err, &NetworkId::TempoModerato) {
            TempoWalletError::SpendingLimitExceeded {
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
    fn test_classify_insufficient_balance_non_address() {
        let err = mpp::MppError::Tempo(mpp::client::TempoClientError::InsufficientBalance {
            token: "pathUSD".to_string(),
            available: "0.50".to_string(),
            required: "1.00".to_string(),
        });
        match classify_payment_error(err, &NetworkId::TempoModerato) {
            TempoWalletError::InsufficientBalance {
                token,
                available,
                required,
            } => {
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
            TempoWalletError::AccessKeyNotProvisioned { .. }
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
            TempoWalletError::InsufficientBalance {
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
        match classify_payment_error(err, &NetworkId::TempoModerato) {
            TempoWalletError::InsufficientBalance {
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
    fn test_classify_unrecognized_falls_through() {
        let err = mpp::MppError::Http("something unexpected".to_string());
        match classify_payment_error(err, &NetworkId::Tempo) {
            TempoWalletError::Http(msg) => assert_eq!(msg, "something unexpected"),
            other => panic!("Expected Http passthrough, got: {other}"),
        }
    }
}
