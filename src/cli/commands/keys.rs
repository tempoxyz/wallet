//! Access key management commands.

use crate::cli::util::{format_expiry, now_secs};
use crate::cli::OutputFormat;
use crate::error::{PrestoError, Result};
use crate::wallet::credentials::WalletCredentials;
use serde::Serialize;

#[derive(Debug, Serialize)]
struct KeyInfo {
    index: usize,
    label: String,
    address: String,
    expiry: u64,
    expired: bool,
    expires_in: String,
    active: bool,
}

#[derive(Debug, Serialize)]
struct KeysList {
    wallet: String,
    active_key_index: usize,
    keys: Vec<KeyInfo>,
}

pub async fn list_keys(output_format: OutputFormat, network: Option<&str>) -> Result<()> {
    let mut creds = WalletCredentials::load()?;
    if let Some(n) = network {
        creds.network = n.to_string();
    }

    let wallet = match creds.active_wallet() {
        Some(w) => w,
        None => {
            match output_format {
                OutputFormat::Json => {
                    println!("{}", serde_json::json!({"error": "No wallet connected"}));
                }
                _ => {
                    return Err(PrestoError::ConfigMissing(
                        "No wallet connected. Run ' tempo-walletlogin' first.".to_string(),
                    ));
                }
            }
            return Ok(());
        }
    };

    match output_format {
        OutputFormat::Json => {
            let now = now_secs();
            let keys: Vec<KeyInfo> = wallet
                .access_keys
                .iter()
                .enumerate()
                .map(|(i, key)| KeyInfo {
                    index: i,
                    label: key.label.clone(),
                    address: key.address(),
                    expiry: key.expiry,
                    expired: key.expiry > 0 && key.expiry < now,
                    expires_in: format_expiry(key.expiry),
                    active: i == wallet.active_key_index,
                })
                .collect();

            let list = KeysList {
                wallet: wallet.account_address.clone(),
                active_key_index: wallet.active_key_index,
                keys,
            };

            println!("{}", serde_json::to_string_pretty(&list)?);
        }
        _ => {
            if wallet.access_keys.is_empty() {
                println!("No access keys.");
                return Ok(());
            }

            println!("Access keys for {}:", wallet.account_address);
            println!();

            for (i, key) in wallet.access_keys.iter().enumerate() {
                let marker = if i == wallet.active_key_index {
                    "→"
                } else {
                    " "
                };

                println!(
                    "{} [{}] {} - {} ({})",
                    marker,
                    i,
                    key.label,
                    key.address(),
                    format_expiry(key.expiry)
                );
            }
        }
    }

    Ok(())
}

/// Switch to a different access key.
pub async fn switch_key(
    index: usize,
    output_format: OutputFormat,
    network: Option<&str>,
) -> Result<Option<String>> {
    let mut creds = WalletCredentials::load()?;
    if let Some(n) = network {
        creds.network = n.to_string();
    }

    let wallet = creds.active_wallet_mut().ok_or_else(|| {
        PrestoError::ConfigMissing("No wallet connected. Run ' tempo-walletlogin' first.".to_string())
    })?;

    if !wallet.switch_key(index) {
        let err_msg = format!(
            "Invalid index {}. You have {} keys (0-{})",
            index,
            wallet.access_keys.len(),
            wallet.access_keys.len().saturating_sub(1)
        );
        match output_format {
            OutputFormat::Json => {
                println!("{}", serde_json::json!({"error": err_msg}));
                return Ok(None);
            }
            _ => return Err(PrestoError::InvalidConfig(err_msg)),
        }
    }

    let label = wallet.access_keys[index].label.clone();
    let address = wallet.access_keys[index].address();

    creds.save()?;

    match output_format {
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::json!({
                    "success": true,
                    "index": index,
                    "label": label,
                    "address": address
                })
            );
        }
        _ => {
            println!("Switched to key [{}]: {} - {}", index, label, address);
        }
    }

    Ok(Some(label))
}

/// Delete an access key.
pub async fn delete_key(
    index: usize,
    output_format: OutputFormat,
    network: Option<&str>,
) -> Result<Option<String>> {
    let mut creds = WalletCredentials::load()?;
    if let Some(n) = network {
        creds.network = n.to_string();
    }

    let wallet = creds.active_wallet_mut().ok_or_else(|| {
        PrestoError::ConfigMissing("No wallet connected. Run ' tempo-walletlogin' first.".to_string())
    })?;

    let key = match wallet.remove_key(index) {
        Some(k) => k,
        None => {
            let err_msg = format!(
                "Invalid index {}. You have {} keys (0-{})",
                index,
                wallet.access_keys.len(),
                wallet.access_keys.len().saturating_sub(1)
            );
            match output_format {
                OutputFormat::Json => {
                    println!("{}", serde_json::json!({"error": err_msg}));
                    return Ok(None);
                }
                _ => return Err(PrestoError::InvalidConfig(err_msg)),
            }
        }
    };

    let label = key.label.clone();
    creds.save()?;

    match output_format {
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::json!({
                    "success": true,
                    "deleted": {
                        "index": index,
                        "label": key.label,
                        "address": key.address()
                    }
                })
            );
        }
        _ => {
            println!("Deleted key [{}]: {} - {}", index, key.label, key.address());
        }
    }

    Ok(Some(label))
}
