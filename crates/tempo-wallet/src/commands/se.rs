//! Secure Enclave key management commands.

use alloy::primitives::Address;
use colored::Colorize;

use tempo_common::{
    cli::context::Context,
    error::{KeyError, TempoError},
    keys::{secure_enclave, KeyEntry, KeyType, WalletType},
};

use super::whoami::show_whoami;

/// Generate a new Secure Enclave key and register it with the wallet.
///
/// The SE key's derived address becomes the wallet address (direct P-256 signer).
/// No browser login is required — the SE key IS the wallet.
pub(crate) async fn generate(ctx: &Context, label: Option<String>) -> Result<(), TempoError> {
    if !cfg!(target_os = "macos") {
        return Err(KeyError::SecureEnclave(
            "Secure Enclave is only available on macOS".to_string(),
        )
        .into());
    }

    let label = label.unwrap_or_else(|| "default".to_string());

    // Refuse to overwrite an existing SE key — funds would be lost.
    if ctx
        .keys
        .iter()
        .any(|k| k.se_label.as_deref() == Some(&label))
    {
        return Err(KeyError::SecureEnclave(format!(
            "SE key '{label}' already exists. Delete it first with: tempo wallet se delete"
        ))
        .into());
    }

    eprintln!("Generating Secure Enclave key...");

    // Generate the key via the Swift shim
    let pubkey_hex = secure_enclave::generate(&label)?;

    eprintln!("  Public key: 0x{}", &pubkey_hex[..16]);
    eprintln!("  Label: {}", label.bold());

    // Derive the address from the uncompressed P-256 public key.
    // EVM address = keccak256(pubkey_bytes[1..])[12..] (skip the 04 prefix)
    let pubkey_bytes = hex::decode(&pubkey_hex)
        .map_err(|_| KeyError::SecureEnclave("invalid public key hex from SE".to_string()))?;
    if pubkey_bytes.len() != 65 || pubkey_bytes[0] != 0x04 {
        return Err(
            KeyError::SecureEnclave("unexpected public key format from SE".to_string()).into(),
        );
    }
    let key_address = alloy::primitives::keccak256(&pubkey_bytes[1..]);
    let key_address = Address::from_slice(&key_address[12..]);

    // The SE key address IS the wallet address (direct P-256 signer).
    let chain_id = ctx.network.chain_id();
    let mut keys = ctx.keys.clone();
    let entry = keys.upsert_by_wallet_address_and_chain(key_address, chain_id);
    entry.wallet_type = WalletType::Local;
    entry.key_type = KeyType::SecureEnclave;
    entry.set_wallet_address(key_address);
    entry.set_key_address(Some(key_address));
    entry.se_label = Some(label.clone());
    entry.key = None;
    keys.save()?;

    eprintln!("\n{}", "Secure Enclave key registered!".green());
    eprintln!("  Wallet address: {key_address:#x}");
    eprintln!();
    eprintln!("The key's private key never leaves the Secure Enclave hardware.");
    eprintln!("Fund this address to start making payments.");

    let keys = ctx.keys.reload()?;
    show_whoami(ctx, Some(&keys), None).await
}

/// Show the public key for an existing SE key.
pub(crate) fn pubkey(ctx: &Context, label: Option<String>) -> Result<(), TempoError> {
    let label = resolve_label(ctx, label)?;
    let pubkey_hex = secure_enclave::pubkey(&label)?;
    println!("0x{pubkey_hex}");
    Ok(())
}

/// Delete an SE key from the Keychain.
pub(crate) fn delete(ctx: &Context, label: Option<String>, yes: bool) -> Result<(), TempoError> {
    let label = resolve_label(ctx, label)?;

    if !yes {
        eprintln!("This will permanently delete the Secure Enclave key '{label}'.");
        eprintln!("The key CANNOT be recovered. Are you sure?");
        eprintln!();
        return Err(tempo_common::error::InputError::NonInteractiveConfirmationRequired.into());
    }

    secure_enclave::delete(&label)?;

    // Remove SE entries with this label from the keystore
    let mut keys = ctx.keys.clone();
    keys.delete_se_label(&label)?;
    keys.save()?;
    eprintln!("Deleted Secure Enclave key '{label}'.");
    Ok(())
}

/// List SE keys in the keystore.
pub(crate) fn list(ctx: &Context) -> Result<(), TempoError> {
    let se_keys: Vec<&KeyEntry> = ctx.keys.iter().filter(|k| k.is_secure_enclave()).collect();

    if se_keys.is_empty() {
        eprintln!("No Secure Enclave keys configured.");
        eprintln!("Generate one with: tempo wallet se generate");
        return Ok(());
    }

    for entry in &se_keys {
        if let Some(label) = &entry.se_label {
            println!("Label:   {label}");
        }
        if let Some(addr) = entry.key_address_hex() {
            println!("Address: {addr}");
        }
        if let Some(wallet) = entry.wallet_address_hex() {
            println!("Wallet:  {wallet}");
        }
        println!();
    }
    println!("{} SE key(s) total.", se_keys.len());
    Ok(())
}

/// Resolve the label: use provided or find from keystore.
fn resolve_label(ctx: &Context, label: Option<String>) -> Result<String, TempoError> {
    if let Some(l) = label {
        return Ok(l);
    }
    // Try to find an SE key in the keystore
    ctx.keys
        .iter()
        .find_map(|k| k.se_label.clone())
        .ok_or_else(|| {
            KeyError::SecureEnclave(
                "No SE key found. Specify --label or generate one first.".to_string(),
            )
            .into()
        })
}
