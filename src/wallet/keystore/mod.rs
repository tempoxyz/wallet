//! Keystore management for encrypted wallet storage
//!
//! This module provides functionality for creating, storing, and managing
//! encrypted keystores for EVM wallets.
//!
//! # Module Structure
//!
//! - `cache` - Password caching functionality
//! - `store` - Keystore type for loading and validating keystore files
//! - `encrypt` - Keystore creation and decryption
//!
//! # Example
//!
//! ```no_run
//! use pget::wallet::keystore::{create_keystore, decrypt_keystore, list_keystores, Keystore};
//!
//! // Create a new keystore
//! let private_key = "0x1234567890123456789012345678901234567890123456789012345678901234";
//! let keystore_path = create_keystore(private_key, "password", "my-wallet").unwrap();
//!
//! // List all keystores
//! let keystores = list_keystores().unwrap();
//!
//! // Load and inspect a keystore
//! let keystore = Keystore::load(&keystores[0]).unwrap();
//! println!("Address: {:?}", keystore.formatted_address());
//!
//! // Decrypt a keystore
//! let private_key_bytes = decrypt_keystore(&keystore_path, Some("password"), true).unwrap();
//! ```

mod cache;
mod encrypt;
mod store;

// Re-export public items
pub use encrypt::{create_keystore, decrypt_keystore, list_keystores};
pub use store::Keystore;
