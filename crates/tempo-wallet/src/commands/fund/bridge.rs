//! Mainnet funding — Relay bridge flow with deposit address polling.

use std::time::{Duration, Instant};

use qrcode::render::unicode;

use crate::wallet::{query_all_balances, TokenBalance};
use tempo_common::{
    cli::{context::Context, output, output::OutputFormat, terminal::address_link},
    error::{NetworkError, TempoError},
};

use super::{
    has_balance_changed,
    relay::{
        create_deposit_address, poll_deposit_status, source_chains, DepositStatus, SourceChain,
    },
    render_balance_diff, FundResponse, POLL_INTERVAL_SECS,
};

/// Default source chain for bridging (Base).
const DEFAULT_SOURCE_CHAIN_ID: u64 = 8453;

/// Timeout for polling bridge deposit status (seconds).
const BRIDGE_POLL_TIMEOUT_SECS: u64 = 600;

pub(super) async fn run(ctx: &Context, address: &str, wait: bool) -> Result<(), TempoError> {
    let balances_before = query_all_balances(&ctx.config, ctx.network, address).await;

    // Use Base as default source chain
    let source_chain = source_chains()
        .iter()
        .find(|c| c.chain_id == DEFAULT_SOURCE_CHAIN_ID)
        .expect("Default source chain (Base) missing from source_chains config");

    if ctx.output_format == OutputFormat::Text {
        eprintln!("Generating deposit address on {}...", source_chain.name);
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(format!("tempo-wallet/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(NetworkError::Reqwest)?;
    let deposit =
        create_deposit_address(&client, source_chain, address, ctx.network.chain_id()).await?;

    if ctx.output_format == OutputFormat::Text {
        let qr_uri = format!("ethereum:{}", deposit.deposit_address);
        render_qr_code(&qr_uri);
        eprintln!();
        let deposit_link = address_link(ctx.network, &deposit.deposit_address);
        eprintln!("Send USDC on {} to: {}", source_chain.name, deposit_link);
        eprintln!("Funds will be bridged automatically to your Tempo wallet.");
        eprintln!();
    }

    if !wait {
        let response = FundResponse {
            network: ctx.network.as_str().to_string(),
            address: address.to_string(),
            action: "bridge",
            success: false,
            deposit_address: Some(deposit.deposit_address),
            source_chain: Some(source_chain.name.to_string()),
            bridge_status: None,
            balances_before: Some(balances_before),
            balances_after: None,
        };
        let _ = output::emit_structured_if_selected(ctx.output_format, &response)?;
        return Ok(());
    }

    // Poll both: Relay status (source chain) and Tempo balance (target chain)
    if ctx.output_format == OutputFormat::Text {
        let deposit_link = address_link(ctx.network, &deposit.deposit_address);
        eprintln!("Deposit address ({}): {}", source_chain.name, deposit_link);
        eprintln!("Watching for deposit...");
    }

    let poll_ctx = BridgePollContext {
        ctx,
        wallet_address: address,
        source_chain,
        request_id: &deposit.request_id,
        client: &client,
    };
    let final_status = poll_bridge_and_balance(&poll_ctx, &balances_before).await;

    let balances_after = query_all_balances(&ctx.config, ctx.network, address).await;

    let success = final_status
        .as_ref()
        .is_some_and(|s| s.status == relay_status::SUCCESS)
        || has_balance_changed(&balances_before, &balances_after);

    let response = FundResponse {
        network: ctx.network.as_str().to_string(),
        address: address.to_string(),
        action: "bridge",
        success,
        deposit_address: Some(deposit.deposit_address),
        source_chain: Some(source_chain.name.to_string()),
        bridge_status: final_status,
        balances_before: Some(balances_before),
        balances_after: Some(balances_after),
    };
    let _ = output::emit_structured_if_selected(ctx.output_format, &response)?;

    Ok(())
}

struct BridgePollContext<'a> {
    ctx: &'a Context,
    wallet_address: &'a str,
    source_chain: &'a SourceChain,
    request_id: &'a str,
    client: &'a reqwest::Client,
}

/// Poll both Relay status and Tempo balance concurrently.
/// Renders status transitions as they happen.
async fn poll_bridge_and_balance(
    poll: &BridgePollContext<'_>,
    initial_balances: &[TokenBalance],
) -> Option<DepositStatus> {
    let timeout = Duration::from_secs(BRIDGE_POLL_TIMEOUT_SECS);
    let interval = Duration::from_secs(POLL_INTERVAL_SECS);
    let start = Instant::now();

    let mut last_relay_status = String::new();
    let mut final_status: Option<DepositStatus> = None;

    loop {
        if start.elapsed() > timeout {
            if poll.ctx.output_format == OutputFormat::Text {
                eprintln!("Timed out after 10 minutes. Run 'tempo wallet whoami' to check later.");
            }
            break;
        }

        tokio::time::sleep(interval).await;

        // Poll Relay status and Tempo balance concurrently
        let (relay_result, current_balances) = tokio::join!(
            poll_deposit_status(poll.client, poll.source_chain.relay_api, poll.request_id),
            query_all_balances(&poll.ctx.config, poll.ctx.network, poll.wallet_address),
        );

        // Process Relay status
        match relay_result {
            Ok(Some(status)) if status.status != last_relay_status => {
                if poll.ctx.output_format == OutputFormat::Text {
                    render_relay_status(poll.source_chain.name, &status);
                }
                last_relay_status.clone_from(&status.status);

                if matches!(
                    status.status.as_str(),
                    relay_status::FAILURE | relay_status::REFUNDED | relay_status::REFUND
                ) {
                    if poll.ctx.output_format == OutputFormat::Text {
                        eprintln!(
                            "Bridge failed. Funds may be refunded on {}.",
                            poll.source_chain.name
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
            if poll.ctx.output_format == OutputFormat::Text {
                eprintln!("  Funds arrived on Tempo!");
                render_balance_diff(initial_balances, &current_balances);
            }
            break;
        }
    }

    final_status
}

mod relay_status {
    pub(super) const WAITING: &str = "waiting";
    pub(super) const PENDING: &str = "pending";
    pub(super) const SUBMITTED: &str = "submitted";
    pub(super) const SUCCESS: &str = "success";
    pub(super) const DELAYED: &str = "delayed";
    pub(super) const FAILURE: &str = "failure";
    pub(super) const REFUNDED: &str = "refunded";
    pub(super) const REFUND: &str = "refund";
}

/// Truncate a transaction hash for display (first 10 chars + "...").
fn truncate_tx_hash(hash: &str) -> &str {
    &hash[..10.min(hash.len())]
}

fn render_relay_status(source_chain: &str, status: &DepositStatus) {
    match status.status.as_str() {
        relay_status::WAITING => {
            eprintln!("  Waiting for deposit on {source_chain}...");
        }
        relay_status::PENDING => {
            eprint!("  Deposit detected on {source_chain}");
            if let Some(hash) = status.in_tx_hashes.as_deref().and_then(|t| t.first()) {
                eprint!(" (tx: {}...)", truncate_tx_hash(hash));
            }
            eprintln!();
        }
        relay_status::SUBMITTED => {
            eprintln!("  Bridging to Tempo...");
        }
        relay_status::SUCCESS => {
            eprint!("  Bridge complete");
            if let Some(hash) = status.out_tx_hashes.as_deref().and_then(|t| t.first()) {
                eprint!(" (tx: {}...)", truncate_tx_hash(hash));
            }
            eprintln!();
        }
        relay_status::DELAYED => {
            eprintln!("  Bridge delayed, still processing...");
        }
        other => {
            eprintln!("  Bridge status: {other}");
        }
    }
}

/// Generate a QR code and display it as compact Unicode to stderr.
fn render_qr_code(data: &str) {
    let code = match qrcode::QrCode::new(data) {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!("QR code generation failed: {e}");
            return;
        }
    };

    let terminal_qr = code
        .render::<unicode::Dense1x2>()
        .dark_color(unicode::Dense1x2::Light)
        .light_color(unicode::Dense1x2::Dark)
        .build();
    eprintln!("{terminal_qr}");
}
