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
            SignError::Io { operation, source } => {
                write!(f, "I/O error during {operation}: {source}")
            }
            SignError::IoWithPath {
                operation,
                path,
                source,
            } => {
                write!(f, "I/O error during {operation}: {path}: {source}")
            }
            SignError::Crypto { operation, source } => {
                write!(f, "Crypto error during {operation}: {source}")
            }
            SignError::CryptoWithPath {
                operation,
                path,
                source,
            } => {
                write!(f, "Crypto error during {operation}: {path}: {source}")
            }
            SignError::Serialization { operation, source } => {
                write!(f, "Serialization error during {operation}: {source}")
            }
        }
    }
}

impl std::error::Error for SignError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SignError::Io { source, .. } => Some(source),
            SignError::IoWithPath { source, .. } => Some(source),
            SignError::Crypto { source, .. } => Some(source),
            SignError::CryptoWithPath { source, .. } => Some(source),
            SignError::Serialization { source, .. } => Some(source),
        }
    }
}
