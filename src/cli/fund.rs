//! Fund command — request testnet faucet tokens or bridge USDC to Tempo mainnet.

use std::time::{Duration, Instant};

use alloy::providers::{Provider, ProviderBuilder};
use serde::Serialize;

use super::keys::{query_all_balances, TokenBalance};
use super::relay::{self, DepositStatus};
use super::OutputFormat;
use crate::config::Config;
use crate::error::PrestoError;
use crate::network::networks::network_or_default;
use crate::network::Network;
use crate::wallet::credentials::WalletCredentials;

/// Default source chain for bridging (Base).
const DEFAULT_SOURCE_CHAIN_ID: u64 = 8453;

// ---------------------------------------------------------------------------
// JSON response
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct FundResponse {
    network: String,
    address: String,
    action: &'static str,
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    deposit_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_chain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bridge_status: Option<DepositStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    balances_before: Option<Vec<TokenBalance>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    balances_after: Option<Vec<TokenBalance>>,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub async fn run_fund(
    config: &Config,
    output_format: OutputFormat,
    network: Option<&str>,
    address: Option<String>,
    no_wait: bool,
) -> anyhow::Result<()> {
    let network_id = network_or_default(network);
    let net: Network = network_id
        .parse()
        .map_err(|_| anyhow::anyhow!("Unknown network '{network_id}'."))?;

    let wallet_address = resolve_address(address, network_id)?;

    match net {
        Network::TempoModerato => {
            run_faucet(config, output_format, network_id, &wallet_address, !no_wait).await
        }
        Network::Tempo => {
            run_mainnet_fund(config, output_format, network_id, &wallet_address, !no_wait).await
        }
    }
}

// ---------------------------------------------------------------------------
// Testnet faucet
// ---------------------------------------------------------------------------

async fn run_faucet(
    config: &Config,
    output_format: OutputFormat,
    network_id: &str,
    address: &str,
    wait: bool,
) -> anyhow::Result<()> {
    let network_info = config.resolve_network(network_id)?;
    let rpc_url: url::Url = network_info
        .rpc_url
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid RPC URL for {network_id}: {e}"))?;

    let balances_before = if wait {
        Some(query_all_balances(config, network_id, address).await)
    } else {
        None
    };

    // Call the testnet faucet RPC method
    let provider = ProviderBuilder::new().connect_http(rpc_url);
    let result: serde_json::Value = provider
        .raw_request("tempo_fundAddress".into(), [address])
        .await
        .map_err(|e| anyhow::anyhow!("Faucet request failed: {e}"))?;

    tracing::debug!("Faucet RPC response: {result}");

    if output_format == OutputFormat::Text {
        eprintln!("Requested faucet funds for {address} on {network_id}.");
    }

    // Poll for balance change
    let balances_after = if wait {
        let initial = balances_before.as_ref().unwrap();
        wait_for_balance(config, output_format, network_id, address, initial).await
    } else {
        None
    };

    if output_format == OutputFormat::Json {
        let success = balances_after
            .as_ref()
            .zip(balances_before.as_ref())
            .map(|(after, before)| has_balance_changed(before, after))
            .unwrap_or(true);

        let response = FundResponse {
            network: network_id.to_string(),
            address: address.to_string(),
            action: "faucet",
            success,
            deposit_address: None,
            source_chain: None,
            bridge_status: None,
            balances_before,
            balances_after,
        };
        println!("{}", serde_json::to_string(&response)?);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Mainnet funding — Relay bridge or direct deposit polling
// ---------------------------------------------------------------------------

async fn run_mainnet_fund(
    config: &Config,
    output_format: OutputFormat,
    network_id: &str,
    address: &str,
    wait: bool,
) -> anyhow::Result<()> {
    let balances_before = Some(query_all_balances(config, network_id, address).await);

    run_relay_bridge(
        config,
        output_format,
        network_id,
        address,
        wait,
        balances_before,
    )
    .await
}

// ---------------------------------------------------------------------------
// Relay bridge flow
// ---------------------------------------------------------------------------

async fn run_relay_bridge(
    config: &Config,
    output_format: OutputFormat,
    network_id: &str,
    address: &str,
    wait: bool,
    balances_before: Option<Vec<TokenBalance>>,
) -> anyhow::Result<()> {
    // Safe: network_id was parsed/validated in the caller (`run_fund`). If this
    // ever fails, the invariant upstream has regressed.
    let net: Network = network_id
        .parse()
        .expect("network_id should be a valid Network (validated in run_fund)");

    // Use Base as default source chain
    let source_chain = relay::source_chains()
        .iter()
        .find(|c| c.chain_id == DEFAULT_SOURCE_CHAIN_ID)
        .expect("Default source chain (Base) missing from source_chains config");

    if output_format == OutputFormat::Text {
        eprintln!("Generating deposit address on {}...", source_chain.name);
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;
    let deposit =
        relay::create_deposit_address(&client, source_chain, address, net.chain_id()).await?;

    if output_format == OutputFormat::Text {
        let qr_uri = format!("ethereum:{}", deposit.deposit_address);
        print_qr_code(&qr_uri);
        eprintln!();
        eprintln!(
            "Send USDC on {} to: {}",
            source_chain.name, deposit.deposit_address
        );
        eprintln!("Funds will be bridged automatically to your Tempo wallet.");
        eprintln!();
    }

    if !wait {
        if output_format == OutputFormat::Json {
            let response = FundResponse {
                network: network_id.to_string(),
                address: address.to_string(),
                action: "bridge",
                success: false,
                deposit_address: Some(deposit.deposit_address),
                source_chain: Some(source_chain.name.to_string()),
                bridge_status: None,
                balances_before,
                balances_after: None,
            };
            println!("{}", serde_json::to_string(&response)?);
        }
        return Ok(());
    }

    // Poll both: Relay status (source chain) and Tempo balance (target chain)
    if output_format == OutputFormat::Text {
        eprintln!(
            "Deposit address ({}): {}",
            source_chain.name, deposit.deposit_address
        );
        eprintln!("Watching for deposit...");
    }

    let ctx = BridgePollContext {
        config,
        output_format,
        network_id,
        wallet_address: address,
        source_chain,
        request_id: &deposit.request_id,
        client: &client,
    };
    let final_status =
        poll_bridge_and_balance(&ctx, balances_before.as_deref().unwrap_or(&[])).await;

    let balances_after = Some(query_all_balances(config, network_id, address).await);

    if output_format == OutputFormat::Json {
        let success = final_status.as_ref().is_some_and(|s| s.status == "success")
            || balances_after
                .as_ref()
                .zip(balances_before.as_ref())
                .is_some_and(|(after, before)| has_balance_changed(before, after));

        let response = FundResponse {
            network: network_id.to_string(),
            address: address.to_string(),
            action: "bridge",
            success,
            deposit_address: Some(deposit.deposit_address),
            source_chain: Some(source_chain.name.to_string()),
            bridge_status: final_status,
            balances_before,
            balances_after,
        };
        println!("{}", serde_json::to_string(&response)?);
    }

    Ok(())
}

struct BridgePollContext<'a> {
    config: &'a Config,
    output_format: OutputFormat,
    network_id: &'a str,
    wallet_address: &'a str,
    source_chain: &'a relay::SourceChain,
    request_id: &'a str,
    client: &'a reqwest::Client,
}

/// Poll both Relay status and Tempo balance concurrently.
/// Prints status transitions as they happen.
async fn poll_bridge_and_balance(
    ctx: &BridgePollContext<'_>,
    initial_balances: &[TokenBalance],
) -> Option<DepositStatus> {
    let timeout = Duration::from_secs(600); // 10 minutes for bridge
    let interval = Duration::from_secs(3);
    let start = Instant::now();

    let mut last_relay_status = String::new();
    let mut final_status: Option<DepositStatus> = None;

    loop {
        if start.elapsed() > timeout {
            if ctx.output_format == OutputFormat::Text {
                eprintln!("Timed out after 10 minutes. Run 'presto whoami' to check later.");
            }
            break;
        }

        tokio::time::sleep(interval).await;

        // Poll Relay status and Tempo balance concurrently
        let (relay_result, current_balances) = tokio::join!(
            relay::poll_deposit_status(ctx.client, ctx.source_chain.relay_api, ctx.request_id),
            query_all_balances(ctx.config, ctx.network_id, ctx.wallet_address),
        );

        // Process Relay status
        match relay_result {
            Ok(Some(status)) if status.status != last_relay_status => {
                if ctx.output_format == OutputFormat::Text {
                    print_relay_status_change(ctx.source_chain.name, &status);
                }
                last_relay_status.clone_from(&status.status);

                if matches!(status.status.as_str(), "failure" | "refunded" | "refund") {
                    if ctx.output_format == OutputFormat::Text {
                        eprintln!(
                            "Bridge failed. Funds may be refunded on {}.",
                            ctx.source_chain.name
                        );
                    }
                    final_status = Some(status);
                    break;
                }

                final_status = Some(status);
            }
            Err(e) => {
                tracing::warn!("Relay status check failed: {e:#}");
            }
            _ => {}
        }

        // Check for balance change on Tempo
        if has_balance_changed(initial_balances, &current_balances) {
            if ctx.output_format == OutputFormat::Text {
                eprintln!("  ✓ Funds arrived on Tempo!");
                print_balance_diff(initial_balances, &current_balances);
            }
            break;
        }
    }

    final_status
}

fn print_relay_status_change(source_chain: &str, status: &DepositStatus) {
    match status.status.as_str() {
        "waiting" => {
            eprintln!("  ⏳ Waiting for deposit on {source_chain}...");
        }
        "pending" => {
            eprint!("  ✓ Deposit detected on {source_chain}");
            if let Some(txs) = &status.in_tx_hashes {
                if let Some(hash) = txs.first() {
                    eprint!(" (tx: {}...)", &hash[..10.min(hash.len())]);
                }
            }
            eprintln!();
        }
        "submitted" => {
            eprintln!("  ↻ Bridging to Tempo...");
        }
        "success" => {
            eprint!("  ✓ Bridge complete");
            if let Some(txs) = &status.out_tx_hashes {
                if let Some(hash) = txs.first() {
                    eprint!(" (tx: {}...)", &hash[..10.min(hash.len())]);
                }
            }
            eprintln!();
        }
        "delayed" => {
            eprintln!("  ⏳ Bridge delayed — still processing...");
        }
        other => {
            eprintln!("  → Bridge status: {other}");
        }
    }
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn resolve_address(address: Option<String>, _network: &str) -> anyhow::Result<String> {
    if let Some(addr) = address {
        return Ok(addr);
    }

    let creds = WalletCredentials::load()?;
    let wallet_addr = creds.wallet_address();

    if wallet_addr.is_empty() {
        anyhow::bail!(PrestoError::ConfigMissing(
            crate::error::no_wallet_message(),
        ));
    }

    Ok(wallet_addr.to_string())
}

/// Poll for a balance change on the target chain and print results.
async fn wait_for_balance(
    config: &Config,
    output_format: OutputFormat,
    network_id: &str,
    address: &str,
    initial: &[TokenBalance],
) -> Option<Vec<TokenBalance>> {
    match poll_balance_change(config, network_id, address, initial).await {
        Some(new_balances) => {
            if output_format == OutputFormat::Text {
                print_balance_diff(initial, &new_balances);
            }
            Some(new_balances)
        }
        None => {
            if output_format == OutputFormat::Text {
                eprintln!(
                    "Balance did not change within 120s. Run 'presto whoami' to check later."
                );
            }
            Some(query_all_balances(config, network_id, address).await)
        }
    }
}

async fn poll_balance_change(
    config: &Config,
    network_id: &str,
    address: &str,
    initial: &[TokenBalance],
) -> Option<Vec<TokenBalance>> {
    let timeout = Duration::from_secs(120);
    let interval = Duration::from_secs(3);
    let start = Instant::now();

    loop {
        if start.elapsed() > timeout {
            return None;
        }
        tokio::time::sleep(interval).await;

        let current = query_all_balances(config, network_id, address).await;
        if has_balance_changed(initial, &current) {
            return Some(current);
        }
    }
}

fn has_balance_changed(initial: &[TokenBalance], current: &[TokenBalance]) -> bool {
    if current.len() != initial.len() {
        return true;
    }
    for cur in current {
        let prev = initial.iter().find(|b| b.currency == cur.currency);
        match prev {
            Some(prev) if prev.balance != cur.balance => return true,
            None => return true,
            _ => {}
        }
    }
    false
}

/// Generate a QR code and display it:
/// 1. Compact Unicode rendering to stderr (half-height, scannable in real terminals)
/// 2. PNG saved to a temp file (agents can display the image to the user)
fn print_qr_code(data: &str) {
    let code = match qrcode::QrCode::new(data) {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!("QR code generation failed: {e}");
            return;
        }
    };

    // Compact terminal QR using Unicode half-blocks (half the vertical lines)
    use qrcode::render::unicode;
    let terminal_qr = code
        .render::<unicode::Dense1x2>()
        .dark_color(unicode::Dense1x2::Light)
        .light_color(unicode::Dense1x2::Dark)
        .build();
    eprintln!("{terminal_qr}");
}

fn print_balance_diff(before: &[TokenBalance], after: &[TokenBalance]) {
    for cur in after {
        let prev = before
            .iter()
            .find(|b| b.currency == cur.currency)
            .map(|b| b.balance.as_str())
            .unwrap_or("0");
        if cur.balance != prev {
            eprintln!("  {} balance: {} → {}", cur.symbol, prev, cur.balance);
        }
    }
}
