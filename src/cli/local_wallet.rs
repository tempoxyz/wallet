//! Wallet management commands — create and delete wallets.

use std::io::{self, BufRead, IsTerminal, Write};

use alloy::signers::local::PrivateKeySigner;
use anyhow::Result;
use zeroize::{Zeroize, Zeroizing};

use crate::error::PrestoError;
use crate::network::networks::network_or_default;
use crate::network::Network;
use crate::wallet::credentials::{
    self, keychain, parse_private_key_signer, KeyEntry, WalletCredentials, WalletType,
};
use crate::wallet::key_authorization;

/// Create a local EOA wallet with a signing key.
///
/// 1. Generate random EOA key → store in OS keychain (wallet owner key)
/// 2. Generate random key → store inline in keys.toml
/// 3. Sign key_authorization for the target chain
/// 4. Do not provision; auto-provisions on first payment
/// 5. Print the fundable wallet address
pub fn create_local_wallet(name: &str, network: Option<&str>) -> Result<()> {
    if credentials::has_credentials_override() {
        anyhow::bail!("Cannot create wallets with --private-key flag");
    }

    let mut creds = WalletCredentials::load()?;
    if creds.keys.contains_key(name) {
        anyhow::bail!("Key '{name}' already exists. Use a different name.");
    }

    // Generate wallet EOA key and store in OS keychain
    let wallet_signer = PrivateKeySigner::random();
    let wallet_key_hex = Zeroizing::new(format!("0x{}", hex::encode(wallet_signer.to_bytes())));
    let wallet_address = wallet_signer.address().to_string();

    keychain()
        .set(name, &wallet_key_hex)
        .map_err(|e| PrestoError::Keychain(format!("Failed to store wallet key: {e}")))?;

    // Generate key
    let access_signer = PrivateKeySigner::random();
    let access_key_hex = Zeroizing::new(format!("0x{}", hex::encode(access_signer.to_bytes())));
    let access_key_address = access_signer.address().to_string();

    // Sign key_authorization for the target chain
    let network_str = network_or_default(network);
    let chain_id = network_str
        .parse::<Network>()
        .map(|n| n.chain_id())
        .map_err(|_| {
            anyhow::anyhow!("Unknown network '{network_str}'. Use 'tempo' or 'tempo-moderato'.")
        })?;
    let auth = key_authorization::sign(&wallet_signer, &access_signer, chain_id)?;

    let key_entry = KeyEntry {
        wallet_type: WalletType::Local,
        wallet_address,
        key_address: Some(access_key_address),
        key: Some(access_key_hex),
        key_authorization: Some(auth.hex),
        chain_id,
        key_type: auth.key_type,
        expiry: Some(auth.expiry),
        token_limits: auth.token_limits,
        provisioned: false,
    };
    creds.keys.insert(name.to_string(), key_entry);
    if let Err(e) = creds.save() {
        let _ = keychain().delete(name);
        return Err(e.into());
    }

    Ok(())
}

/// Renew the key for an existing local wallet.
///
/// 1. Load the wallet EOA key from the OS keychain
/// 2. Generate a new random key → store inline in keys.toml
/// 3. Sign a fresh key_authorization (30-day expiry, $100 limit)
/// 4. Clear provisioned flag (new key must re-provision)
pub fn create_access_key(name: &str) -> Result<()> {
    if credentials::has_credentials_override() {
        anyhow::bail!("Cannot renew wallets with --private-key flag");
    }

    let mut creds = WalletCredentials::load()?;
    let key_entry = creds
        .keys
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("Wallet '{name}' not found."))?;

    if key_entry.wallet_type != WalletType::Local {
        anyhow::bail!("Only local wallets can be renewed. Use 'presto login' for passkey wallets.");
    }

    // Load wallet EOA key from OS keychain
    let wallet_key_hex = keychain()
        .get(name)
        .map_err(|e| PrestoError::Keychain(format!("Failed to load wallet key: {e}")))?
        .ok_or_else(|| {
            anyhow::anyhow!(
            "Wallet key not found in keychain for '{name}'. The wallet may need to be re-created."
        )
        })?;
    let wallet_signer: PrivateKeySigner = parse_private_key_signer(&wallet_key_hex)
        .map_err(|e| anyhow::anyhow!("Invalid wallet key in keychain: {e}"))?;

    // Generate new key
    let access_signer = PrivateKeySigner::random();
    let access_key_hex = Zeroizing::new(format!("0x{}", hex::encode(access_signer.to_bytes())));
    let access_key_address = access_signer.address().to_string();

    // Sign key_authorization with fresh expiry
    let chain_id = key_entry.chain_id;
    let auth = key_authorization::sign(&wallet_signer, &access_signer, chain_id)?;

    // Update the key entry in-place
    let entry = creds.keys.get_mut(name).unwrap();
    entry.key_address = Some(access_key_address);
    entry.key = Some(access_key_hex);
    entry.key_authorization = Some(auth.hex);
    entry.provisioned = false;
    entry.expiry = Some(auth.expiry);
    entry.token_limits = auth.token_limits;

    creds.save()?;
    Ok(())
}

/// Delete a wallet by name.
pub fn delete_wallet(name: &str, yes: bool) -> Result<()> {
    let creds = WalletCredentials::load()?;

    if !creds.keys.contains_key(name) {
        anyhow::bail!("Wallet '{name}' not found.");
    }

    if !yes {
        if !io::stdin().is_terminal() {
            anyhow::bail!("Use --yes for non-interactive delete");
        }

        print!("Delete wallet '{name}'? [y/N] ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Cancelled.");
            return Ok(());
        }
    }

    let mut creds = creds;
    creds.delete_key(name)?;
    creds.save()?;

    if creds.keys.is_empty() {
        println!("Deleted wallet '{name}'. No wallets configured.");
    } else {
        println!("Deleted wallet '{name}'.");
    }
    Ok(())
}

/// Import an existing EOA private key as a local wallet.
///
/// Reads the private key from `--private-key`, `--stdin-key`, or prompts interactively
/// (masked) on a TTY. Stores the key in the OS keychain and records the wallet
/// address in keys.toml. Does not create a key; run `presto login` to connect
/// and provision when ready.
pub fn import_wallet(name: &str, private_key_arg: Option<String>, stdin_key: bool) -> Result<()> {
    if credentials::has_credentials_override() {
        anyhow::bail!("Cannot import wallets with --private-key flag");
    }

    let mut creds = WalletCredentials::load()?;
    if creds.keys.contains_key(name) {
        anyhow::bail!("Wallet '{name}' already exists. Use a different name.");
    }

    let key_hex: Zeroizing<String> = if let Some(pk) = private_key_arg {
        Zeroizing::new(pk)
    } else if stdin_key {
        read_private_key_noninteractive()? // single-line from stdin
    } else {
        read_private_key()? // interactive masked prompt or pipe
    };

    // Parse and validate the key using shared helper
    let signer: PrivateKeySigner =
        parse_private_key_signer(&key_hex).map_err(anyhow::Error::from)?;

    let private_key_hex = Zeroizing::new(format!("0x{}", hex::encode(signer.to_bytes())));
    let address = signer.address().to_string();

    // Store wallet EOA key in OS keychain
    keychain()
        .set(name, &private_key_hex)
        .map_err(|e| PrestoError::Keychain(format!("Failed to store key: {e}")))?;

    let key = KeyEntry {
        wallet_type: WalletType::Local,
        wallet_address: address.clone(),
        ..Default::default()
    };
    creds.keys.insert(name.to_string(), key);
    if let Err(e) = creds.save() {
        let _ = keychain().delete(name);
        return Err(e.into());
    }

    println!("Imported wallet '{name}'.");
    println!("  Address: {address}");
    println!("\nRun 'presto login' to connect and authorize payments.");
    Ok(())
}

/// Read a private key from stdin.
///
/// Disables terminal echo on Unix to avoid leaking the key,
/// or reads silently from a pipe.
fn read_private_key() -> Result<Zeroizing<String>> {
    if !io::stdin().is_terminal() {
        return read_private_key_noninteractive();
    }

    struct EchoGuard;
    impl Drop for EchoGuard {
        fn drop(&mut self) {
            let _ = std::process::Command::new("stty").args(["echo"]).status();
            let _ = writeln!(io::stdout());
        }
    }

    print!("Enter private key: ");
    io::stdout().flush()?;

    // Disable echo with a guard to ensure restoration on all paths
    let _echo_guard = EchoGuard;
    let _ = std::process::Command::new("stty").args(["-echo"]).status();

    let mut input = String::new();
    io::stdin().lock().read_line(&mut input)?;

    let key = Zeroizing::new(input.trim().to_string());
    input.zeroize();
    if key.is_empty() {
        anyhow::bail!("No private key provided");
    }
    Ok(key)
}

/// Read a private key from stdin non-interactively (one line, no prompts).
fn read_private_key_noninteractive() -> Result<Zeroizing<String>> {
    let mut input = String::new();
    io::stdin().lock().read_line(&mut input)?;
    let key = Zeroizing::new(input.trim().to_string());
    input.zeroize();
    if key.is_empty() {
        anyhow::bail!("No private key provided on stdin");
    }
    Ok(key)
}
