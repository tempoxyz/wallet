//! Test wallet constants and key presets.

use crate::config::write_test_files;
use tempfile::TempDir;

// ── Hardhat Account #0 (used for charge flow tests) ────────────────────

/// Hardhat account #0 private key (secp256k1).
pub const HARDHAT_PRIVATE_KEY: &str =
    "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

// ── Pre-built keys.toml content ─────────────────────────────────────────

/// Standard keys.toml for Moderato charge tests (Hardhat #0, Direct signing mode).
pub const MODERATO_DIRECT_KEYS_TOML: &str = r#"
[[keys]]
wallet_address = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
key_address = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
key = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
chain_id = 42431
"#;

/// Standard keys.toml for Keychain signing mode (wallet != key address).
pub const MODERATO_KEYCHAIN_KEYS_TOML: &str = r#"
[[keys]]
wallet_address = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
chain_id = 42431
key_address = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
key = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
provisioned = true
"#;

/// Set up a temp dir with config (pointing RPC to mock) but NO keys.toml.
pub fn setup_config_only(temp: &TempDir, rpc_base_url: &str) {
    let config_toml = format!("moderato_rpc = \"{rpc_base_url}\"\n");
    write_test_files(temp.path(), &config_toml, None);
}
