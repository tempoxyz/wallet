//! Create local wallets and renew access keys.

use alloy::signers::local::PrivateKeySigner;
use anyhow::Result;
use zeroize::Zeroizing;

use super::keychain::keychain;
use tempo_common::error::{ConfigError, KeyError};
use tempo_common::keys::{authorization, parse_private_key_signer, KeyEntry, Keystore, WalletType};
use tempo_common::network::NetworkId;

/// Create a local EOA wallet with a signing key.
///
/// 1. Generate random EOA key and store it in OS keychain (wallet owner key)
/// 2. Generate a random access key and store inline in keys.toml
/// 3. Sign key_authorization for the target chain
/// 4. Do not provision yet; first paid request auto-provisions
/// 5. Return the fundable wallet address
pub(super) fn create_local_wallet(network: &NetworkId, keys: &Keystore) -> Result<String> {
    if keys.ephemeral {
        anyhow::bail!(ConfigError::Invalid(
            "Cannot create wallets with --private-key flag".to_string()
        ));
    }

    let mut keys = keys.clone();

    // Generate wallet EOA key and store in OS keychain.
    let wallet_signer = PrivateKeySigner::random();
    let wallet_key_hex = Zeroizing::new(format!("0x{}", hex::encode(wallet_signer.to_bytes())));
    let wallet_address = wallet_signer.address().to_string();

    keychain()
        .set(&wallet_address, &wallet_key_hex)
        .map_err(|e| KeyError::Keychain(format!("Failed to store wallet key: {e}")))?;

    // Generate access key.
    let access_signer = PrivateKeySigner::random();
    let access_key_hex = Zeroizing::new(format!("0x{}", hex::encode(access_signer.to_bytes())));
    let access_key_address = access_signer.address().to_string();

    // Sign key_authorization for the target chain.
    let chain_id = network.chain_id();
    let auth = authorization::sign(&wallet_signer, &access_signer, chain_id)?;

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
    keys.keys.push(key_entry);

    if let Err(e) = keys.save() {
        if let Err(del_err) = keychain().delete(&wallet_address) {
            tracing::warn!("Failed to clean up keychain entry for {wallet_address}: {del_err}");
        }
        return Err(e.into());
    }

    Ok(wallet_address)
}

/// Renew the access key for an existing local wallet.
///
/// 1. Load the wallet EOA key from the OS keychain
/// 2. Generate a new random access key and store inline in keys.toml
/// 3. Sign a fresh key_authorization
/// 4. Clear provisioned flag (new key must re-provision)
pub(crate) fn create_access_key(
    wallet_address: Option<&str>,
    keys: &Keystore,
) -> Result<()> {
    if keys.ephemeral {
        anyhow::bail!(ConfigError::Invalid(
            "Cannot renew wallets with --private-key flag".to_string()
        ));
    }

    let mut keys = keys.clone();
    let idx = if let Some(addr) = wallet_address {
        keys.keys
            .iter()
            .position(|k| {
                k.wallet_address.eq_ignore_ascii_case(addr) && k.wallet_type == WalletType::Local
            })
            .ok_or_else(|| {
                ConfigError::Missing(format!("No local wallet found for address '{addr}'."))
            })?
    } else {
        let mut local_iter = keys
            .keys
            .iter()
            .enumerate()
            .filter(|(_, k)| k.wallet_type == WalletType::Local)
            .map(|(i, _)| i);

        match (local_iter.next(), local_iter.next()) {
            (None, _) => anyhow::bail!(ConfigError::Missing("No local wallet found.".to_string())),
            (Some(i), None) => i,
            (Some(_), Some(_)) => anyhow::bail!(ConfigError::Invalid(
                "Multiple local wallets found. Specify --wallet <address>.".to_string()
            )),
        }
    };

    let key_entry = &keys.keys[idx];

    // Load wallet EOA key from OS keychain.
    let wallet_key_hex = keychain()
        .get(&key_entry.wallet_address)
        .map_err(|e| KeyError::Keychain(format!("Failed to load wallet key: {e}")))?
        .ok_or_else(|| {
            KeyError::Keychain(format!(
                "Wallet key not found in keychain for '{}'. The wallet may need to be re-created.",
                key_entry.wallet_address
            ))
        })?;

    let wallet_signer: PrivateKeySigner = parse_private_key_signer(&wallet_key_hex)
        .map_err(|e| KeyError::Keychain(format!("Invalid wallet key in keychain: {e}")))?;

    // Generate new access key.
    let access_signer = PrivateKeySigner::random();
    let access_key_hex = Zeroizing::new(format!("0x{}", hex::encode(access_signer.to_bytes())));
    let access_key_address = access_signer.address().to_string();

    // Sign key_authorization with fresh expiry.
    let chain_id = key_entry.chain_id;
    let auth = authorization::sign(&wallet_signer, &access_signer, chain_id)?;

    // Update key entry in-place.
    let entry = &mut keys.keys[idx];
    entry.key_address = Some(access_key_address);
    entry.key = Some(access_key_hex);
    entry.key_authorization = Some(auth.hex);
    entry.key_type = auth.key_type;
    entry.provisioned = false;
    entry.expiry = Some(auth.expiry);
    entry.limits = auth.limits;

    keys.save()?;
    Ok(())
}
