//! Whoami / wallet status display.

use std::collections::HashMap;
use std::io::Write;

use alloy::primitives::Address;
use serde::Serialize;

use crate::analytics::WHOAMI_VIEWED;
use crate::wallet::{
    balance_breakdown, build_key_info, format_expiry_countdown, key_expiry_timestamp,
    print_key_limits_to, query_all_balances, BalanceInfo, KeyInfo,
};
use tempo_common::cli::context::Context;
use tempo_common::cli::output;
use tempo_common::cli::output::OutputFormat;
use tempo_common::cli::terminal::address_link;
use tempo_common::config::Config;
use tempo_common::error::TempoError;
use tempo_common::keys::Keystore;
use tempo_common::network::NetworkId;
use tempo_common::payment::session;

#[derive(Debug, Default, Serialize)]
struct StatusResponse {
    ready: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    wallet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    balance: Option<BalanceInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    key: Option<KeyInfo>,
    /// Key expiry as a Unix timestamp (used for text display only, not serialized).
    #[serde(skip)]
    key_expiry: Option<u64>,
}

pub(crate) async fn run(ctx: &Context) -> Result<(), TempoError> {
    ctx.track_event(WHOAMI_VIEWED);
    show_whoami(ctx, None, None).await
}

pub(super) async fn show_whoami(
    ctx: &Context,
    keys: Option<&Keystore>,
    wallet_address: Option<&str>,
) -> Result<(), TempoError> {
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
        addr.parse::<Address>()
            .ok()
            .and_then(|address| keys.key_for_wallet_address_and_network(address, network))
    } else {
        keys.key_for_network(network)
    };

    let chain_id = Some(network.chain_id());

    let Some(key_entry) = active_entry else {
        response.ready = false;
        return response;
    };

    let Some(wallet_addr) = canonical_wallet_address_hex(key_entry) else {
        response.ready = false;
        return response;
    };
    response.wallet = Some(wallet_addr.clone());

    let balance_cache = HashMap::from([(
        (wallet_addr.clone(), key_entry.chain_id),
        query_all_balances(config, network, &wallet_addr).await,
    )]);

    let mut key_info = build_key_info(config, network, chain_id, key_entry, &balance_cache).await;
    // whoami shows wallet/type/balance at the top level, not per-key
    key_info.wallet_address = None;
    key_info.wallet_type = None;

    let balance = key_info.balance.take();
    let symbol = key_info
        .symbol
        .clone()
        .unwrap_or_else(|| "tokens".to_string());

    // Compute locked balance from active sessions
    let sessions = session::list_sessions().unwrap_or_default();
    if let Some(bb) = balance
        .as_deref()
        .zip(Some(symbol.as_str()))
        .and_then(|(bal, sym)| balance_breakdown(bal, sym, chain_id, &sessions))
    {
        response.balance = Some(BalanceInfo {
            total: bb.total,
            locked: bb.locked,
            available: bb.available,
            active_sessions: bb.session_count,
            symbol: symbol.clone(),
        });
    } else if let Some(b) = balance {
        response.balance = Some(BalanceInfo {
            total: b.clone(),
            locked: "0".to_string(),
            available: b,
            active_sessions: 0,
            symbol: symbol.clone(),
        });
    }

    if key_entry.key_address_parsed().is_none() {
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

fn canonical_wallet_address_hex(key_entry: &tempo_common::keys::KeyEntry) -> Option<String> {
    key_entry.wallet_address_hex()
}

impl StatusResponse {
    fn render(&self, format: OutputFormat) -> Result<(), TempoError> {
        output::emit_by_format(format, self, || {
            let w = &mut std::io::stdout();
            let explorer = self
                .key
                .as_ref()
                .and_then(|k| k.chain_id)
                .and_then(NetworkId::from_chain_id);

            if self.wallet.is_none() && self.key.is_none() {
                writeln!(w, "Not logged in. Run `tempo wallet login` to get started.")?;
                return Ok(());
            }

            if let Some(wallet) = &self.wallet {
                let wallet_link = address_link(explorer.unwrap_or_default(), wallet);
                writeln!(w, "{:>10}: {}", "Wallet", wallet_link)?;
            }

            // Wallet balance
            if let Some(bal) = &self.balance {
                writeln!(w, "{:>10}: {} {}", "Balance", bal.total, bal.symbol)?;
                if bal.active_sessions > 0 {
                    let session_label = if bal.active_sessions == 1 {
                        "session"
                    } else {
                        "sessions"
                    };
                    writeln!(
                        w,
                        "{:>10}: {} {} ({} active {})",
                        "Locked", bal.locked, bal.symbol, bal.active_sessions, session_label
                    )?;
                    writeln!(w, "{:>10}: {} {}", "Available", bal.available, bal.symbol)?;
                }
            }

            if let Some(key) = &self.key {
                writeln!(w)?;
                let key_link = address_link(explorer.unwrap_or_default(), &key.address);
                writeln!(w, "{:>10}: {}", "Key", key_link)?;
                if let Some(network) = self.key.as_ref().and_then(|k| k.network.as_deref()) {
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
        })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempo_common::keys::KeyEntry;

    #[test]
    fn canonical_wallet_address_hex_rejects_malformed_address() {
        let entry = KeyEntry {
            wallet_address: "not-an-address".to_string(),
            ..Default::default()
        };

        assert!(canonical_wallet_address_hex(&entry).is_none());
    }
}
