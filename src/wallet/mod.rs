//! Wallet management: signers, keychain, and Tempo passkey wallets.

pub(crate) mod credentials;
pub(crate) mod key_authorization;
pub(crate) mod keychain;
mod passkey;
pub(crate) mod signer;

pub(crate) use passkey::WalletManager;
