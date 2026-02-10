//! Error display helpers with actionable suggestions.
//!
//! Provides user-friendly error messages that include suggestions
//! for how to fix common problems.

use crate::error::TempoCtlError;

/// Get a suggestion for how to fix an error, if available.
pub fn get_suggestion(err: &anyhow::Error) -> Option<String> {
    // Try to downcast to TempoCtlError
    if let Some(tempoctl_err) = err.downcast_ref::<TempoCtlError>() {
        return get_tempoctl_error_suggestion(tempoctl_err);
    }

    // Check error message for common patterns
    let msg = err.to_string().to_lowercase();

    if (msg.contains("no such file") || msg.contains("not found")) && msg.contains("config") {
        return Some("Run 'tempoctl login' to set up your wallet.".into());
    }

    if msg.contains("permission denied") {
        return Some("Check file permissions or run with appropriate privileges.".into());
    }

    if msg.contains("connection refused") || msg.contains("connect error") {
        return Some("Check your internet connection and try again.".into());
    }

    if msg.contains("timeout") {
        return Some("The request timed out. Try again or use --max-time.".into());
    }

    None
}

/// Get suggestion for a specific TempoCtlError variant.
fn get_tempoctl_error_suggestion(err: &TempoCtlError) -> Option<String> {
    match err {
        TempoCtlError::ConfigMissing(_) => {
            Some("Run 'tempoctl login' to set up your wallet.".into())
        }

        TempoCtlError::NoConfigDir => Some("Set the HOME environment variable.".into()),

        TempoCtlError::InvalidConfig(_) => {
            Some("Run 'tempoctl config' to view your current configuration.".into())
        }

        TempoCtlError::InvalidKey(_) => {
            Some("EVM private keys should be 64 hex characters (with optional 0x prefix).".into())
        }

        TempoCtlError::AmountExceedsMax { .. } => {
            Some("Increase with --max-amount or remove the limit.".into())
        }

        TempoCtlError::UnknownNetwork(_) => {
            Some("Run 'tempoctl networks list' to see available networks.".into())
        }

        TempoCtlError::Http(msg) => {
            if msg.starts_with("402") {
                Some("Ensure you have a wallet configured with 'tempoctl login'.".into())
            } else if msg.starts_with("401") || msg.starts_with("403") {
                Some("Check your credentials.".into())
            } else if msg.starts_with("404") {
                Some("Check the URL.".into())
            } else if msg.starts_with('5') {
                Some("Server error. Try again later.".into())
            } else {
                None
            }
        }

        TempoCtlError::Signing { .. } | TempoCtlError::SigningSimple(_) => {
            Some("Check your wallet configuration with 'tempoctl config'.".into())
        }

        TempoCtlError::BalanceQuery(_) | TempoCtlError::SpendingLimitQuery(_) => {
            Some("Check your network connection and RPC endpoint.".into())
        }

        TempoCtlError::SpendingLimitExceeded { .. } => {
            Some("Run 'tempoctl login' to generate a fresh authorization key.".into())
        }

        TempoCtlError::InsufficientBalance { .. } => Some("Deposit funds into your wallet.".into()),

        TempoCtlError::PaymentRejected { reason, .. } => {
            if reason.contains("insufficient") {
                Some("The price may have changed. Try the request again.".into())
            } else {
                Some("Try the request again.".into())
            }
        }

        _ => None,
    }
}

/// Format an error with its suggestion for display.
pub fn format_error_with_suggestion(err: &anyhow::Error) -> String {
    let mut output = format!("Error: {err:#}");

    if let Some(suggestion) = get_suggestion(err) {
        output.push_str("\n\nFix: ");
        output.push_str(&suggestion);
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_error_format(err: TempoCtlError, expected_prefix: &str, expected_fix: &str) {
        let anyhow_err: anyhow::Error = err.into();
        let output = format_error_with_suggestion(&anyhow_err);

        assert!(
            output.starts_with("Error: "),
            "Expected 'Error: ' prefix, got: {output}"
        );
        assert!(
            output.contains(expected_prefix),
            "Expected error to contain '{expected_prefix}', got: {output}"
        );
        assert!(
            output.contains(&format!("\n\nFix: {expected_fix}")),
            "Expected fix '{expected_fix}', got: {output}"
        );
    }

    #[test]
    fn test_spending_limit_exceeded_format() {
        assert_error_format(
            TempoCtlError::SpendingLimitExceeded {
                token: "pathUSD".into(),
                limit: "0.50".into(),
                required: "1.00".into(),
            },
            "Spending limit exceeded: limit is 0.50 pathUSD, need 1.00 pathUSD",
            "Run 'tempoctl login' to generate a fresh authorization key.",
        );
    }

    #[test]
    fn test_spending_limit_exceeded_with_address_token() {
        assert_error_format(
            TempoCtlError::SpendingLimitExceeded {
                token: "0x20c0000000000000000000000000000000000000".into(),
                limit: "0.50".into(),
                required: "1.00".into(),
            },
            "0x20c0000000000000000000000000000000000000",
            "Run 'tempoctl login' to generate a fresh authorization key.",
        );
    }

    #[test]
    fn test_insufficient_balance_format() {
        assert_error_format(
            TempoCtlError::InsufficientBalance {
                token: "pathUSD".into(),
                available: "0.50".into(),
                required: "1.00".into(),
            },
            "Insufficient pathUSD balance: have 0.50, need 1.00",
            "Deposit funds into your wallet.",
        );
    }

    #[test]
    fn test_payment_rejected_insufficient_format() {
        assert_error_format(
            TempoCtlError::PaymentRejected {
                reason: "insufficient_payment".into(),
                status_code: 403,
            },
            "Payment rejected by server: insufficient_payment",
            "The price may have changed. Try the request again.",
        );
    }

    #[test]
    fn test_payment_rejected_other_format() {
        assert_error_format(
            TempoCtlError::PaymentRejected {
                reason: "rate limited".into(),
                status_code: 429,
            },
            "Payment rejected by server: rate limited",
            "Try the request again.",
        );
    }

    #[test]
    fn test_amount_exceeds_max_format() {
        assert_error_format(
            TempoCtlError::AmountExceedsMax {
                required: 1000000,
                max: 500000,
            },
            "Required amount (1000000) exceeds maximum allowed (500000)",
            "Increase with --max-amount or remove the limit.",
        );
    }

    #[test]
    fn test_config_missing_format() {
        assert_error_format(
            TempoCtlError::ConfigMissing("wallet not configured".into()),
            "Configuration missing: wallet not configured",
            "Run 'tempoctl login' to set up your wallet.",
        );
    }

    #[test]
    fn test_no_config_dir_format() {
        assert_error_format(
            TempoCtlError::NoConfigDir,
            "Failed to determine config directory",
            "Set the HOME environment variable.",
        );
    }

    #[test]
    fn test_invalid_config_format() {
        assert_error_format(
            TempoCtlError::InvalidConfig("invalid rpc url".into()),
            "Invalid configuration: invalid rpc url",
            "Run 'tempoctl config' to view your current configuration.",
        );
    }

    #[test]
    fn test_invalid_key_format() {
        assert_error_format(
            TempoCtlError::InvalidKey("wrong format".into()),
            "Invalid private key: wrong format",
            "EVM private keys should be 64 hex characters (with optional 0x prefix).",
        );
    }

    #[test]
    fn test_signing_simple_format() {
        assert_error_format(
            TempoCtlError::SigningSimple("Failed to sign transaction".into()),
            "Signing error: Failed to sign transaction",
            "Check your wallet configuration with 'tempoctl config'.",
        );
    }

    #[test]
    fn test_unknown_network_format() {
        assert_error_format(
            TempoCtlError::UnknownNetwork("testnet".into()),
            "Unknown network: testnet",
            "Run 'tempoctl networks list' to see available networks.",
        );
    }

    #[test]
    fn test_balance_query_format() {
        assert_error_format(
            TempoCtlError::BalanceQuery("RPC timeout".into()),
            "Balance query failed: RPC timeout",
            "Check your network connection and RPC endpoint.",
        );
    }

    #[test]
    fn test_spending_limit_query_format() {
        assert_error_format(
            TempoCtlError::SpendingLimitQuery("RPC timeout".into()),
            "Spending limit query failed: RPC timeout",
            "Check your network connection and RPC endpoint.",
        );
    }

    #[test]
    fn test_http_402_format() {
        assert_error_format(
            TempoCtlError::Http("402 Payment Required".into()),
            "HTTP error: 402 Payment Required",
            "Ensure you have a wallet configured with 'tempoctl login'.",
        );
    }

    #[test]
    fn test_http_401_format() {
        assert_error_format(
            TempoCtlError::Http("401 Unauthorized".into()),
            "HTTP error: 401 Unauthorized",
            "Check your credentials.",
        );
    }

    #[test]
    fn test_http_404_format() {
        assert_error_format(
            TempoCtlError::Http("404 Not Found".into()),
            "HTTP error: 404 Not Found",
            "Check the URL.",
        );
    }

    #[test]
    fn test_http_500_format() {
        assert_error_format(
            TempoCtlError::Http("500 Internal Server Error".into()),
            "HTTP error: 500 Internal Server Error",
            "Server error. Try again later.",
        );
    }

    #[test]
    fn test_http_no_false_positive_on_digit_5() {
        let err = TempoCtlError::Http("Spending limit exceeded".into());
        let suggestion = get_tempoctl_error_suggestion(&err);
        assert!(
            suggestion.is_none(),
            "Http with non-status message should not match: {:?}",
            suggestion
        );
    }

    #[test]
    fn test_generic_config_not_found() {
        let err = anyhow::anyhow!("no such file or directory: config.toml");
        let suggestion = get_suggestion(&err);
        assert!(suggestion.is_some());
        assert!(suggestion.unwrap().contains("tempoctl login"));
    }

    #[test]
    fn test_generic_permission_denied() {
        let err = anyhow::anyhow!("permission denied");
        let suggestion = get_suggestion(&err);
        assert!(suggestion.is_some());
        assert!(suggestion.unwrap().contains("file permissions"));
    }

    #[test]
    fn test_generic_connection_refused() {
        let err = anyhow::anyhow!("connection refused");
        let suggestion = get_suggestion(&err);
        assert!(suggestion.is_some());
        assert!(suggestion.unwrap().contains("internet connection"));
    }

    #[test]
    fn test_generic_timeout() {
        let err = anyhow::anyhow!("timeout");
        let suggestion = get_suggestion(&err);
        assert!(suggestion.is_some());
        assert!(suggestion.unwrap().contains("--max-time"));
    }
}
