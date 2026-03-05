//! Mainnet funding — Relay bridge flow with deposit address polling.

use std::time::{Duration, Instant};

use crate::account::{query_all_balances, TokenBalance};
use crate::cli::OutputFormat;
use crate::config::Config;
use crate::network::NetworkId;

use super::relay::{
    create_deposit_address, poll_deposit_status, source_chains, DepositStatus, SourceChain,
};
use super::{has_balance_changed, print_balance_diff, FundResponse, POLL_INTERVAL_SECS};

/// Default source chain for bridging (Base).
const DEFAULT_SOURCE_CHAIN_ID: u64 = 8453;

/// Timeout for polling bridge deposit status (seconds).
const BRIDGE_POLL_TIMEOUT_SECS: u64 = 600;

pub(super) async fn run_mainnet_fund(
    config: &Config,
    output_format: OutputFormat,
    network_id: NetworkId,
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
    network_id: NetworkId,
    address: &str,
    wait: bool,
    balances_before: Option<Vec<TokenBalance>>,
) -> anyhow::Result<()> {
    let net = network_id;

    // Use Base as default source chain
    let source_chain = source_chains()
        .iter()
        .find(|c| c.chain_id == DEFAULT_SOURCE_CHAIN_ID)
        .expect("Default source chain (Base) missing from source_chains config");

    if output_format == OutputFormat::Text {
        eprintln!("Generating deposit address on {}...", source_chain.name);
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;
    let deposit = create_deposit_address(&client, source_chain, address, net.chain_id()).await?;

    if output_format == OutputFormat::Text {
        let qr_uri = format!("ethereum:{}", deposit.deposit_address);
        print_qr_code(&qr_uri);
        eprintln!();
        let deposit_link = net.address_link(&deposit.deposit_address);
        eprintln!("Send USDC on {} to: {}", source_chain.name, deposit_link);
        eprintln!("Funds will be bridged automatically to your Tempo wallet.");
        eprintln!();
    }

    if !wait {
        if output_format.is_structured() {
            let response = FundResponse {
                network: network_id.as_str().to_string(),
                address: address.to_string(),
                action: "bridge",
                success: false,
                deposit_address: Some(deposit.deposit_address),
                source_chain: Some(source_chain.name.to_string()),
                bridge_status: None,
                balances_before,
                balances_after: None,
            };
            println!("{}", output_format.serialize(&response)?);
        }
        return Ok(());
    }

    // Poll both: Relay status (source chain) and Tempo balance (target chain)
    if output_format == OutputFormat::Text {
        let deposit_link = net.address_link(&deposit.deposit_address);
        eprintln!("Deposit address ({}): {}", source_chain.name, deposit_link);
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

    if output_format.is_structured() {
        let success = final_status
            .as_ref()
            .is_some_and(|s| s.status == relay_status::SUCCESS)
            || balances_after
                .as_ref()
                .zip(balances_before.as_ref())
                .is_some_and(|(after, before)| has_balance_changed(before, after));

        let response = FundResponse {
            network: network_id.as_str().to_string(),
            address: address.to_string(),
            action: "bridge",
            success,
            deposit_address: Some(deposit.deposit_address),
            source_chain: Some(source_chain.name.to_string()),
            bridge_status: final_status,
            balances_before,
            balances_after,
        };
        println!("{}", output_format.serialize(&response)?);
    }

    Ok(())
}

struct BridgePollContext<'a> {
    config: &'a Config,
    output_format: OutputFormat,
    network_id: NetworkId,
    wallet_address: &'a str,
    source_chain: &'a SourceChain,
    request_id: &'a str,
    client: &'a reqwest::Client,
}

/// Poll both Relay status and Tempo balance concurrently.
/// Prints status transitions as they happen.
async fn poll_bridge_and_balance(
    ctx: &BridgePollContext<'_>,
    initial_balances: &[TokenBalance],
) -> Option<DepositStatus> {
    let timeout = Duration::from_secs(BRIDGE_POLL_TIMEOUT_SECS);
    let interval = Duration::from_secs(POLL_INTERVAL_SECS);
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
            poll_deposit_status(ctx.client, ctx.source_chain.relay_api, ctx.request_id),
            query_all_balances(ctx.config, ctx.network_id, ctx.wallet_address),
        );

        // Process Relay status
        match relay_result {
            Ok(Some(status)) if status.status != last_relay_status => {
                if ctx.output_format == OutputFormat::Text {
                    print_relay_status_change(ctx.source_chain.name, &status);
                }
                last_relay_status.clone_from(&status.status);

                if matches!(
                    status.status.as_str(),
                    relay_status::FAILURE | relay_status::REFUNDED | relay_status::REFUND
                ) {
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
                eprintln!("  Funds arrived on Tempo!");
                print_balance_diff(initial_balances, &current_balances);
            }
            break;
        }
    }

    final_status
}

mod relay_status {
    pub const SUCCESS: &str = "success";
    pub const FAILURE: &str = "failure";
    pub const REFUNDED: &str = "refunded";
    pub const REFUND: &str = "refund";
}

fn print_relay_status_change(source_chain: &str, status: &DepositStatus) {
    match status.status.as_str() {
        "waiting" => {
            eprintln!("  Waiting for deposit on {source_chain}...");
        }
        "pending" => {
            eprint!("  Deposit detected on {source_chain}");
            if let Some(txs) = &status.in_tx_hashes {
                if let Some(hash) = txs.first() {
                    eprint!(" (tx: {}...)", &hash[..10.min(hash.len())]);
                }
            }
            eprintln!();
        }
        "submitted" => {
            eprintln!("  Bridging to Tempo...");
        }
        "success" => {
            eprint!("  Bridge complete");
            if let Some(txs) = &status.out_tx_hashes {
                if let Some(hash) = txs.first() {
                    eprint!(" (tx: {}...)", &hash[..10.min(hash.len())]);
                }
            }
            eprintln!();
        }
        "delayed" => {
            eprintln!("  Bridge delayed, still processing...");
        }
        other => {
            eprintln!("  Bridge status: {other}");
        }
    }
}

/// Generate a QR code and display it as compact Unicode to stderr.
fn print_qr_code(data: &str) {
    let code = match qrcode::QrCode::new(data) {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!("QR code generation failed: {e}");
            return;
        }
    };

    use qrcode::render::unicode;
    let terminal_qr = code
        .render::<unicode::Dense1x2>()
        .dark_color(unicode::Dense1x2::Light)
        .light_color(unicode::Dense1x2::Dark)
        .build();
    eprintln!("{terminal_qr}");
}
