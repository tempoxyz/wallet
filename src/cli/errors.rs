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
        return Some("Run 'tempoctl login' to create a configuration file.".into());
    }

    if msg.contains("permission denied") {
        return Some("Check file permissions or run with appropriate privileges.".into());
    }

    if msg.contains("connection refused") || msg.contains("connect error") {
        return Some("Check your internet connection and try again.".into());
    }

    if msg.contains("timeout") {
        return Some(
            "The request timed out. Try again or increase the timeout with --max-time.".into(),
        );
    }

    None
}

/// Get suggestion for a specific TempoCtlError variant.
fn get_tempoctl_error_suggestion(err: &TempoCtlError) -> Option<String> {
    match err {
        TempoCtlError::ConfigMissing(_) => {
            Some("Run 'tempoctl login' to create a configuration file.".into())
        }

        TempoCtlError::NoConfigDir => {
            Some("Could not determine home directory. Set the HOME environment variable.".into())
        }

        TempoCtlError::InvalidConfig(_) => {
            Some("Run 'tempoctl config' to view your current configuration.".into())
        }

        TempoCtlError::InvalidKey(_) => {
            Some("EVM private keys should be 64 hex characters (with optional 0x prefix).".into())
        }

        TempoCtlError::AmountExceedsMax { required, max } => Some(format!(
            "The server requires {required} but your max is {max}.\n\
             Increase with --max-amount or remove the limit."
        )),

        TempoCtlError::UnknownNetwork(network) => Some(format!(
            "Network '{network}' is not recognized.\n\
             Run 'tempoctl networks list' to see available networks.\n\
             Or add a custom network in ~/.tempoctl/config.toml"
        )),

        TempoCtlError::Http(msg) => {
            if msg.contains("402") {
                Some("The server requires payment. Ensure you have configured a wallet.".into())
            } else if msg.contains("401") || msg.contains("403") {
                Some("Authentication failed. Check your credentials.".into())
            } else if msg.contains("404") {
                Some("The requested resource was not found. Check the URL.".into())
            } else if msg.contains("5") {
                Some("Server error. Try again later.".into())
            } else {
                None
            }
        }

        TempoCtlError::Signing { .. } | TempoCtlError::SigningSimple(_) => {
            Some("Failed to sign the transaction. Check your wallet configuration.".into())
        }

        TempoCtlError::BalanceQuery(_) => {
            Some("Could not query balance. Check your network connection and RPC endpoint.".into())
        }

        _ => None,
    }
}

/// Format an error with its suggestion for display.
pub fn format_error_with_suggestion(err: &anyhow::Error) -> String {
    let mut output = format!("Error: {err:#}");

    if let Some(suggestion) = get_suggestion(err) {
        output.push_str("\n\nSuggestion:\n");
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
