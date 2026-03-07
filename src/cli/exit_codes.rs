//! Exit codes for the tempo-wallet CLI.
//!
//! Following standard Unix conventions and providing specific codes
//! for different error categories to aid scripting and automation.

/// Exit codes for the tempo-wallet CLI (simplified set).
///
/// - 1: General error (fallback)
/// - 2: Invalid usage (bad arguments, invalid flags, invalid config)
/// - 3: Network error (connect, timeout, TLS, proxy)
/// - 4: Payment error (payment rejected, unsupported method/intent)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub(crate) enum ExitCode {
    /// General/unknown error
    GeneralError = 1,

    /// Invalid usage (bad arguments, invalid flags)
    InvalidUsage = 2,

    /// Network/connection error
    NetworkError = 3,

    /// Payment declined or failed
    PaymentFailed = 4,
}

impl ExitCode {
    /// Convert to process exit code
    pub(crate) fn code(self) -> i32 {
        self as i32
    }

    /// Machine-readable error code label for JSON error objects
    pub(crate) fn label(self) -> &'static str {
        match self {
            ExitCode::GeneralError => "E_GENERAL",
            ExitCode::InvalidUsage => "E_USAGE",
            ExitCode::NetworkError => "E_NETWORK",
            ExitCode::PaymentFailed => "E_PAYMENT",
        }
    }

    /// Exit the process with this code
    pub(crate) fn exit(self) -> ! {
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
        // Try to downcast to TempoWalletError for specific handling
        if let Some(app_err) = err
            .chain()
            .find_map(|e| e.downcast_ref::<crate::error::TempoWalletError>())
        {
            return ExitCode::from(app_err);
        }

        ExitCode::GeneralError
    }
}

impl From<&crate::error::TempoWalletError> for ExitCode {
    fn from(err: &crate::error::TempoWalletError) -> Self {
        use crate::error::TempoWalletError;

        match err {
            // Configuration errors
            TempoWalletError::ConfigMissing(_)
            | TempoWalletError::InvalidConfig(_)
            | TempoWalletError::NoConfigDir
            | TempoWalletError::TomlParse(_)
            | TempoWalletError::TomlSerialize(_) => ExitCode::InvalidUsage,

            // Payment/funds errors
            TempoWalletError::SpendingLimitExceeded { .. }
            | TempoWalletError::InsufficientBalance { .. }
            | TempoWalletError::PaymentRejected { .. }
            | TempoWalletError::TransactionReverted(_)
            | TempoWalletError::ChannelNotFound { .. }
            | TempoWalletError::AccessKeyNotProvisioned { .. }
            | TempoWalletError::InvalidChallenge(_)
            | TempoWalletError::MissingHeader(_)
            | TempoWalletError::ChallengeExpired(_)
            | TempoWalletError::UnsupportedPaymentMethod(_)
            | TempoWalletError::UnsupportedPaymentIntent(_)
            | TempoWalletError::Mpp(_) => ExitCode::PaymentFailed,

            // Network/provider errors
            TempoWalletError::UnknownNetwork(_)
            | TempoWalletError::Http(_)
            | TempoWalletError::Reqwest(_)
            | TempoWalletError::OfflineMode => ExitCode::NetworkError,

            // Auth/signing errors -> usage (bad keys/addresses entered by user)
            TempoWalletError::InvalidKey(_)
            | TempoWalletError::Signing(_)
            | TempoWalletError::InvalidAddress(_) => ExitCode::InvalidUsage,

            // Invalid arguments / user input
            TempoWalletError::InvalidUrl(_)
            | TempoWalletError::InvalidHeader(_)
            | TempoWalletError::BodyTooLarge(_)
            | TempoWalletError::HeaderTooLarge(_) => ExitCode::InvalidUsage,

            // File/stdin I/O during input processing
            TempoWalletError::ReadStdin(_) | TempoWalletError::ReadFile { .. } => {
                ExitCode::GeneralError
            }

            // Auth / login
            TempoWalletError::Keychain(_) | TempoWalletError::LoginExpired => {
                ExitCode::GeneralError
            }

            // Serialization / IO
            TempoWalletError::Json(_) | TempoWalletError::Io(_) => ExitCode::GeneralError,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exit_code_values() {
        assert_eq!(ExitCode::GeneralError.code(), 1);
        assert_eq!(ExitCode::InvalidUsage.code(), 2);
        assert_eq!(ExitCode::NetworkError.code(), 3);
        assert_eq!(ExitCode::PaymentFailed.code(), 4);
    }

    #[test]
    fn test_exit_code_from_app_error() {
        use crate::error::TempoWalletError;

        assert_eq!(
            ExitCode::from(&TempoWalletError::ConfigMissing("test".into())),
            ExitCode::InvalidUsage
        );
        assert_eq!(
            ExitCode::from(&TempoWalletError::UnknownNetwork("test".into())),
            ExitCode::NetworkError
        );
    }

    #[test]
    fn test_challenge_expired_exit_code() {
        use crate::error::TempoWalletError;
        assert_eq!(
            ExitCode::from(&TempoWalletError::ChallengeExpired("expired".into())),
            ExitCode::PaymentFailed
        );
    }
}
