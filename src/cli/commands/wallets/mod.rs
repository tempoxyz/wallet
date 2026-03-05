//! Wallet management commands — create local wallets, renew keys, and list wallets.

mod fund;
mod keychain;

use std::collections::BTreeMap;

use alloy::signers::local::PrivateKeySigner;
use anyhow::Result;
use clap::CommandFactory;
use serde::Serialize;
use zeroize::Zeroizing;

use self::keychain::keychain;
use crate::cli::args::WalletCommands;
use crate::cli::{Cli, Context, OutputFormat};
use crate::error::TempoWalletError;
use crate::keys::authorization;
use crate::keys::{parse_private_key_signer, KeyEntry, Keystore, WalletType};
use crate::network::NetworkId;
use crate::util::sanitize_error;

pub(crate) async fn run(ctx: &Context, command: Option<WalletCommands>) -> Result<()> {
    if let Some(subcommand) = command {
        match subcommand {
            WalletCommands::List => show_wallet_list(ctx.output_format, &ctx.keys).await,
            WalletCommands::Create => {
                let result = create_local_wallet(&ctx.network, &ctx.keys);
                if result.is_ok() {
                    if let Some(a) = ctx.analytics.as_ref() {
                        a.track(
                            crate::analytics::Event::WalletCreated,
                            crate::analytics::WalletCreatedPayload {
                                network: ctx.network.as_str().to_string(),
                                wallet_type: "local".to_string(),
                            },
                        );
                    }
                }
                let wallet_addr = result?;
                let fresh_keys = ctx.keys.reload()?;
                super::whoami::show_whoami(
                    &ctx.config,
                    ctx.output_format,
                    ctx.network,
                    Some(&wallet_addr),
                    &fresh_keys,
                )
                .await
            }
            WalletCommands::Fund { address, no_wait } => {
                let method = match ctx.network {
                    NetworkId::TempoModerato => "faucet",
                    NetworkId::Tempo => "bridge",
                };
                if let Some(a) = ctx.analytics.as_ref() {
                    a.track(
                        crate::analytics::Event::WalletFundStarted,
                        crate::analytics::WalletFundPayload {
                            network: ctx.network.as_str().to_string(),
                            method: method.to_string(),
                        },
                    );
                }
                let result = fund::run(
                    &ctx.config,
                    ctx.output_format,
                    ctx.network,
                    address,
                    no_wait,
                    &ctx.keys,
                )
                .await;
                if let Some(a) = ctx.analytics.as_ref() {
                    match &result {
                        Ok(()) => {
                            a.track(
                                crate::analytics::Event::WalletFundSuccess,
                                crate::analytics::WalletFundPayload {
                                    network: ctx.network.as_str().to_string(),
                                    method: method.to_string(),
                                },
                            );
                        }
                        Err(e) => {
                            a.track(
                                crate::analytics::Event::WalletFundFailure,
                                crate::analytics::WalletFundFailurePayload {
                                    network: ctx.network.as_str().to_string(),
                                    method: method.to_string(),
                                    error: sanitize_error(&e.to_string()),
                                },
                            );
                        }
                    }
                }
                result
            }
        }
    } else {
        if let Some(wallet_cmd) = Cli::command().find_subcommand_mut("wallets") {
            wallet_cmd.print_help()?;
        } else {
            Cli::command().print_help()?;
        }
        Ok(())
    }
}

/// Create a local EOA wallet with a signing key.
///
/// 1. Generate random EOA key → store in OS keychain (wallet owner key)
/// 2. Generate random key → store inline in keys.toml
/// 3. Sign key_authorization for the target chain
/// 4. Do not provision; auto-provisions on first payment
/// 5. Print the fundable wallet address
fn create_local_wallet(network: &NetworkId, keys: &Keystore) -> Result<String> {
    if keys.ephemeral {
        anyhow::bail!(TempoWalletError::InvalidConfig(
            "Cannot create wallets with --private-key flag".to_string()
        ));
    }

    let mut keys = keys.clone();

    // Generate wallet EOA key and store in OS keychain
    let wallet_signer = PrivateKeySigner::random();
    let wallet_key_hex = Zeroizing::new(format!("0x{}", hex::encode(wallet_signer.to_bytes())));
    let wallet_address = wallet_signer.address().to_string();

    keychain()
        .set(&wallet_address, &wallet_key_hex)
        .map_err(|e| TempoWalletError::Keychain(format!("Failed to store wallet key: {e}")))?;

    // Generate key
    let access_signer = PrivateKeySigner::random();
    let access_key_hex = Zeroizing::new(format!("0x{}", hex::encode(access_signer.to_bytes())));
    let access_key_address = access_signer.address().to_string();

    // Sign key_authorization for the target chain
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

/// Renew the key for an existing local wallet.
///
/// 1. Load the wallet EOA key from the OS keychain
/// 2. Generate a new random key → store inline in keys.toml
/// 3. Sign a fresh key_authorization (30-day expiry, $100 limit)
/// 4. Clear provisioned flag (new key must re-provision)
pub(super) fn create_access_key(wallet_address: Option<&str>, keys: &Keystore) -> Result<()> {
    if keys.ephemeral {
        anyhow::bail!(TempoWalletError::InvalidConfig(
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
                TempoWalletError::ConfigMissing(format!(
                    "No local wallet found for address '{addr}'."
                ))
            })?
    } else {
        let local_indices: Vec<_> = keys
            .keys
            .iter()
            .enumerate()
            .filter(|(_, k)| k.wallet_type == WalletType::Local)
            .map(|(i, _)| i)
            .collect();
        match local_indices.len() {
            0 => anyhow::bail!(TempoWalletError::ConfigMissing(
                "No local wallet found.".to_string()
            )),
            1 => local_indices[0],
            _ => anyhow::bail!(TempoWalletError::InvalidConfig(
                "Multiple local wallets found. Specify --wallet <address>.".to_string()
            )),
        }
    };

    let key_entry = &keys.keys[idx];

    // Load wallet EOA key from OS keychain
    let wallet_key_hex = keychain()
        .get(&key_entry.wallet_address)
        .map_err(|e| TempoWalletError::Keychain(format!("Failed to load wallet key: {e}")))?
        .ok_or_else(|| {
            TempoWalletError::Keychain(format!(
                "Wallet key not found in keychain for '{}'. The wallet may need to be re-created.",
                key_entry.wallet_address
            ))
        })?;
    let wallet_signer: PrivateKeySigner = parse_private_key_signer(&wallet_key_hex)
        .map_err(|e| TempoWalletError::Keychain(format!("Invalid wallet key in keychain: {e}")))?;

    // Generate new key
    let access_signer = PrivateKeySigner::random();
    let access_key_hex = Zeroizing::new(format!("0x{}", hex::encode(access_signer.to_bytes())));
    let access_key_address = access_signer.address().to_string();

    // Sign key_authorization with fresh expiry
    let chain_id = key_entry.chain_id;
    let auth = authorization::sign(&wallet_signer, &access_signer, chain_id)?;

    // Update the key entry in-place
    let entry = &mut keys.keys[idx];
    entry.key_address = Some(access_key_address);
    entry.key = Some(access_key_hex);
    entry.key_authorization = Some(auth.hex);
    entry.provisioned = false;
    entry.expiry = Some(auth.expiry);
    entry.limits = auth.limits;

    keys.save()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Wallet List
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct WalletListEntry {
    address: String,
    wallet_type: String,
    networks: Vec<String>,
}

#[derive(Debug, Serialize)]
struct WalletListResponse {
    wallets: Vec<WalletListEntry>,
    total: usize,
}

async fn show_wallet_list(output_format: OutputFormat, keys: &Keystore) -> anyhow::Result<()> {
    // Group keys by wallet address (case-insensitive).
    let mut wallets: BTreeMap<String, WalletListEntry> = BTreeMap::new();
    for entry in &keys.keys {
        if entry.wallet_address.is_empty() {
            continue;
        }
        let key = entry.wallet_address.to_lowercase();
        let wallet = wallets.entry(key).or_insert_with(|| WalletListEntry {
            address: entry.wallet_address.clone(),
            wallet_type: match entry.wallet_type {
                WalletType::Passkey => "passkey".to_string(),
                WalletType::Local => "local".to_string(),
            },
            networks: Vec::new(),
        });
        if let Some(net) = NetworkId::from_chain_id(entry.chain_id) {
            let name = net.as_str().to_string();
            if !wallet.networks.contains(&name) {
                wallet.networks.push(name);
            }
        }
    }

    // Include keychain-only wallets (orphaned if keys.toml was deleted).
    if let Ok(keychain_addrs) = keychain().list() {
        for addr in keychain_addrs {
            let key = addr.to_lowercase();
            wallets.entry(key).or_insert_with(|| WalletListEntry {
                address: addr,
                wallet_type: "local".to_string(),
                networks: Vec::new(),
            });
        }
    }

    let wallets: Vec<_> = wallets.into_values().collect();
    let total = wallets.len();
    let response = WalletListResponse { wallets, total };

    match output_format {
        OutputFormat::Json | OutputFormat::Toon => {
            println!("{}", output_format.serialize(&response)?);
        }
        OutputFormat::Text => {
            if response.wallets.is_empty() {
                println!("No wallets configured.");
                return Ok(());
            }
            for wallet in &response.wallets {
                let network = wallet
                    .networks
                    .first()
                    .and_then(|n| NetworkId::resolve(Some(n)).ok())
                    .unwrap_or_default();
                let addr_link = network.address_link(&wallet.address);
                println!("{:>10}: {} ({})", "Wallet", addr_link, wallet.wallet_type);
                if !wallet.networks.is_empty() {
                    println!("{:>10}: {}", "Networks", wallet.networks.join(", "));
                }
                println!();
            }
            println!("{} wallet(s) total.", response.total);
        }
    }

    Ok(())
}
