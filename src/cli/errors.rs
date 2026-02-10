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
            if msg.contains("402") {
                Some("Ensure you have a wallet configured with 'tempoctl login'.".into())
            } else if msg.contains("401") || msg.contains("403") {
                Some("Check your credentials.".into())
            } else if msg.contains("404") {
                Some("Check the URL.".into())
            } else if msg.contains("5") {
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

    #[test]
    fn test_config_missing_suggestion() {
        let err = TempoCtlError::ConfigMissing("test".into());
        let suggestion = get_tempoctl_error_suggestion(&err);
        assert!(suggestion.is_some());
        assert!(suggestion.unwrap().contains("tempoctl login"));
    }

    #[test]
    fn test_unknown_network_suggestion() {
        let err = TempoCtlError::UnknownNetwork("testnet".into());
        let suggestion = get_tempoctl_error_suggestion(&err);
        assert!(suggestion.is_some());
        assert!(suggestion.unwrap().contains("tempoctl networks list"));
    }
}
