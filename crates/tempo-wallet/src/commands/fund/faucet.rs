//! Testnet faucet funding flow.

use std::time::Duration;

use alloy::providers::{Provider, ProviderBuilder};

use crate::wallet::{query_all_balances, TokenBalance};
use tempo_common::{
    cli::{context::Context, output, output::OutputFormat, terminal::address_link},
    error::{NetworkError, TempoError},
};

use super::{
    has_balance_changed, poll_until, render_balance_diff, FundResponse, POLL_INTERVAL_SECS,
};

/// Timeout for polling faucet balance changes (seconds).
const FAUCET_POLL_TIMEOUT_SECS: u64 = 120;

pub(super) async fn run(ctx: &Context, address: &str, wait: bool) -> Result<(), TempoError> {
    let rpc_url = ctx.config.rpc_url(ctx.network);

    let balances_before = if wait {
        Some(query_all_balances(&ctx.config, ctx.network, address).await)
    } else {
        None
    };

    // Call the testnet faucet RPC method
    let provider = ProviderBuilder::new().connect_http(rpc_url);
    let result: serde_json::Value = provider
        .raw_request("tempo_fundAddress".into(), [address])
        .await
        .map_err(|source| NetworkError::RpcSource {
            operation: "request faucet funds",
            source: Box::new(source),
        })?;

    tracing::debug!("Faucet RPC response: {result}");

    if ctx.output_format == OutputFormat::Text {
        let addr_link = address_link(ctx.network, address);
        eprintln!(
            "Requested faucet funds for {addr_link} on {}.",
            ctx.network.as_str()
        );
    }

    // Poll for balance change
    let balances_after = if wait {
        let initial = balances_before.as_ref().unwrap();
        wait_for_balance(ctx, address, initial).await
    } else {
        None
    };

    let success = balances_after
        .as_ref()
        .zip(balances_before.as_ref())
        .is_none_or(|(after, before)| has_balance_changed(before, after));

    let response = FundResponse {
        network: ctx.network.as_str().to_string(),
        address: address.to_string(),
        action: "faucet",
        success,
        deposit_address: None,
        source_chain: None,
        bridge_status: None,
        balances_before,
        balances_after,
    };
    let _ = output::emit_structured_if_selected(ctx.output_format, &response)?;

    Ok(())
}

/// Poll for a balance change on the target chain and render results.
async fn wait_for_balance(
    ctx: &Context,
    address: &str,
    initial: &[TokenBalance],
) -> Option<Vec<TokenBalance>> {
    let result = poll_until(
        Duration::from_secs(FAUCET_POLL_TIMEOUT_SECS),
        Duration::from_secs(POLL_INTERVAL_SECS),
        || async {
            let current = query_all_balances(&ctx.config, ctx.network, address).await;
            has_balance_changed(initial, &current).then_some(current)
        },
    )
    .await;

    if let Some(new_balances) = result {
        if ctx.output_format == OutputFormat::Text {
            render_balance_diff(initial, &new_balances);
        }
        Some(new_balances)
    } else {
        if ctx.output_format == OutputFormat::Text {
            eprintln!(
                "Balance did not change within {FAUCET_POLL_TIMEOUT_SECS}s. Run 'tempo wallet whoami' to check later."
            );
        }
        Some(query_all_balances(&ctx.config, ctx.network, address).await)
    }
}
