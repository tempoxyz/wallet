//! Whoami / wallet status display.

use std::collections::HashMap;
use std::io::Write;

use serde::Serialize;

use crate::account::{
    balance_breakdown, build_key_info, format_expiry_countdown, key_expiry_timestamp,
    print_key_limits_to, query_all_balances, KeyInfo,
};
use crate::cli::{Context, OutputFormat};
use tempo_common::analytics::Event;
use tempo_common::config::Config;
use tempo_common::keys::Keystore;
use tempo_common::network::NetworkId;
use tempo_common::output;
use tempo_common::util::address_link;

#[derive(Debug, Default, Serialize)]
struct StatusResponse {
    ready: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    wallet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    wallet_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    balance: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    locked: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    available: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    active_sessions: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    network: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    chain_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    key: Option<KeyInfo>,
    /// Key expiry as a Unix timestamp (used for text display only, not serialized).
    #[serde(skip)]
    key_expiry: Option<u64>,
}

pub(crate) async fn run(ctx: &Context) -> anyhow::Result<()> {
    ctx.track_event(Event::WhoamiViewed);
    show_whoami(ctx, None, None).await
}

pub(super) async fn show_whoami(
    ctx: &Context,
    keys: Option<&Keystore>,
    wallet_address: Option<&str>,
) -> anyhow::Result<()> {
    let keys = keys.unwrap_or(&ctx.keys);
    let response = build_response(&ctx.config, keys, ctx.network, wallet_address).await;
    response.render(ctx.output_format)
}

async fn build_response(
    config: &Config,
    keys: &Keystore,
    network: NetworkId,
    wallet_address: Option<&str>,
) -> StatusResponse {
    let mut response = StatusResponse {
        ready: keys.has_wallet(),
        ..Default::default()
    };

    if !keys.has_wallet() {
        return response;
    }

    let active_entry = if let Some(addr) = wallet_address {
        keys.key_for_wallet_and_network(addr, network)
    } else {
        keys.key_for_network(network)
    };

    response.network = Some(network.as_str().to_string());
    let chain_id = Some(network.chain_id());
    response.chain_id = chain_id;

    let Some(key_entry) = active_entry else {
        response.ready = false;
        return response;
    };

    if !key_entry.wallet_address.is_empty() {
        response.wallet = Some(key_entry.wallet_address.clone());
    }

    let wt = key_entry.wallet_type.as_str();
    response.wallet_type = Some(wt.to_string());

    let wallet_addr = response.wallet.as_deref().unwrap_or("");
    let balance_cache = HashMap::from([(
        (wallet_addr.to_string(), key_entry.chain_id),
        query_all_balances(config, network, wallet_addr).await,
    )]);

    let mut key_info =
        build_key_info(config, network, chain_id, wt, key_entry, &balance_cache).await;
    // whoami shows wallet/type/balance at the top level, not per-key
    key_info.wallet_address = None;
    key_info.wallet_type = None;
    response.symbol = key_info.symbol.clone();
    response.balance = key_info.balance.take();

    // Compute locked balance from active sessions
    if let Some(bb) = response
        .balance
        .as_deref()
        .zip(response.symbol.as_deref())
        .and_then(|(bal, sym)| balance_breakdown(bal, sym, response.chain_id))
    {
        response.balance = Some(bb.total);
        response.available = Some(bb.available);
        response.locked = Some(bb.locked);
        response.active_sessions = Some(bb.session_count);
    }

    if key_entry.key_address.is_none() {
        response.ready = false;
    }
    response.key = Some(key_info);
    response.key_expiry = key_expiry_timestamp(key_entry);

    // Readiness requires: key present, wallet connected, and either
    // already provisioned or has a key_authorization (will auto-provision on first use).
    response.ready = response.ready
        && response.wallet.is_some()
        && (key_entry.provisioned || key_entry.key_authorization.is_some());

    response
}

impl StatusResponse {
    fn render(&self, format: OutputFormat) -> anyhow::Result<()> {
        output::emit_by_format(format, self, || {
            let w = &mut std::io::stdout();
            let explorer = self.chain_id.and_then(NetworkId::from_chain_id);

            if self.wallet.is_none() && self.key.is_none() {
                writeln!(w, "Not logged in. Run `tempo-wallet login` to get started.")?;
                return Ok(());
            }

            if let Some(wallet) = &self.wallet {
                let wt = self.wallet_type.as_deref().unwrap_or("unknown");
                let wallet_link = address_link(explorer.unwrap_or_default(), wallet);
                writeln!(w, "{:>10}: {} ({})", "Wallet", wallet_link, wt)?;
            }

            // Wallet balance
            if let Some(bal) = &self.balance {
                let sym = self.symbol.as_deref().unwrap_or("tokens");
                writeln!(w, "{:>10}: {} {}", "Balance", bal, sym)?;
                if let (Some(locked), Some(available), Some(count)) =
                    (&self.locked, &self.available, self.active_sessions)
                {
                    let session_label = if count == 1 { "session" } else { "sessions" };
                    writeln!(
                        w,
                        "{:>10}: {} {} ({} active {})",
                        "Locked", locked, sym, count, session_label
                    )?;
                    writeln!(w, "{:>10}: {} {}", "Available", available, sym)?;
                }
            }

            if let Some(key) = &self.key {
                writeln!(w)?;
                let key_link = address_link(explorer.unwrap_or_default(), &key.address);
                writeln!(w, "{:>10}: {}", "Key", key_link)?;
                if let Some(network) = &self.network {
                    writeln!(w, "{:>10}: {}", "Chain", network)?;
                }
                if let Some(expiry_ts) = self.key_expiry {
                    writeln!(
                        w,
                        "{:>10}: {}",
                        "Expires",
                        format_expiry_countdown(expiry_ts)
                    )?;
                }
                print_key_limits_to(key, w)?;
            }

            Ok(())
        })
    }
}
