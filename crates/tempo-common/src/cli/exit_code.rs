//! Shared exit code mapping for Tempo extension CLIs.

/// Exit codes for Tempo extension CLIs.
///
/// - 1: General error (fallback)
/// - 2: Invalid usage (bad arguments, invalid flags, invalid config)
/// - 3: Network error (connect, timeout, TLS, proxy)
/// - 4: Payment error (payment rejected, unsupported method/intent)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ExitCode {
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
    /// Convert to process exit code.
    pub fn code(self) -> i32 {
        self as i32
    }

    /// Machine-readable error code label for structured error output.
    pub fn label(self) -> &'static str {
        match self {
            ExitCode::GeneralError => "E_GENERAL",
            ExitCode::InvalidUsage => "E_USAGE",
            ExitCode::NetworkError => "E_NETWORK",
            ExitCode::PaymentFailed => "E_PAYMENT",
        }
    }

    /// Exit the process with this code.
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
        if let Some(app_err) = err
            .chain()
            .find_map(|e| e.downcast_ref::<crate::error::TempoError>())
        {
            return ExitCode::from(app_err);
        }

        ExitCode::GeneralError
    }
}

impl From<&crate::error::TempoError> for ExitCode {
    fn from(err: &crate::error::TempoError) -> Self {
        use crate::error::{KeyError, TempoError};

        match err {
            TempoError::Config(_) => ExitCode::InvalidUsage,
            TempoError::Key(k) => match k {
                KeyError::Keychain(_) | KeyError::LoginExpired => ExitCode::GeneralError,
                _ => ExitCode::InvalidUsage,
            },
            TempoError::Input(_) => ExitCode::InvalidUsage,
            TempoError::Network(_) => ExitCode::NetworkError,
            TempoError::Payment(_) => ExitCode::PaymentFailed,
            TempoError::Io(_) | TempoError::Json(_) => ExitCode::GeneralError,
            TempoError::TomlParse(_) | TempoError::TomlSerialize(_) => ExitCode::InvalidUsage,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_values_are_stable() {
        assert_eq!(ExitCode::GeneralError.code(), 1);
        assert_eq!(ExitCode::InvalidUsage.code(), 2);
        assert_eq!(ExitCode::NetworkError.code(), 3);
        assert_eq!(ExitCode::PaymentFailed.code(), 4);
    }
}
