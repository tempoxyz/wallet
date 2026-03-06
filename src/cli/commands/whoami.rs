//! Whoami / wallet status display.

use serde::Serialize;

use crate::account::{
    balance_breakdown, build_key_info, format_expiry_countdown, key_expiry_timestamp,
    print_key_limits_to, query_all_balances, KeyInfo,
};
use crate::analytics::{self, Event};
use crate::cli::{Context, OutputFormat};
use crate::config::Config;
use crate::keys::{Keystore, WalletType};
use crate::network::NetworkId;

// ---------------------------------------------------------------------------
// Whoami / Status
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct StatusResponse {
    ready: bool,
    wallet: Option<String>,
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
    network: String,
    chain_id: Option<u64>,
    key: Option<KeyInfo>,
    /// Key expiry as a Unix timestamp (used for text display only, not serialized).
    #[serde(skip)]
    key_expiry: Option<u64>,
}

pub(crate) async fn run(ctx: &Context) -> anyhow::Result<()> {
    if let Some(ref a) = ctx.analytics {
        a.track(Event::WhoamiViewed, analytics::EmptyPayload);
    }
    show_whoami(&ctx.config, ctx.output_format, ctx.network, None, &ctx.keys).await
}

pub(crate) async fn show_whoami(
    config: &Config,
    output_format: OutputFormat,
    network: NetworkId,
    wallet_address: Option<&str>,
    keys: &Keystore,
) -> anyhow::Result<()> {
    let response = build_whoami_response(config, keys, network, wallet_address).await;

    match output_format {
        OutputFormat::Json | OutputFormat::Toon => {
            println!("{}", output_format.serialize(&response)?);
        }
        OutputFormat::Text => {
            print_whoami_text(&response, &mut std::io::stdout())?;
        }
    }

    Ok(())
}

async fn build_whoami_response(
    config: &Config,
    keys: &Keystore,
    network: NetworkId,
    wallet_address: Option<&str>,
) -> StatusResponse {
    let mut response = StatusResponse {
        ready: keys.has_wallet(),
        wallet: None,
        wallet_type: None,
        symbol: None,
        balance: None,
        locked: None,
        available: None,
        active_sessions: None,
        network: String::new(),
        chain_id: None,
        key: None,
        key_expiry: None,
    };

    if keys.has_wallet() {
        let active_entry = if let Some(addr) = wallet_address {
            keys.key_for_wallet_and_network(addr, network)
        } else {
            keys.key_for_network(network)
        };

        response.network = network.as_str().to_string();
        let chain_id = Some(network.chain_id());
        response.chain_id = chain_id;

        if let Some(key_entry) = active_entry {
            if !key_entry.wallet_address.is_empty() {
                response.wallet = Some(key_entry.wallet_address.clone());
            }

            let wt = match key_entry.wallet_type {
                WalletType::Passkey => "passkey",
                WalletType::Local => "local",
            };
            response.wallet_type = Some(wt.to_string());

            let key_label = match key_entry.wallet_type {
                WalletType::Passkey => "passkey".to_string(),
                WalletType::Local => "local".to_string(),
            };

            let wallet_addr = response.wallet.as_deref().unwrap_or("");
            let balance_cache = vec![(
                (wallet_addr.to_string(), key_entry.chain_id),
                query_all_balances(config, network, wallet_addr).await,
            )]
            .into_iter()
            .collect();

            let mut key_info = build_key_info(
                config,
                network,
                chain_id,
                &key_label,
                key_entry,
                &balance_cache,
            )
            .await;
            // whoami shows wallet/type/balance at the top level, not per-key
            key_info.wallet_address = None;
            key_info.wallet_type = None;
            response.symbol = key_info.symbol.clone();
            response.balance = key_info.balance.take();

            // Compute locked balance from active sessions
            if let (Some(bal_str), Some(sym)) = (response.balance.clone(), response.symbol.clone())
            {
                compute_locked_balance(&mut response, &bal_str, &sym);
            }

            if key_info.address == "none" {
                response.ready = false;
            }
            response.key = Some(key_info);
            response.key_expiry = key_expiry_timestamp(key_entry);

            // Readiness requires: key present, wallet connected, and either
            // already provisioned or has a key_authorization (will auto-provision on first use).
            let has_wallet_addr = response.wallet.as_deref().is_some_and(|s| !s.is_empty());
            let is_provisioned = key_entry.provisioned;
            let has_key_auth = key_entry.key_authorization.is_some();
            response.ready = response.ready && has_wallet_addr && (is_provisioned || has_key_auth);
        } else {
            response.wallet = None;
            response.wallet_type = None;
            response.ready = false;
        }
    } else {
        response.ready = false;
    }

    response
}

/// Show whoami output on stderr (for use during interactive login when stdout may be piped).
pub(super) async fn show_whoami_stderr(
    config: &Config,
    network: NetworkId,
    wallet_address: Option<&str>,
    keys: &Keystore,
) -> anyhow::Result<()> {
    let response = build_whoami_response(config, keys, network, wallet_address).await;
    print_whoami_text(&response, &mut std::io::stderr())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Display helpers
// ---------------------------------------------------------------------------

/// Compute the locked and total balances from active sessions.
///
/// `bal_str` is the wallet's on-chain `balanceOf` (i.e., the available amount).
/// Locked = sum of (deposit - spent) for sessions with remaining deposits.
/// Total balance = available + locked.
fn compute_locked_balance(response: &mut StatusResponse, bal_str: &str, sym: &str) {
    let bb = match balance_breakdown(bal_str, sym, response.chain_id) {
        Some(v) => v,
        None => return,
    };

    response.balance = Some(bb.total);
    response.available = Some(bb.available);
    response.locked = Some(bb.locked);
    response.active_sessions = Some(bb.session_count);
}

fn print_whoami_text(response: &StatusResponse, w: &mut dyn std::io::Write) -> anyhow::Result<()> {
    let explorer = response.chain_id.and_then(NetworkId::from_chain_id);

    if response.wallet.is_none() && response.key.is_none() {
        writeln!(w, "Not logged in. Run `tempo-wallet login` to get started.")?;
        return Ok(());
    }

    if let Some(wallet) = &response.wallet {
        let wt = response.wallet_type.as_deref().unwrap_or("unknown");
        let wallet_link = explorer.unwrap_or_default().address_link(wallet);
        writeln!(w, "{:>10}: {} ({})", "Wallet", wallet_link, wt)?;
    }

    // Wallet balance
    if let Some(bal) = &response.balance {
        let sym = response.symbol.as_deref().unwrap_or("tokens");
        writeln!(w, "{:>10}: {} {}", "Balance", bal, sym)?;
        if let (Some(locked), Some(available), Some(count)) = (
            &response.locked,
            &response.available,
            response.active_sessions,
        ) {
            let session_label = if count == 1 { "session" } else { "sessions" };
            writeln!(
                w,
                "{:>10}: {} {} ({} active {})",
                "Locked", locked, sym, count, session_label
            )?;
            writeln!(w, "{:>10}: {} {}", "Available", available, sym)?;
        }
    }

    if let Some(key) = &response.key {
        writeln!(w)?;
        let key_link = explorer.unwrap_or_default().address_link(&key.address);
        writeln!(w, "{:>10}: {}", "Key", key_link)?;
        if !response.network.is_empty() {
            writeln!(w, "{:>10}: {}", "Chain", response.network)?;
        }
        if let Some(expiry_ts) = response.key_expiry {
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
}
