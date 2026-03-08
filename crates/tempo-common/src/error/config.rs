use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Configuration missing: {0}")]
    Missing(String),
    #[error("Invalid configuration: {0}")]
    Invalid(String),
    #[error("Failed to determine config directory")]
    NoConfigDir,
}
