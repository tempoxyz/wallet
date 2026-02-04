//! Wallet management commands for pget CLI

use crate::wallet::keystore::{create_keystore, list_keystores, Keystore};
use alloy::primitives::Address;
use alloy::signers::local::PrivateKeySigner;
use anyhow::Result;
use dialoguer::{Input, Password};
use std::path::PathBuf;

/// Derive an EVM address from raw private key bytes.
fn derive_evm_address(private_key_bytes: &[u8]) -> Result<Address, anyhow::Error> {
    let hex_key = hex::encode(private_key_bytes);
    let signer: PrivateKeySigner = hex_key
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid private key: {}", e))?;
    Ok(signer.address())
}

/// List all available keystores in the keystores directory
///
/// Scans the default keystores directory (`~/.pget/keystores/`) and displays
/// all found keystore files along with their addresses (if available).
///
/// # Examples
///
/// ```text
/// $ pget method list
/// Available keystores:
///   evm.json (0xabcd1234...)
///   test-wallet.json (0x5678efgh...)
/// ```
///
/// # Errors
///
/// Returns an error if the keystores directory cannot be accessed.
pub fn list_command() -> Result<()> {
    let keystores = list_keystores()?;

    if keystores.is_empty() {
        println!("No keystores found.");
        println!(
            "Use 'pget wallet connect' for Tempo wallet, or 'pget method new' for local keystores."
        );
        return Ok(());
    }

    println!("Available keystores:");
    println!();

    for keystore_path in keystores {
        let filename = keystore_path
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("unknown");

        // Try to read the keystore to get the address
        if let Ok(content) = std::fs::read_to_string(&keystore_path) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(address) = json["address"].as_str() {
                    println!("  {filename} (0x{address})");
                } else {
                    println!("  {filename} (no address field)");
                }
            } else {
                println!("  {filename} (invalid format)");
            }
        } else {
            println!("  {filename} (unreadable)");
        }
    }

    Ok(())
}

/// Create a new encrypted keystore
///
/// Creates an encrypted keystore file using either a generated private key or
/// an existing one provided by the user. The keystore is encrypted with a
/// user-provided password using the standard Ethereum keystore format.
///
/// # Arguments
///
/// * `name` - Name for the keystore file (e.g., "my-wallet" creates "my-wallet.json")
/// * `generate` - If true, generates a new private key; if false, prompts for an existing key
///
/// # Behavior
///
/// When `generate` is true:
/// - Generates a new random private key
/// - Displays the private key (user should save this securely)
/// - Prompts for a password to encrypt the keystore
///
/// When `generate` is false:
/// - Prompts the user to enter an existing private key
/// - Prompts for a password to encrypt the keystore
///
/// # Examples
///
/// ```text
/// $ pget method new --name my-wallet --generate
/// Creating new keystore: my-wallet
/// Generated new private key: 0x1234...
/// Save this private key securely! You'll need it to recover your wallet.
/// Enter password to encrypt the keystore: ****
/// Keystore created at: /home/user/.pget/keystores/my-wallet.json
///   Address: 0xabcd...
/// ```
///
/// # Errors
///
/// Returns an error if:
/// - The keystore directory cannot be created or accessed
/// - The private key is invalid
/// - Password confirmation fails
/// - File write operation fails
pub fn new_command(name: &str, generate: bool) -> Result<()> {
    println!("Creating new keystore: {name}");

    let private_key = if generate {
        // Generate a new private key
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let key_bytes: [u8; 32] = rng.gen();
        let key_hex = hex::encode(key_bytes);
        println!("Generated new private key: 0x{key_hex}");
        println!("Save this private key securely! You'll need it to recover your wallet.");
        key_hex
    } else {
        // Prompt for private key
        Input::new()
            .with_prompt("Enter private key (hex, with or without 0x prefix)")
            .interact_text()?
    };

    // Get password for encryption
    let password = Password::new()
        .with_prompt("Enter password to encrypt the keystore")
        .with_confirmation("Confirm password", "Passwords do not match")
        .interact()?;

    // Create the keystore
    let keystore_path = create_keystore(&private_key, &password, name)?;

    println!("Keystore created at: {}", keystore_path.display());

    // Show the address
    if let Ok(content) = std::fs::read_to_string(&keystore_path) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(address) = json["address"].as_str() {
                println!("  Address: 0x{address}");
            }
        }
    }

    println!();
    println!("To use this keystore, update your config file:");
    println!("[evm]");
    println!("keystore = \"{}\"", keystore_path.display());

    Ok(())
}

/// Import an existing private key into a new encrypted keystore
///
/// Creates an encrypted keystore from an existing private key. This is useful
/// for importing keys from other wallets or recovering from a backed-up private key.
///
/// # Arguments
///
/// * `name` - Name for the keystore file
/// * `private_key` - Optional private key as a hex string. If None, the user will be prompted.
///
/// # Security Note
///
/// For better security, avoid passing the private key as a command-line argument
/// (it may be visible in shell history). Instead, omit the argument and enter
/// the key when prompted.
///
/// # Examples
///
/// ```text
/// $ pget method import --name imported-wallet
/// Importing private key to keystore: imported-wallet
/// Enter private key to import: 0x1234...
/// Enter password to encrypt the keystore: ****
/// Private key imported to: /home/user/.pget/keystores/imported-wallet.json
///   Address: 0xabcd...
/// ```
///
/// # Errors
///
/// Returns an error if:
/// - The private key is invalid or malformed
/// - The keystore directory cannot be accessed
/// - Password confirmation fails
/// - File write operation fails
pub fn import_command(name: &str, private_key: Option<String>) -> Result<()> {
    println!("Importing private key to keystore: {name}");

    let private_key = private_key.unwrap_or_else(|| {
        Input::new()
            .with_prompt("Enter private key to import (hex, with or without 0x prefix)")
            .interact_text()
            .expect("Failed to read private key")
    });

    // Get password for encryption
    let password = Password::new()
        .with_prompt("Enter password to encrypt the keystore")
        .with_confirmation("Confirm password", "Passwords do not match")
        .interact()?;

    // Create the keystore
    let keystore_path = create_keystore(&private_key, &password, name)?;

    println!("Private key imported to: {}", keystore_path.display());

    // Show the address
    if let Ok(content) = std::fs::read_to_string(&keystore_path) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(address) = json["address"].as_str() {
                println!("  Address: 0x{address}");
            }
        }
    }

    println!();
    println!("To use this keystore, update your config file:");
    println!("[evm]");
    println!("keystore = \"{}\"", keystore_path.display());

    Ok(())
}

/// Helper function to find a keystore by name
fn find_keystore_by_name(name: &str) -> Result<PathBuf> {
    let keystores = list_keystores()?;

    // Try exact match with .json extension
    let name_with_json = format!("{name}.json");

    for keystore_path in keystores {
        if let Some(filename) = keystore_path.file_name().and_then(|f| f.to_str()) {
            if filename == name_with_json || filename == name {
                return Ok(keystore_path);
            }
        }
    }

    anyhow::bail!("Keystore '{name}' not found. Use 'pget method list' to see available keystores.")
}

/// Display keystore details without revealing the private key
///
/// Shows metadata about a keystore file including its address, creation date,
/// file size, and encryption details. This command does NOT require a password
/// and does NOT decrypt or display the private key.
///
/// # Arguments
///
/// * `name` - Name of the keystore to show (with or without .json extension)
///
/// # Examples
///
/// ```text
/// $ pget method show --name my-wallet
/// Keystore Details:
///
/// Name: my-wallet
/// Path: /home/user/.pget/keystores/my-wallet.json
/// Address: 0xabcd1234...
///   Created: SystemTime { ... }
///   Modified: SystemTime { ... }
///   Size: 491 bytes
/// Encryption: Standard Ethereum keystore format
/// Cipher: aes-128-ctr
/// KDF: scrypt
/// ```
///
/// # Errors
///
/// Returns an error if:
/// - The keystore with the given name is not found
/// - The keystore file cannot be read
/// - The keystore format is invalid
pub fn show_command(name: &str) -> Result<()> {
    let keystore_path = find_keystore_by_name(name)?;
    let keystore = Keystore::load(&keystore_path)?;

    println!("Keystore Details:");
    println!();
    println!("Name: {name}");
    println!("Path: {}", keystore_path.display());

    if let Some(address) = keystore.formatted_address() {
        println!("Address: {address}");
    } else {
        println!("Address: (not available)");
    }

    if let Ok(metadata) = std::fs::metadata(&keystore_path) {
        if let Ok(created) = metadata.created() {
            if let Ok(datetime) = created.duration_since(std::time::UNIX_EPOCH) {
                let secs = datetime.as_secs();
                // Simple date formatting (YYYY-MM-DD HH:MM:SS)
                use std::time::SystemTime;
                let system_time = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs);
                println!("  Created: {system_time:?}");
            }
        }

        if let Ok(modified) = metadata.modified() {
            if let Ok(datetime) = modified.duration_since(std::time::UNIX_EPOCH) {
                let secs = datetime.as_secs();
                use std::time::SystemTime;
                let system_time = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs);
                println!("  Modified: {system_time:?}");
            }
        }

        println!("  Size: {} bytes", metadata.len());
    }

    if keystore.content.get("crypto").is_some() {
        println!("Encryption: Standard Ethereum keystore format");

        if let Some(cipher) = keystore.content["crypto"]["cipher"].as_str() {
            println!("Cipher: {cipher}");
        }

        if let Some(kdf) = keystore.content["crypto"]["kdf"].as_str() {
            println!("KDF: {kdf}");
        }
    } else if keystore.content.get("Crypto").is_some() {
        println!("Encryption: Standard Ethereum keystore format (uppercase)");
    } else {
        println!("Encryption: Unknown format");
    }

    println!();

    Ok(())
}

/// Verify keystore integrity and password correctness
///
/// Performs a comprehensive verification of a keystore file by:
/// 1. Validating the keystore format and structure
/// 2. Checking that the address field is present
/// 3. Attempting to decrypt the keystore with the provided password
/// 4. Deriving the address from the decrypted private key
/// 5. Verifying that the derived address matches the stored address
///
/// This command requires the keystore password and will fail if the password
/// is incorrect or if the keystore is corrupted.
///
/// # Arguments
///
/// * `name` - Name of the keystore to verify (with or without .json extension)
///
/// # Examples
///
/// ```text
/// $ pget method verify --name my-wallet
/// Verifying keystore: my-wallet
///
/// [OK] Keystore format is valid
/// [OK] Address field present
/// Enter password to verify keystore integrity: ****
/// [OK] Successfully decrypted keystore
/// [OK] Address derivation matches
/// Verification successful!
/// Address: 0xabcd1234...
/// ```
///
/// # Errors
///
/// Returns an error if:
/// - The keystore with the given name is not found
/// - The keystore format is invalid
/// - The password is incorrect
/// - The stored address doesn't match the derived address (indicating corruption)
pub fn verify_command(name: &str) -> Result<()> {
    let keystore_path = find_keystore_by_name(name)?;
    let keystore = Keystore::load(&keystore_path)?;

    println!("Verifying keystore: {name}");
    println!();

    match keystore.validate() {
        Ok(()) => {
            println!("[OK] Keystore format is valid");
        }
        Err(e) => {
            println!("[FAIL] Keystore format is invalid: {e}");
            return Err(e.into());
        }
    }

    if keystore.address().is_some() {
        println!("[OK] Address field present");
    } else {
        println!("[WARN] Address field missing");
    }

    let password = Password::new()
        .with_prompt("Enter password to verify keystore integrity")
        .allow_empty_password(false)
        .interact()?;

    match keystore.decrypt(&password) {
        Ok(private_key_bytes) => {
            println!("[OK] Successfully decrypted keystore");

            if let Some(stored_address) = keystore.address() {
                match derive_evm_address(&private_key_bytes) {
                    Ok(derived_address) => {
                        let derived_hex = format!("{:x}", derived_address);

                        if stored_address.to_lowercase() == derived_hex.to_lowercase() {
                            println!("[OK] Address derivation matches");
                            println!("Verification successful!");
                            println!("Address: 0x{stored_address}");
                        } else {
                            println!("[FAIL] Address mismatch!");
                            println!("Stored:  0x{stored_address}");
                            println!("Derived: 0x{derived_hex}");
                            anyhow::bail!("Address derivation does not match stored address");
                        }
                    }
                    Err(e) => {
                        println!("[WARN] Could not derive address from private key: {e}");
                        println!("Address: 0x{stored_address}");
                    }
                }
            } else {
                println!("[WARN] No address stored in keystore to verify against");
            }
        }
        Err(e) => {
            println!("[FAIL] Failed to decrypt keystore: {e}");
            anyhow::bail!("Keystore decryption failed");
        }
    }

    Ok(())
}
