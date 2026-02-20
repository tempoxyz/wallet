//! Error display helpers with actionable suggestions.
//!
//! Provides user-friendly error messages that include suggestions
//! for how to fix common problems.

use crate::error::PrestoError;

/// Walk the anyhow error chain to find a PrestoError.
fn find_presto_error(err: &anyhow::Error) -> Option<&PrestoError> {
    err.chain().find_map(|e| e.downcast_ref::<PrestoError>())
}

/// Get a suggestion for how to fix an error, if available.
pub fn get_suggestion(err: &anyhow::Error) -> Option<String> {
    // Try to find a PrestoError in the error chain
    if let Some(presto_err) = find_presto_error(err) {
        return get_presto_error_suggestion(presto_err);
    }

    // Check error message for common patterns
    let msg = err.to_string().to_lowercase();

    if (msg.contains("no such file") || msg.contains("not found")) && msg.contains("config") {
        return Some("Try running ' tempo-walletlogin' to set up your wallet.".into());
    }

    if msg.contains("permission denied") {
        return Some("Check file permissions or run with appropriate privileges.".into());
    }

    if msg.contains("connection refused") || msg.contains("connect error") {
        return Some("Check your internet connection and try again.".into());
    }

    if msg.contains("timeout") {
        return Some("The request timed out. Try again or use --timeout.".into());
    }

    None
}

/// Get suggestion for a specific PrestoError variant.
fn get_presto_error_suggestion(err: &PrestoError) -> Option<String> {
    match err {
        PrestoError::ConfigMissing(_) => Some("Run ' tempo-walletlogin' to set up your wallet.".into()),

        PrestoError::NoConfigDir => Some("Set the HOME environment variable.".into()),

        PrestoError::InvalidConfig(_) => Some("Check your configuration file.".into()),

        PrestoError::InvalidKey(_) => {
            Some("EVM private keys should be 64 hex characters (with optional 0x prefix).".into())
        }

        PrestoError::UnknownNetwork(_) => Some("Supported networks: tempo, tempo-moderato.".into()),

        PrestoError::Http(msg) => {
            if msg.starts_with("402") {
                Some("Run ' tempo-walletlogin' to set up your wallet.".into())
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

        PrestoError::Signing { .. } | PrestoError::SigningSimple(_) => {
            Some("Check your wallet configuration.".into())
        }

        PrestoError::BalanceQuery(_) => {
            Some("Check your network connection and RPC endpoint.".into())
        }

        PrestoError::AccessKeyNotProvisioned => {
            Some("Run ' tempo-walletlogin' to provision your access key.".into())
        }

        PrestoError::SpendingLimitQuery(msg) => {
            if msg.contains("revoked") || msg.contains("expired") {
                Some("Run ' tempo-walletlogin' to generate a fresh access key.".into())
            } else {
                Some("Check your network connection and RPC endpoint.".into())
            }
        }

        PrestoError::SpendingLimitExceeded { .. } => {
            Some("Run ' tempo-walletlogin' to generate a fresh authorization key.".into())
        }

        PrestoError::InsufficientBalance { .. } => Some("Deposit funds into your wallet.".into()),

        PrestoError::PaymentRejected { reason, .. } => {
            if reason.contains("access key does not exist")
                || reason.contains("access key is not provisioned")
            {
                Some("Run ' tempo-walletlogin' to provision your access key.".into())
            } else if reason.contains("insufficient") {
                Some("The price may have changed. Try the request again.".into())
            } else {
                Some("Try the request again.".into())
            }
        }

        PrestoError::InvalidAmount(_) => {
            Some("Use a numeric amount (e.g., 1.0 or 1000000).".into())
        }
        PrestoError::MissingRequirement(_) => {
            Some("The server's payment challenge is incomplete. Retry the request.".into())
        }
        PrestoError::UnsupportedToken(_) => {
            Some("This token is not supported. Check the server's accepted currencies.".into())
        }
        PrestoError::InvalidAddress(_) => {
            Some("Provide a valid EVM address (0x + 40 hex chars).".into())
        }
        PrestoError::Json(_) => {
            Some("Check your JSON syntax. If using --json, verify shell quoting.".into())
        }
        PrestoError::TomlParse(_) | PrestoError::TomlSerialize(_) => {
            Some("Fix your config file, or run ' tempo-walletlogin' to regenerate it.".into())
        }
        PrestoError::HexDecode(_)
        | PrestoError::Base64Decode(_)
        | PrestoError::InvalidBase64Url(_) => Some(
            "Ensure the value is correctly encoded (no extra whitespace or truncation).".into(),
        ),
        PrestoError::UnsupportedPaymentMethod(_) => Some(
            "This payment method is not supported. Upgrade  tempo-walletor try a different server."
                .into(),
        ),
        PrestoError::UnsupportedPaymentIntent(_) => Some(
            "This payment intent is not supported. Upgrade  tempo-walletor try a different server."
                .into(),
        ),
        PrestoError::InvalidChallenge(_) => {
            Some("The server's payment challenge is malformed. Retry the request.".into())
        }
        PrestoError::MissingHeader(_) => {
            Some("The server response is missing a required header. Use -v for details.".into())
        }
        PrestoError::ChallengeExpired(_) => {
            Some("Retry immediately. If it keeps expiring, check your system clock.".into())
        }
        PrestoError::InvalidDid(_) => {
            Some("Run ' tempo-walletlogin' to recreate identity credentials.".into())
        }
        PrestoError::Io(_) => Some("Check file paths and permissions.".into()),
        PrestoError::Reqwest(_) => Some("Check your internet connection and retry.".into()),
        PrestoError::InvalidUtf8(_) => {
            Some("The response contains non-UTF8 data. Try saving to a file with -o.".into())
        }
        PrestoError::SystemTime(_) => Some("Check that your system clock is set correctly.".into()),
        PrestoError::Mpp(_) => Some("Payment protocol error. Retry the request.".into()),
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

    fn assert_error_format(err: PrestoError, expected_prefix: &str, expected_fix: &str) {
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
            PrestoError::SpendingLimitExceeded {
                token: "pathUSD".into(),
                limit: "0.50".into(),
                required: "1.00".into(),
            },
            "Spending limit exceeded: limit is 0.50 pathUSD, need 1.00 pathUSD",
            "Run ' tempo-walletlogin' to generate a fresh authorization key.",
        );
    }

    #[test]
    fn test_spending_limit_exceeded_with_address_token() {
        assert_error_format(
            PrestoError::SpendingLimitExceeded {
                token: "0x20c0000000000000000000000000000000000000".into(),
                limit: "0.50".into(),
                required: "1.00".into(),
            },
            "0x20c0000000000000000000000000000000000000",
            "Run ' tempo-walletlogin' to generate a fresh authorization key.",
        );
    }

    #[test]
    fn test_insufficient_balance_format() {
        assert_error_format(
            PrestoError::InsufficientBalance {
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
            PrestoError::PaymentRejected {
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
            PrestoError::PaymentRejected {
                reason: "rate limited".into(),
                status_code: 429,
            },
            "Payment rejected by server: rate limited",
            "Try the request again.",
        );
    }

    #[test]
    fn test_config_missing_format() {
        assert_error_format(
            PrestoError::ConfigMissing("wallet not configured".into()),
            "Configuration missing: wallet not configured",
            "Run ' tempo-walletlogin' to set up your wallet.",
        );
    }

    #[test]
    fn test_no_config_dir_format() {
        assert_error_format(
            PrestoError::NoConfigDir,
            "Failed to determine config directory",
            "Set the HOME environment variable.",
        );
    }

    #[test]
    fn test_invalid_config_format() {
        assert_error_format(
            PrestoError::InvalidConfig("invalid rpc url".into()),
            "Invalid configuration: invalid rpc url",
            "Check your configuration file.",
        );
    }

    #[test]
    fn test_invalid_key_format() {
        assert_error_format(
            PrestoError::InvalidKey("wrong format".into()),
            "Invalid private key: wrong format",
            "EVM private keys should be 64 hex characters (with optional 0x prefix).",
        );
    }

    #[test]
    fn test_signing_simple_format() {
        assert_error_format(
            PrestoError::SigningSimple("Failed to sign transaction".into()),
            "Signing error: Failed to sign transaction",
            "Check your wallet configuration.",
        );
    }

    #[test]
    fn test_unknown_network_format() {
        assert_error_format(
            PrestoError::UnknownNetwork("testnet".into()),
            "Unknown network: testnet",
            "Supported networks: tempo, tempo-moderato.",
        );
    }

    #[test]
    fn test_balance_query_format() {
        assert_error_format(
            PrestoError::BalanceQuery("RPC timeout".into()),
            "Balance query failed: RPC timeout",
            "Check your network connection and RPC endpoint.",
        );
    }

    #[test]
    fn test_spending_limit_query_format() {
        assert_error_format(
            PrestoError::SpendingLimitQuery("RPC timeout".into()),
            "Spending limit query failed: RPC timeout",
            "Check your network connection and RPC endpoint.",
        );
    }

    #[test]
    fn test_http_402_format() {
        assert_error_format(
            PrestoError::Http("402 Payment Required".into()),
            "HTTP error: 402 Payment Required",
            "Run ' tempo-walletlogin' to set up your wallet.",
        );
    }

    #[test]
    fn test_http_401_format() {
        assert_error_format(
            PrestoError::Http("401 Unauthorized".into()),
            "HTTP error: 401 Unauthorized",
            "Check your credentials.",
        );
    }

    #[test]
    fn test_http_404_format() {
        assert_error_format(
            PrestoError::Http("404 Not Found".into()),
            "HTTP error: 404 Not Found",
            "Check the URL.",
        );
    }

    #[test]
    fn test_http_500_format() {
        assert_error_format(
            PrestoError::Http("500 Internal Server Error".into()),
            "HTTP error: 500 Internal Server Error",
            "Server error. Try again later.",
        );
    }

    #[test]
    fn test_http_no_false_positive_on_digit_5() {
        let err = PrestoError::Http("Spending limit exceeded".into());
        let suggestion = get_presto_error_suggestion(&err);
        assert!(
            suggestion.is_none(),
            "Http with non-status message should not match: {:?}",
            suggestion
        );
    }

    #[test]
    fn test_unsupported_payment_method_format() {
        assert_error_format(
            PrestoError::UnsupportedPaymentMethod("bitcoin".into()),
            "Unsupported payment method: bitcoin",
            "This payment method is not supported. Upgrade  tempo-walletor try a different server.",
        );
    }

    #[test]
    fn test_challenge_expired_format() {
        assert_error_format(
            PrestoError::ChallengeExpired("5 minutes ago".into()),
            "Challenge expired: 5 minutes ago",
            "Retry immediately. If it keeps expiring, check your system clock.",
        );
    }

    #[test]
    fn test_invalid_amount_format() {
        assert_error_format(
            PrestoError::InvalidAmount("abc".into()),
            "Invalid amount: abc",
            "Use a numeric amount (e.g., 1.0 or 1000000).",
        );
    }

    #[test]
    fn test_io_error_has_suggestion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err = PrestoError::Io(io_err);
        let suggestion = get_presto_error_suggestion(&err);
        assert!(suggestion.is_some());
        assert!(suggestion.unwrap().contains("file paths"));
    }

    #[test]
    fn test_generic_config_not_found() {
        let err = anyhow::anyhow!("no such file or directory: config.toml");
        let suggestion = get_suggestion(&err);
        assert!(suggestion.is_some());
        assert!(suggestion.unwrap().contains(" tempo-walletlogin"));
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
        assert!(suggestion.unwrap().contains("--timeout"));
    }
}
