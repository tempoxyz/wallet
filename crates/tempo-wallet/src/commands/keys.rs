//! Key management commands — listing, balance and spending limit queries.

use std::collections::{BTreeSet, HashMap};

use anyhow::Result;
use futures::future::join_all;

use crate::wallet::{
    balance_breakdown, build_key_info, format_expiry_countdown, key_expiry_timestamp,
    print_key_limits, query_all_balances, KeysResponse, TokenBalance,
};
use tempo_common::cli::context::Context;
use tempo_common::cli::output;
use tempo_common::cli::terminal::{address_link, print_field_w};
use tempo_common::keys::Keystore;
use tempo_common::network::NetworkId;
use tempo_common::payment::session;

pub(crate) async fn run(ctx: &Context) -> Result<()> {
    let config = &ctx.config;
    let network = ctx.network;
    let keystore = &ctx.keys;

    // Pre-fetch balances for each unique (wallet, chain_id) pair.
    let mut seen = BTreeSet::new();
    let mut balance_tasks = Vec::new();
    for entry in keystore.iter() {
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
        .iter()
        .map(|entry| {
            let entry_network = NetworkId::from_chain_id(entry.chain_id).unwrap_or(network);
            let cache = &balance_cache;
            async move {
                build_key_info(config, entry_network, Some(entry.chain_id), entry, cache).await
            }
        })
        .collect();
    let keys = join_all(key_info_tasks).await;

    let response = KeysResponse::new(keys);
    let sessions = session::list_sessions().unwrap_or_default();

    output::emit_by_format(ctx.output_format, &response, || {
        render_keys(&response, keystore, &sessions);
        Ok(())
    })?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Text rendering
// ---------------------------------------------------------------------------

fn render_keys(response: &KeysResponse, keystore: &Keystore, sessions: &[session::SessionRecord]) {
    if response.keys.is_empty() {
        println!("No keys configured.");
        return;
    }

    for (key, entry) in response.keys.iter().zip(keystore.iter()) {
        let explorer = NetworkId::from_chain_id(entry.chain_id);

        if let Some(wallet) = &key.wallet_address {
            let wallet_link = address_link(explorer.unwrap_or_default(), wallet);
            print_field_w(10, "Wallet", &wallet_link);
        }
        if let (Some(bal), Some(sym)) = (key.balance, &key.symbol) {
            let bal_str = format!("{bal}");
            if let Some(bb) = balance_breakdown(&bal_str, sym, Some(entry.chain_id), sessions) {
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
        if let Some(pk) = &entry.key {
            print_field_w(10, "Key", pk.as_str());
        } else {
            let key_link = address_link(explorer.unwrap_or_default(), &key.address);
            print_field_w(10, "Key", &key_link);
        }
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
