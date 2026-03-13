use std::fmt::{Display, Formatter};

#[derive(Debug)]
pub enum SignError {
    Io {
        operation: &'static str,
        source: std::io::Error,
    },
    IoWithPath {
        operation: &'static str,
        path: String,
        source: std::io::Error,
    },
    Crypto {
        operation: &'static str,
        source: minisign::PError,
    },
    CryptoWithPath {
        operation: &'static str,
        path: String,
        source: minisign::PError,
    },
    Serialization {
        operation: &'static str,
        source: serde_json::Error,
    },
}

impl Display for SignError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { operation, source } => {
                write!(f, "I/O error during {operation}: {source}")
            }
            Self::IoWithPath {
                operation,
                path,
                source,
            } => {
                write!(f, "I/O error during {operation}: {path}: {source}")
            }
            Self::Crypto { operation, source } => {
                write!(f, "Crypto error during {operation}: {source}")
            }
            Self::CryptoWithPath {
                operation,
                path,
                source,
            } => {
                write!(f, "Crypto error during {operation}: {path}: {source}")
            }
            Self::Serialization { operation, source } => {
                write!(f, "Serialization error during {operation}: {source}")
            }
        }
    }
}

impl std::error::Error for SignError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } | Self::IoWithPath { source, .. } => Some(source),
            Self::Crypto { source, .. } | Self::CryptoWithPath { source, .. } => Some(source),
            Self::Serialization { source, .. } => Some(source),
        }
    }
}
