//! Exit codes for the presto CLI.
//!
//! Following standard Unix conventions and providing specific codes
//! for different error categories to aid scripting and automation.

/// Exit codes for the presto CLI.
///
/// These codes follow Unix conventions where possible:
/// - 0: Success
/// - 1: General error
/// - 2: Misuse of shell command (e.g., invalid arguments)
/// - 130: Script terminated by Ctrl+C (128 + SIGINT)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ExitCode {
    /// Successful execution
    Success = 0,

    /// General/unknown error
    GeneralError = 1,

    /// Invalid usage (bad arguments, invalid flags)
    InvalidUsage = 2,

    /// Configuration error (missing config, invalid config)
    ConfigError = 3,

    /// Network/connection error
    NetworkError = 4,

    /// Payment declined or failed
    PaymentFailed = 5,

    /// Insufficient funds for payment
    InsufficientFunds = 6,

    /// User cancelled operation (e.g., declined confirmation)
    UserCancelled = 7,

    /// Authentication/signing error
    AuthError = 8,

    /// Resource not found (network, wallet, etc.)
    NotFound = 9,

    /// Operation timed out
    Timeout = 10,

    /// Interrupted by signal (Ctrl+C)
    /// Standard Unix convention: 128 + signal number (SIGINT = 2)
    Interrupted = 130,
}

impl ExitCode {
    /// Convert to process exit code
    pub fn code(self) -> i32 {
        self as i32
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

        if msg.contains("timeout") {
            ExitCode::Timeout
        } else if msg.contains("connection") || msg.contains("network") {
            ExitCode::NetworkError
        } else if msg.contains("config") {
            ExitCode::ConfigError
        } else if msg.contains("not found") {
            ExitCode::NotFound
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
            | PrestoError::TomlSerialize(_) => ExitCode::ConfigError,

            PrestoError::LoginExpired => ExitCode::Timeout,

            // Payment/funds errors
            PrestoError::SpendingLimitExceeded { .. } | PrestoError::InsufficientBalance { .. } => {
                ExitCode::InsufficientFunds
            }

            // Invalid usage errors
            PrestoError::InvalidAmount(_) => ExitCode::InvalidUsage,

            // Payment protocol errors
            PrestoError::PaymentRejected { .. }
            | PrestoError::InvalidChallenge(_)
            | PrestoError::MissingHeader(_)
            | PrestoError::ChallengeExpired(_)
            | PrestoError::UnsupportedPaymentMethod(_)
            | PrestoError::UnsupportedPaymentIntent(_)
            | PrestoError::Mpp(_) => ExitCode::PaymentFailed,

            // Network/provider errors
            PrestoError::UnknownNetwork(_) | PrestoError::Http(_) | PrestoError::Reqwest(_) => {
                ExitCode::NetworkError
            }

            // Auth/signing errors
            PrestoError::InvalidKey(_)
            | PrestoError::Signing { .. }
            | PrestoError::SigningSimple(_)
            | PrestoError::InvalidAddress(_) => ExitCode::AuthError,

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
            ExitCode::ConfigError
        );
        assert_eq!(
            ExitCode::from(&PrestoError::UnknownNetwork("test".into())),
            ExitCode::NetworkError
        );
    }

    #[test]
    fn test_invalid_amount_exit_code() {
        use crate::error::PrestoError;
        assert_eq!(
            ExitCode::from(&PrestoError::InvalidAmount("abc".into())),
            ExitCode::InvalidUsage
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
