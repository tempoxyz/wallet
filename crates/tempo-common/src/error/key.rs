use thiserror::Error;

#[derive(Error, Debug)]
pub enum KeyError {
    #[error("Invalid private key: {0}")]
    InvalidKey(String),
    #[error("Keychain error: {0}")]
    Keychain(String),
    #[error("Signing error: {0}")]
    Signing(String),
    #[error("Invalid address: {0}")]
    InvalidAddress(String),
    #[error("Login expired. Use tempo-wallet login to try again.")]
    LoginExpired,
}
