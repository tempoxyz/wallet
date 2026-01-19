//! Error display helpers with actionable suggestions.
//!
//! Provides user-friendly error messages that include suggestions
//! for how to fix common problems.

use purl::PurlError;

/// Get a suggestion for how to fix an error, if available.
pub fn get_suggestion(err: &anyhow::Error) -> Option<String> {
    // Try to downcast to PurlError
    if let Some(purl_err) = err.downcast_ref::<PurlError>() {
        return get_purl_error_suggestion(purl_err);
    }

    // Check error message for common patterns
    let msg = err.to_string().to_lowercase();

    if msg.contains("no such file") || msg.contains("not found") {
        if msg.contains("config") {
            return Some("Run 'purl init' to create a configuration file.".into());
        }
        if msg.contains("keystore") {
            return Some(
                "Run 'purl method new <name> --generate' to create a new keystore.".into(),
            );
        }
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

/// Get suggestion for a specific PurlError variant.
fn get_purl_error_suggestion(err: &PurlError) -> Option<String> {
    match err {
        PurlError::NoPaymentMethods => Some(
            "To configure payment methods:\n  \
             • Run 'purl init' for interactive setup\n  \
             • Or run 'purl method new <name> --generate' to create a wallet"
                .into(),
        ),

        PurlError::ConfigMissing(_) => {
            Some("Run 'purl init' to create a configuration file.".into())
        }

        PurlError::NoConfigDir => {
            Some("Could not determine home directory. Set the HOME environment variable.".into())
        }

        PurlError::InvalidConfig(msg) => {
            if msg.contains("keystore") {
                Some("Check that your keystore file exists and is valid JSON.".into())
            } else if msg.contains("private_key") {
                Some("Private key should be 64 hex characters (with optional 0x prefix).".into())
            } else {
                Some("Run 'purl config' to view your current configuration.".into())
            }
        }

        PurlError::InvalidKey(_) => Some(
            "EVM private keys should be 64 hex characters (with optional 0x prefix).\n\
             Solana keys should be base58-encoded keypairs."
                .into(),
        ),

        PurlError::NoCompatibleMethod { networks } => {
            let networks_str = networks.join(", ");
            Some(format!(
                "Server accepts: {networks_str}\n\
                 Configure a wallet for one of these networks with 'purl init'."
            ))
        }

        PurlError::AmountExceedsMax { required, max } => Some(format!(
            "The server requires {required} but your max is {max}.\n\
             Increase with --max-amount or remove the limit."
        )),

        PurlError::UnknownNetwork(network) => Some(format!(
            "Network '{network}' is not recognized.\n\
             Run 'purl networks list' to see available networks.\n\
             Or add a custom network in ~/.purl/config.toml"
        )),

        PurlError::TokenConfigNotFound { asset, network } => Some(format!(
            "Token {asset} not configured for {network}.\n\
             Add it to ~/.purl/config.toml under [[tokens]]."
        )),

        PurlError::ProviderNotFound(network) => Some(format!(
            "No payment provider for network '{network}'.\n\
             Run 'purl networks list' to see supported networks."
        )),

        PurlError::Http(msg) => {
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

        PurlError::Signing(_) => Some(
            "Failed to sign the transaction. Check your wallet configuration:\n  \
             • Verify your keystore password is correct\n  \
             • Ensure your private key is valid"
                .into(),
        ),

        PurlError::BalanceQuery(_) => {
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
    fn test_no_payment_methods_suggestion() {
        let err = PurlError::NoPaymentMethods;
        let suggestion = get_purl_error_suggestion(&err);
        assert!(suggestion.is_some());
        assert!(suggestion.unwrap().contains("purl init"));
    }

    #[test]
    fn test_config_missing_suggestion() {
        let err = PurlError::ConfigMissing("test".into());
        let suggestion = get_purl_error_suggestion(&err);
        assert!(suggestion.is_some());
        assert!(suggestion.unwrap().contains("purl init"));
    }

    #[test]
    fn test_unknown_network_suggestion() {
        let err = PurlError::UnknownNetwork("testnet".into());
        let suggestion = get_purl_error_suggestion(&err);
        assert!(suggestion.is_some());
        assert!(suggestion.unwrap().contains("purl networks list"));
    }
}
