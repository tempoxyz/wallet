//! Testnet faucet funding flow.

use std::time::Duration;

use alloy::providers::{Provider, ProviderBuilder};

use crate::account::{query_all_balances, TokenBalance};
use crate::cli::OutputFormat;
use crate::config::Config;
use crate::network::NetworkId;

use super::{
    has_balance_changed, poll_until, print_balance_diff, FundResponse, FAUCET_POLL_TIMEOUT_SECS,
    POLL_INTERVAL_SECS,
};

pub(super) async fn run_faucet(
    config: &Config,
    output_format: OutputFormat,
    network_id: NetworkId,
    address: &str,
    wait: bool,
) -> anyhow::Result<()> {
    let rpc_url = config.rpc_url(network_id);

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
        let addr_link = network_id.address_link(address);
        eprintln!(
            "Requested faucet funds for {addr_link} on {}.",
            network_id.as_str()
        );
    }

    // Poll for balance change
    let balances_after = if wait {
        let initial = balances_before.as_ref().unwrap();
        wait_for_balance(config, output_format, network_id, address, initial).await
    } else {
        None
    };

    if output_format.is_structured() {
        let success = balances_after
            .as_ref()
            .zip(balances_before.as_ref())
            .map(|(after, before)| has_balance_changed(before, after))
            .unwrap_or(true);

        let response = FundResponse {
            network: network_id.as_str().to_string(),
            address: address.to_string(),
            action: "faucet",
            success,
            deposit_address: None,
            source_chain: None,
            bridge_status: None,
            balances_before,
            balances_after,
        };
        println!("{}", output_format.serialize(&response)?);
    }

    Ok(())
}

/// Poll for a balance change on the target chain and print results.
async fn wait_for_balance(
    config: &Config,
    output_format: OutputFormat,
    network_id: NetworkId,
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
                    "Balance did not change within {FAUCET_POLL_TIMEOUT_SECS}s. Run 'presto whoami' to check later."
                );
            }
            Some(query_all_balances(config, network_id, address).await)
        }
    }
}

async fn poll_balance_change(
    config: &Config,
    network_id: NetworkId,
    address: &str,
    initial: &[TokenBalance],
) -> Option<Vec<TokenBalance>> {
    poll_until(
        Duration::from_secs(FAUCET_POLL_TIMEOUT_SECS),
        Duration::from_secs(POLL_INTERVAL_SECS),
        || async {
            let current = query_all_balances(config, network_id, address).await;
            has_balance_changed(initial, &current).then_some(current)
        },
    )
    .await
}
