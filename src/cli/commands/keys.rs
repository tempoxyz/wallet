//! Access key management commands.

use crate::cli::OutputFormat;
use crate::error::{Result, TempoCtlError};
use crate::wallet::credentials::WalletCredentials;

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
        TempoCtlError::ConfigMissing("No wallet connected. Run 'tempoctl login' first.".to_string())
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
            _ => return Err(TempoCtlError::InvalidConfig(err_msg)),
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
        TempoCtlError::ConfigMissing("No wallet connected. Run 'tempoctl login' first.".to_string())
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
                _ => return Err(TempoCtlError::InvalidConfig(err_msg)),
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
