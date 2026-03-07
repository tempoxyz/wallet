//! Wallet management commands — create local wallets, renew keys, and list wallets.

mod fund;
mod keychain;

use std::collections::{BTreeMap, BTreeSet};

use alloy::signers::local::PrivateKeySigner;
use anyhow::Result;
use serde::Serialize;
use zeroize::Zeroizing;

use self::keychain::keychain;
use crate::cli::args::WalletCommands;
use crate::cli::Context;
use tempo_common::analytics::{
    Event, WalletCreatedPayload, WalletFundFailurePayload, WalletFundPayload,
};
use tempo_common::error::TempoError;
use tempo_common::keys::{authorization, parse_private_key_signer, KeyEntry, Keystore, WalletType};
use tempo_common::network::NetworkId;
use tempo_common::output;
use tempo_common::util::{address_link, print_field_w, sanitize_error};

pub(crate) async fn run(ctx: &Context, command: WalletCommands) -> Result<()> {
    match command {
        WalletCommands::List => list_wallets(ctx),
        WalletCommands::Create => {
            let result = create_local_wallet(&ctx.network, &ctx.keys);
            if result.is_ok() {
                ctx.track(
                    Event::WalletCreated,
                    WalletCreatedPayload {
                        wallet_type: "local".to_string(),
                    },
                );
            }
            let wallet_addr = result?;
            let fresh_keys = ctx.keys.reload()?;
            super::whoami::show_whoami(ctx, Some(&fresh_keys), Some(&wallet_addr)).await
        }
        WalletCommands::Fund { address, no_wait } => {
            let method = match ctx.network {
                NetworkId::TempoModerato => "faucet",
                NetworkId::Tempo => "bridge",
            };
            track_fund_start(ctx, method);
            let result = fund::run(ctx, address, no_wait).await;
            track_fund_result(ctx, method, &result);
            result
        }
    }
}

fn track_fund_start(ctx: &Context, method: &str) {
    ctx.track(
        Event::WalletFundStarted,
        WalletFundPayload {
            network: ctx.network.as_str().to_string(),
            method: method.to_string(),
        },
    );
}

fn track_fund_result(ctx: &Context, method: &str, result: &Result<()>) {
    match result {
        Ok(()) => {
            ctx.track(
                Event::WalletFundSuccess,
                WalletFundPayload {
                    network: ctx.network.as_str().to_string(),
                    method: method.to_string(),
                },
            );
        }
        Err(e) => {
            ctx.track(
                Event::WalletFundFailure,
                WalletFundFailurePayload {
                    network: ctx.network.as_str().to_string(),
                    method: method.to_string(),
                    error: sanitize_error(&e.to_string()),
                },
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Wallet + key management
// ---------------------------------------------------------------------------

/// Create a local EOA wallet with a signing key.
///
/// 1. Generate random EOA key and store it in OS keychain (wallet owner key)
/// 2. Generate a random access key and store inline in keys.toml
/// 3. Sign key_authorization for the target chain
/// 4. Do not provision yet; first paid request auto-provisions
/// 5. Return the fundable wallet address
fn create_local_wallet(network: &NetworkId, keys: &Keystore) -> Result<String> {
    if keys.ephemeral {
        anyhow::bail!(TempoError::InvalidConfig(
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
        .map_err(|e| TempoError::Keychain(format!("Failed to store wallet key: {e}")))?;

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
pub(super) fn create_access_key(wallet_address: Option<&str>, keys: &Keystore) -> Result<()> {
    if keys.ephemeral {
        anyhow::bail!(TempoError::InvalidConfig(
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
                TempoError::ConfigMissing(format!("No local wallet found for address '{addr}'."))
            })?
    } else {
        let mut local_iter = keys
            .keys
            .iter()
            .enumerate()
            .filter(|(_, k)| k.wallet_type == WalletType::Local)
            .map(|(i, _)| i);

        match (local_iter.next(), local_iter.next()) {
            (None, _) => anyhow::bail!(TempoError::ConfigMissing(
                "No local wallet found.".to_string()
            )),
            (Some(i), None) => i,
            (Some(_), Some(_)) => anyhow::bail!(TempoError::InvalidConfig(
                "Multiple local wallets found. Specify --wallet <address>.".to_string()
            )),
        }
    };

    let key_entry = &keys.keys[idx];

    // Load wallet EOA key from OS keychain.
    let wallet_key_hex = keychain()
        .get(&key_entry.wallet_address)
        .map_err(|e| TempoError::Keychain(format!("Failed to load wallet key: {e}")))?
        .ok_or_else(|| {
            TempoError::Keychain(format!(
                "Wallet key not found in keychain for '{}'. The wallet may need to be re-created.",
                key_entry.wallet_address
            ))
        })?;

    let wallet_signer: PrivateKeySigner = parse_private_key_signer(&wallet_key_hex)
        .map_err(|e| TempoError::Keychain(format!("Invalid wallet key in keychain: {e}")))?;

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

impl WalletListResponse {
    fn new(wallets: Vec<WalletListEntry>) -> Self {
        let total = wallets.len();
        Self { wallets, total }
    }
}

fn list_wallets(ctx: &Context) -> Result<()> {
    // Group keys by wallet address (case-insensitive).
    let mut wallets: BTreeMap<String, (WalletListEntry, BTreeSet<String>)> = BTreeMap::new();
    for entry in &ctx.keys.keys {
        if entry.wallet_address.is_empty() {
            continue;
        }
        let key = entry.wallet_address.to_lowercase();
        let (_, networks) = wallets.entry(key).or_insert_with(|| {
            (
                WalletListEntry {
                    address: entry.wallet_address.clone(),
                    wallet_type: entry.wallet_type.as_str().to_string(),
                    networks: Vec::new(),
                },
                BTreeSet::new(),
            )
        });
        if let Some(net) = NetworkId::from_chain_id(entry.chain_id) {
            networks.insert(net.as_str().to_string());
        }
    }

    // Include keychain-only wallets (orphaned if keys.toml was deleted).
    if let Ok(keychain_addrs) = keychain().list() {
        for addr in keychain_addrs {
            let key = addr.to_lowercase();
            wallets.entry(key).or_insert_with(|| {
                (
                    WalletListEntry {
                        address: addr,
                        wallet_type: "local".to_string(),
                        networks: Vec::new(),
                    },
                    BTreeSet::new(),
                )
            });
        }
    }

    let wallets: Vec<_> = wallets
        .into_values()
        .map(|(mut entry, networks)| {
            entry.networks = networks.into_iter().collect();
            entry
        })
        .collect();
    let response = WalletListResponse::new(wallets);

    output::emit_by_format(ctx.output_format, &response, || {
        render_wallets(&response);
        Ok(())
    })?;

    Ok(())
}

fn render_wallets(response: &WalletListResponse) {
    if response.wallets.is_empty() {
        println!("No wallets configured.");
        return;
    }
    for wallet in &response.wallets {
        let network = wallet
            .networks
            .first()
            .and_then(|n| NetworkId::resolve(Some(n)).ok())
            .unwrap_or_default();
        let addr_link = address_link(network, &wallet.address);
        print_field_w(
            10,
            "Wallet",
            &format!("{addr_link} ({})", wallet.wallet_type),
        );
        if !wallet.networks.is_empty() {
            print_field_w(10, "Networks", &wallet.networks.join(", "));
        }
        println!();
    }
    println!("{} wallet(s) total.", response.wallets.len());
}
