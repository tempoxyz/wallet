//! Wallet management: signers and Tempo passkey wallets.

pub mod access_key;
mod auth_server;
pub mod credentials;
mod manager;
pub mod signer;

pub use access_key::AccessKey;
pub use manager::WalletManager;

// Re-export commonly used types for convenience
#[allow(unused_imports)]
pub use credentials::{NetworkWallet, WalletCredentials};
