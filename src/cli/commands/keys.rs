//! Key management commands — listing, cleanup, balance and spending limit queries.

use std::collections::{BTreeSet, HashMap};

use anyhow::Result;
use futures::future::join_all;

use crate::account::{
    balance_breakdown, build_key_info, format_expiry_countdown, key_expiry_timestamp,
    print_key_limits, query_all_balances, KeysResponse, TokenBalance,
};
use crate::analytics::Event;
use crate::cli::args::KeyCommands;
use crate::cli::{Context, OutputFormat};
use crate::keys::Keystore;
use crate::network::NetworkId;
use crate::util::print_field_w;

pub(crate) async fn run(ctx: &Context, command: Option<KeyCommands>) -> Result<()> {
    match command {
        Some(KeyCommands::List) => list_keys(ctx).await,
        Some(KeyCommands::Create { wallet }) => {
            super::wallets::create_access_key(wallet.as_deref(), &ctx.keys)?;
            if let Some(a) = ctx.analytics.as_ref() {
                a.track_event(Event::KeyCreated);
            }
            let fresh_keys = ctx.keys.reload()?;
            super::whoami::show_whoami(ctx, Some(&fresh_keys), None).await
        }
        Some(KeyCommands::Clean { yes }) => clean_keys(yes),
        None => {
            use clap::CommandFactory;
            if let Some(key_cmd) = crate::cli::Cli::command().find_subcommand_mut("keys") {
                key_cmd.print_help()?;
            } else {
                crate::cli::Cli::command().print_help()?;
            }
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

fn clean_keys(yes: bool) -> Result<()> {
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

async fn list_keys(ctx: &Context) -> Result<()> {
    let config = &ctx.config;
    let network = ctx.network;
    let keystore = &ctx.keys;

    // Pre-fetch balances for each unique (wallet, chain_id) pair.
    let mut seen = BTreeSet::new();
    let mut balance_tasks = Vec::new();
    for entry in &keystore.keys {
        if entry.wallet_address.is_empty() {
            continue;
        }
        let cache_key = (entry.wallet_address.clone(), entry.chain_id);
        if !seen.insert(cache_key) {
            continue;
        }
        let entry_network = NetworkId::from_chain_id(entry.chain_id).unwrap_or(network);
        let addr = entry.wallet_address.clone();
        let chain_id = entry.chain_id;
        balance_tasks.push(async move {
            let balances = query_all_balances(config, entry_network, &addr).await;
            ((addr, chain_id), balances)
        });
    }
    let balance_cache: HashMap<(String, u64), Vec<TokenBalance>> =
        join_all(balance_tasks).await.into_iter().collect();

    // Build key info for all entries concurrently.
    let key_info_tasks: Vec<_> = keystore
        .keys
        .iter()
        .map(|entry| {
            let entry_network = NetworkId::from_chain_id(entry.chain_id).unwrap_or(network);
            let label = entry.wallet_type.as_str();
            let cache = &balance_cache;
            async move {
                build_key_info(
                    config,
                    entry_network,
                    Some(entry.chain_id),
                    label,
                    entry,
                    cache,
                )
                .await
            }
        })
        .collect();
    let keys = join_all(key_info_tasks).await;

    let response = KeysResponse::new(keys);

    match ctx.output_format {
        OutputFormat::Json | OutputFormat::Toon => {
            println!("{}", ctx.output_format.serialize(&response)?);
        }
        OutputFormat::Text => render_keys(&response, keystore),
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Text rendering
// ---------------------------------------------------------------------------

fn render_keys(response: &KeysResponse, keystore: &Keystore) {
    if response.keys.is_empty() {
        println!("No keys configured.");
        return;
    }

    for (key, entry) in response.keys.iter().zip(keystore.keys.iter()) {
        let explorer = NetworkId::from_chain_id(entry.chain_id);

        if let (Some(wallet), Some(wt)) = (&key.wallet_address, &key.wallet_type) {
            let wallet_link = explorer.unwrap_or_default().address_link(wallet);
            print_field_w(10, "Wallet", &format!("{wallet_link} ({wt})"));
        }
        if let (Some(bal), Some(sym)) = (&key.balance, &key.symbol) {
            if let Some(bb) = balance_breakdown(bal, sym, Some(entry.chain_id)) {
                let session_label = if bb.session_count == 1 {
                    "session"
                } else {
                    "sessions"
                };
                print_field_w(10, "Balance", &format!("{} {sym}", bb.total));
                print_field_w(
                    10,
                    "Locked",
                    &format!(
                        "{} {sym} ({} active {session_label})",
                        bb.locked, bb.session_count
                    ),
                );
                print_field_w(10, "Available", &format!("{} {sym}", bb.available));
            } else {
                print_field_w(10, "Balance", &format!("{bal} {sym}"));
            }
        }
        let key_link = explorer.unwrap_or_default().address_link(&key.address);
        print_field_w(10, "Key", &key_link);
        if let Some(net) = explorer {
            print_field_w(10, "Chain", net.as_str());
        }
        if let Some(expiry_ts) = key_expiry_timestamp(entry) {
            print_field_w(10, "Expires", &format_expiry_countdown(expiry_ts));
        }
        print_key_limits(key);
        println!();
    }
    println!("{} key(s) total.", response.keys.len());
}
