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

impl From<&crate::error::TempoError> for ExitCode {
    fn from(err: &crate::error::TempoError) -> Self {
        use crate::error::{KeyError, TempoError};

        match err {
            TempoError::Config(_) => ExitCode::InvalidUsage,
            TempoError::Key(k) => match k {
                KeyError::LoginExpired => ExitCode::GeneralError,
                _ => ExitCode::InvalidUsage,
            },
            TempoError::Input(_) => ExitCode::InvalidUsage,
            TempoError::Network(_) => ExitCode::NetworkError,
            TempoError::Payment(_) => ExitCode::PaymentFailed,
            TempoError::Io(_) | TempoError::Json(_) | TempoError::ToonEncode(_) => {
                ExitCode::GeneralError
            }
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

    #[test]
    fn from_tempo_error_network_is_network_exit() {
        use crate::error::{NetworkError, TempoError};

        let err: TempoError = NetworkError::HttpStatus {
            operation: "test request",
            status: 504,
            body: Some("timeout".to_string()),
        }
        .into();
        assert_eq!(ExitCode::from(&err), ExitCode::NetworkError);
    }

    #[test]
    fn from_tempo_error_key_variants() {
        use crate::error::{KeyError, TempoError};
        // LoginExpired → GeneralError
        let err: TempoError = KeyError::LoginExpired.into();
        assert_eq!(ExitCode::from(&err), ExitCode::GeneralError);

        // InvalidKey → InvalidUsage (user provided bad input)
        let err: TempoError = KeyError::InvalidKey("bad".to_string()).into();
        assert_eq!(ExitCode::from(&err), ExitCode::InvalidUsage);
    }

    #[test]
    fn from_tempo_error_session_persistence_context_source_is_payment_exit() {
        use crate::error::{NetworkError, PaymentError, TempoError};

        let source: TempoError = NetworkError::Http("upstream unavailable".to_string()).into();
        let err: TempoError = PaymentError::SessionPersistenceContextSource {
            operation: "session request reuse",
            context: "Session request failed; session state preserved for on-chain dispute",
            source: Box::new(source),
        }
        .into();

        assert_eq!(ExitCode::from(&err), ExitCode::PaymentFailed);
    }
}
