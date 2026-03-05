//! Wallet keys: key entries, selection logic, and persistence.

pub(crate) mod authorization;
mod io;
mod model;
mod signer;

pub(crate) use model::parse_private_key_signer;
pub(crate) use model::{KeyEntry, Keystore, WalletType};
// Re-exported for sibling modules (authorization.rs uses `super::{KeyType, StoredTokenLimit}`)
use model::{KeyType, StoredTokenLimit};
pub(crate) use signer::Signer;
