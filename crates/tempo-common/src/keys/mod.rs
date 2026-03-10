//! Wallet keys: key entries, selection logic, and persistence.

pub mod authorization;
mod io;
mod keystore;
mod model;
#[cfg(target_os = "macos")]
pub mod secure_enclave;
mod signer;

pub use keystore::Keystore;
use model::StoredTokenLimit;
pub use model::{KeyEntry, KeyType, WalletType};
pub use signer::{parse_private_key_signer, Signer};
