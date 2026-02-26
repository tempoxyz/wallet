//! Wallet management: signers, keychain, and Tempo passkey wallets.

pub mod credentials;
pub(crate) mod key_authorization;
pub mod keychain;
mod passkey_login;
pub mod signer;

pub use passkey_login::WalletManager;
