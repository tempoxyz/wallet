//! Exit codes for the  tempo-walletCLI.
//!
//! Following standard Unix conventions and providing specific codes
//! for different error categories to aid scripting and automation.

/// Exit codes for the  tempo-walletCLI (simplified set).
///
/// - 0: Success
/// - 1: General error (fallback)
/// - 2: Invalid usage (bad arguments, invalid flags, invalid config)
/// - 3: Network error (connect, timeout, TLS, proxy)
/// - 4: HTTP error (HTTP >= 400 after successful transfer)
/// - 5: Payment error (payment rejected, unsupported method/intent)
/// - 130: Interrupted (Ctrl+C)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
#[allow(dead_code)]
pub enum ExitCode {
    /// Successful execution
    Success = 0,

    /// General/unknown error
    GeneralError = 1,

    /// Invalid usage (bad arguments, invalid flags)
    InvalidUsage = 2,

    /// Network/connection error
    NetworkError = 3,

    /// HTTP error (HTTP >= 400)
    HttpError = 4,

    /// Payment declined or failed
    PaymentFailed = 5,

    /// Interrupted by signal (Ctrl+C)
    /// Standard Unix convention: 128 + signal number (SIGINT = 2)
    Interrupted = 130,
}

impl ExitCode {
    /// Convert to process exit code
    pub fn code(self) -> i32 {
        self as i32
    }

    /// Machine-readable error code label for JSON error objects
    pub fn label(self) -> &'static str {
        match self {
            ExitCode::Success => "OK",
            ExitCode::GeneralError => "E_GENERAL",
            ExitCode::InvalidUsage => "E_USAGE",
            ExitCode::NetworkError => "E_NETWORK",
            ExitCode::HttpError => "E_HTTP",
            ExitCode::PaymentFailed => "E_PAYMENT",
            ExitCode::Interrupted => "E_INTERRUPTED",
        }
    }

    /// Exit the process with this code
    pub fn exit(self) -> ! {
        std::process::exit(self.code())
    }
}

impl From<ExitCode> for i32 {
    fn from(code: ExitCode) -> Self {
        code.code()
    }
}

impl From<&anyhow::Error> for ExitCode {
    fn from(err: &anyhow::Error) -> Self {
        // Try to downcast to PrestoError for specific handling
        if let Some(presto_err) = err
            .chain()
            .find_map(|e| e.downcast_ref::<crate::error::PrestoError>())
        {
            return ExitCode::from(presto_err);
        }

        // Check error message for common patterns
        let msg = err.to_string().to_lowercase();

        if msg.contains("timeout")
            || msg.contains("timed out")
            || msg.contains("connect")
            || msg.contains("connection")
            || msg.contains("network")
        {
            ExitCode::NetworkError
        } else if msg.contains("config") || msg.contains("invalid") || msg.contains("usage") {
            ExitCode::InvalidUsage
        } else {
            ExitCode::GeneralError
        }
    }
}

impl From<&crate::error::PrestoError> for ExitCode {
    fn from(err: &crate::error::PrestoError) -> Self {
        use crate::error::PrestoError;

        match err {
            // Configuration errors
            PrestoError::ConfigMissing(_)
            | PrestoError::InvalidConfig(_)
            | PrestoError::NoConfigDir
            | PrestoError::TomlParse(_)
            | PrestoError::TomlSerialize(_) => ExitCode::InvalidUsage,

            // Payment/funds errors
            PrestoError::SpendingLimitExceeded { .. }
            | PrestoError::InsufficientBalance { .. }
            | PrestoError::PaymentRejected { .. }
            | PrestoError::InvalidChallenge(_)
            | PrestoError::MissingHeader(_)
            | PrestoError::ChallengeExpired(_)
            | PrestoError::UnsupportedPaymentMethod(_)
            | PrestoError::UnsupportedPaymentIntent(_)
            | PrestoError::Mpp(_) => ExitCode::PaymentFailed,

            // Network/provider errors
            PrestoError::UnknownNetwork(_)
            | PrestoError::Http(_)
            | PrestoError::Reqwest(_)
            | PrestoError::OfflineMode => ExitCode::NetworkError,

            // Auth/signing errors -> usage (bad keys/addresses entered by user)
            PrestoError::InvalidKey(_)
            | PrestoError::Signing(_)
            | PrestoError::InvalidAddress(_) => ExitCode::InvalidUsage,

            // Invalid arguments / user input
            PrestoError::InvalidUrl(_) | PrestoError::InvalidHeader(_) => ExitCode::InvalidUsage,

            // General errors
            _ => ExitCode::GeneralError,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exit_code_values() {
        assert_eq!(ExitCode::Success.code(), 0);
        assert_eq!(ExitCode::GeneralError.code(), 1);
        assert_eq!(ExitCode::Interrupted.code(), 130);
    }

    #[test]
    fn test_exit_code_from_presto_error() {
        use crate::error::PrestoError;

        assert_eq!(
            ExitCode::from(&PrestoError::ConfigMissing("test".into())),
            ExitCode::InvalidUsage
        );
        assert_eq!(
            ExitCode::from(&PrestoError::UnknownNetwork("test".into())),
            ExitCode::NetworkError
        );
    }

    #[test]
    fn test_challenge_expired_exit_code() {
        use crate::error::PrestoError;
        assert_eq!(
            ExitCode::from(&PrestoError::ChallengeExpired("expired".into())),
            ExitCode::PaymentFailed
        );
    }
}
