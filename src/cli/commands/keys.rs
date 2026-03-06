//! Key management commands — listing, cleanup, balance and spending limit queries.

use std::collections::HashMap;

use anyhow::Result;
use clap::CommandFactory;
use futures::future::join_all;

use crate::account::{
    balance_breakdown, build_key_info, format_expiry_countdown, key_expiry_timestamp,
    print_key_limits, query_all_balances, KeysResponse, TokenBalance,
};
use crate::cli::args::KeyCommands;
use crate::cli::{Cli, Context, OutputFormat};
use crate::keys::{Keystore, WalletType};
use crate::network::NetworkId;

pub(crate) async fn run(ctx: &Context, command: Option<KeyCommands>) -> Result<()> {
    let output_format = ctx.output_format;
    match command {
        Some(KeyCommands::List) => {
            show_keys(&ctx.config, output_format, ctx.network, &ctx.keys).await
        }
        Some(KeyCommands::Create { wallet }) => {
            super::wallets::create_access_key(wallet.as_deref(), &ctx.keys)?;
            if let Some(a) = ctx.analytics.as_ref() {
                a.track(
                    crate::analytics::Event::KeyCreated,
                    crate::analytics::NetworkPayload {
                        network: ctx.network.as_str().to_string(),
                    },
                );
            }
            let fresh_keys = ctx.keys.reload()?;
            super::whoami::show_whoami(&ctx.config, output_format, ctx.network, None, &fresh_keys)
                .await
        }
        Some(KeyCommands::Clean { yes }) => run_key_clean(yes),
        None => {
            if let Some(key_cmd) = Cli::command().find_subcommand_mut("keys") {
                key_cmd.print_help()?;
            } else {
                Cli::command().print_help()?;
            }
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

fn run_key_clean(yes: bool) -> anyhow::Result<()> {
    let path = Keystore::keys_path()?;

    if !path.exists() {
        eprintln!("Nothing to clean (no keys.toml found).");
        return Ok(());
    }

    if !crate::util::confirm(
        &format!("Delete all local key state at {}?", path.display()),
        yes,
    )? {
        println!("Cancelled.");
        return Ok(());
    }

    std::fs::remove_file(&path)?;
    eprintln!("Removed {}", path.display());
    Ok(())
}

async fn show_keys(
    config: &crate::config::Config,
    output_format: OutputFormat,
    network: NetworkId,
    keystore: &Keystore,
) -> anyhow::Result<()> {
    // Pre-fetch balances for each unique (wallet, network) pair.
    // Cache key includes chain_id so the same wallet on different networks
    // doesn't overwrite its sibling's balances.
    let mut balance_cache: HashMap<(String, u64), Vec<TokenBalance>> = HashMap::new();
    let mut balance_tasks = Vec::new();
    for entry in &keystore.keys {
        if entry.wallet_address.is_empty() {
            continue;
        }
        let entry_network = NetworkId::from_chain_id(entry.chain_id).unwrap_or(network);
        let addr = entry.wallet_address.clone();
        let chain_id = entry.chain_id;
        balance_tasks.push(async move {
            (
                (addr.clone(), chain_id),
                query_all_balances(config, entry_network, &addr).await,
            )
        });
    }
    for (key, balances) in join_all(balance_tasks).await {
        balance_cache.insert(key, balances);
    }

    let mut keys = Vec::new();

    for entry in &keystore.keys {
        let label = match entry.wallet_type {
            WalletType::Passkey => "passkey",
            WalletType::Local => "local",
        };
        let entry_network = NetworkId::from_chain_id(entry.chain_id).unwrap_or(network);
        let entry_chain_id = Some(entry.chain_id);
        keys.push(
            build_key_info(
                config,
                entry_network,
                entry_chain_id,
                label,
                entry,
                &balance_cache,
            )
            .await,
        );
    }

    let total = keys.len();
    let response = KeysResponse { keys, total };

    match output_format {
        OutputFormat::Json | OutputFormat::Toon => {
            println!("{}", output_format.serialize(&response)?);
        }
        OutputFormat::Text => {
            if response.keys.is_empty() {
                println!("No keys configured.");
                return Ok(());
            }
            for key in &response.keys {
                let explorer = keystore
                    .keys
                    .iter()
                    .find(|e| e.key_address.as_deref() == Some(&key.address))
                    .and_then(|e| NetworkId::from_chain_id(e.chain_id));

                if let (Some(wallet), Some(wt)) = (&key.wallet_address, &key.wallet_type) {
                    let wallet_link = explorer.unwrap_or_default().address_link(wallet);
                    println!("{:>10}: {} ({})", "Wallet", wallet_link, wt);
                }
                if let (Some(bal), Some(sym)) = (&key.balance, &key.symbol) {
                    let chain_id = keystore
                        .keys
                        .iter()
                        .find(|e| e.key_address.as_deref() == Some(&key.address))
                        .map(|e| e.chain_id);
                    if let Some(bb) = balance_breakdown(bal, sym, chain_id) {
                        let session_label = if bb.session_count == 1 {
                            "session"
                        } else {
                            "sessions"
                        };
                        println!("{:>10}: {} {}", "Balance", bb.total, sym);
                        println!(
                            "{:>10}: {} {} ({} active {})",
                            "Locked", bb.locked, sym, bb.session_count, session_label
                        );
                        println!("{:>10}: {} {}", "Available", bb.available, sym);
                    } else {
                        println!("{:>10}: {} {}", "Balance", bal, sym);
                    }
                }
                let key_link = explorer.unwrap_or_default().address_link(&key.address);
                println!("{:>10}: {}", "Key", key_link);
                if let Some(entry) = keystore
                    .keys
                    .iter()
                    .find(|e| e.key_address.as_deref() == Some(&key.address))
                {
                    if let Some(net) = NetworkId::from_chain_id(entry.chain_id) {
                        println!("{:>10}: {}", "Chain", net.as_str());
                    }
                    if let Some(expiry_ts) = key_expiry_timestamp(entry) {
                        println!("{:>10}: {}", "Expires", format_expiry_countdown(expiry_ts));
                    }
                }
                print_key_limits(key);
                println!();
            }
            println!("{} key(s) total.", response.total);
        }
    }

    Ok(())
}
