//! Wallet management commands — create local wallets and renew keys.

use alloy::signers::local::PrivateKeySigner;
use anyhow::Result;
use zeroize::Zeroizing;

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
pub(crate) fn create_local_wallet(network: Option<&str>) -> Result<String> {
    if credentials::has_credentials_override() {
        anyhow::bail!("Cannot create wallets with --private-key flag");
    }

    let mut creds = WalletCredentials::load()?;

    // Generate wallet EOA key and store in OS keychain
    let wallet_signer = PrivateKeySigner::random();
    let wallet_key_hex = Zeroizing::new(format!("0x{}", hex::encode(wallet_signer.to_bytes())));
    let wallet_address = wallet_signer.address().to_string();

    keychain()
        .set(&wallet_address, &wallet_key_hex)
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
        wallet_address: wallet_address.clone(),
        key_address: Some(access_key_address),
        key: Some(access_key_hex),
        key_authorization: Some(auth.hex),
        chain_id,
        key_type: auth.key_type,
        expiry: Some(auth.expiry),
        limits: auth.limits,
        provisioned: false,
    };
    creds.keys.push(key_entry);
    if let Err(e) = creds.save() {
        let _ = keychain().delete(&wallet_address);
        return Err(e.into());
    }

    Ok(wallet_address)
}

/// Renew the key for an existing local wallet.
///
/// 1. Load the wallet EOA key from the OS keychain
/// 2. Generate a new random key → store inline in keys.toml
/// 3. Sign a fresh key_authorization (30-day expiry, $100 limit)
/// 4. Clear provisioned flag (new key must re-provision)
pub(crate) fn create_access_key(wallet_address: Option<&str>) -> Result<()> {
    if credentials::has_credentials_override() {
        anyhow::bail!("Cannot renew wallets with --private-key flag");
    }

    let mut creds = WalletCredentials::load()?;
    let idx = if let Some(addr) = wallet_address {
        creds
            .keys
            .iter()
            .position(|k| {
                k.wallet_address.eq_ignore_ascii_case(addr) && k.wallet_type == WalletType::Local
            })
            .ok_or_else(|| anyhow::anyhow!("No local wallet found for address '{addr}'."))?
    } else {
        let local_indices: Vec<_> = creds
            .keys
            .iter()
            .enumerate()
            .filter(|(_, k)| k.wallet_type == WalletType::Local)
            .map(|(i, _)| i)
            .collect();
        match local_indices.len() {
            0 => anyhow::bail!("No local wallet found."),
            1 => local_indices[0],
            _ => anyhow::bail!("Multiple local wallets found. Specify --wallet <address>."),
        }
    };

    let key_entry = &creds.keys[idx];

    // Load wallet EOA key from OS keychain
    let wallet_key_hex = keychain()
        .get(&key_entry.wallet_address)
        .map_err(|e| PrestoError::Keychain(format!("Failed to load wallet key: {e}")))?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Wallet key not found in keychain for '{}'. The wallet may need to be re-created.",
                key_entry.wallet_address
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
    let entry = &mut creds.keys[idx];
    entry.key_address = Some(access_key_address);
    entry.key = Some(access_key_hex);
    entry.key_authorization = Some(auth.hex);
    entry.provisioned = false;
    entry.expiry = Some(auth.expiry);
    entry.limits = auth.limits;

    creds.save()?;
    Ok(())
}
