//! Wallet management: signers, keychain, and Tempo passkey wallets.

pub mod credentials;
pub(crate) mod key_authorization;
pub mod keychain;
mod setup;
pub mod signer;

pub use setup::WalletManager;
