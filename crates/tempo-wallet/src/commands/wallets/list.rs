//! List configured wallets.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use serde::Serialize;

use super::keychain::keychain;
use tempo_common::cli::context::Context;
use tempo_common::cli::output;
use tempo_common::cli::terminal::{address_link, print_field_w};
use tempo_common::network::NetworkId;

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

pub(super) fn run(ctx: &Context) -> Result<()> {
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
