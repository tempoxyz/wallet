//! Whoami command — unified wallet/balance/key info.

use crate::cli::commands::tempo_wallet::{query_all_balances, TokenBalance};
use crate::cli::util::{format_expiry, now_secs};
use crate::cli::OutputFormat;
use crate::error::Result;
use crate::wallet::credentials::WalletCredentials;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub(crate) struct AccessKeyInfo {
    index: usize,
    address: String,
    label: String,
    expiry: u64,
    expired: bool,
    active: bool,
}

#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub ready: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wallet: Option<String>,
    pub network: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub balances: Vec<TokenBalance>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub access_keys: Vec<AccessKeyInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_key_index: Option<usize>,
    pub issues: Vec<String>,
}

pub async fn show_whoami(output_format: OutputFormat, network: Option<&str>) -> Result<()> {
    let mut creds = WalletCredentials::load()?;
    if let Some(n) = network {
        creds.network = n.to_string();
    }

    let mut response = StatusResponse {
        ready: true,
        wallet: None,
        network: creds.network.clone(),
        balances: vec![],
        access_keys: vec![],
        active_key_index: None,
        issues: vec![],
    };

    if let Some(wallet) = creds.active_wallet() {
        response.wallet = Some(wallet.account_address.clone());
        response.active_key_index = Some(wallet.active_key_index);

        let now = now_secs();
        response.access_keys = wallet
            .access_keys
            .iter()
            .enumerate()
            .map(|(i, key)| AccessKeyInfo {
                index: i,
                address: key.address(),
                label: key.label.clone(),
                expiry: key.expiry,
                expired: key.expiry > 0 && key.expiry < now,
                active: i == wallet.active_key_index,
            })
            .collect();

        if let Some(key) = wallet.active_access_key() {
            if key.expiry > 0 && key.expiry < now {
                response.ready = false;
                response.issues.push("Access key expired".to_string());
            }
        } else {
            response.ready = false;
            response.issues.push("No access key".to_string());
        }

        response.balances = query_all_balances(&creds.network, &wallet.account_address).await;
    } else {
        response.ready = false;
        response
            .issues
            .push("No wallet connected. Run 'tempoctl login'.".to_string());
    }

    match output_format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string(&response)?);
        }
        _ => {
            if response.ready {
                println!("Ready to make requests\n");
            } else {
                println!("Not ready\n");
            }

            println!("Network: {}", response.network);

            if let Some(wallet) = &response.wallet {
                println!("Wallet: {}", wallet);
            }

            if !response.balances.is_empty() {
                println!();
                for tb in &response.balances {
                    println!("{:>16} {}", tb.balance, tb.token);
                }
            }

            if let Some(key) = response.access_keys.iter().find(|k| k.active) {
                println!();
                println!("Access Key: {}", key.address);
                if key.expired {
                    println!("  Status: Expired");
                } else if key.expiry > 0 {
                    println!("  Status: Active ({})", format_expiry(key.expiry));
                }
            }

            if response.access_keys.len() > 1 {
                println!("\nAll access keys ({}):", response.access_keys.len());
                for key in &response.access_keys {
                    let marker = if key.active { "→" } else { " " };
                    println!(
                        "  {} [{}] {} - {}",
                        marker, key.index, key.label, key.address
                    );
                }
            }

            if !response.issues.is_empty() {
                println!("\nIssues:");
                for issue in &response.issues {
                    println!("  - {}", issue);
                }
            }
        }
    }

    Ok(())
}
