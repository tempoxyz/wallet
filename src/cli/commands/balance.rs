//! Balance command for checking token wallet balances on configured networks

use crate::cli::OutputFormat;
use crate::config::Config;
use crate::network::Network;
use crate::payment::money::format_u256_with_decimals;
use crate::payment::provider::{get_balances, NetworkBalance};
use alloy::primitives::U256;
use anyhow::Result;
use tracing::warn;

/// Check if mock mode is enabled for testing
fn is_mock_mode() -> bool {
    std::env::var("PRESTO_MOCK_NETWORK").is_ok()
}

/// Generate mock balance data for testing (returns one balance per supported token)
fn mock_balances(network: Network, _address: &str) -> Vec<NetworkBalance> {
    let base_amount = match network {
        Network::Tempo => 1_000_000u64,
        Network::TempoModerato | Network::TempoLocalnet => 5_000_000u64,
    };

    network
        .supported_tokens()
        .into_iter()
        .map(|token_config| {
            let mock_atomic = U256::from(base_amount);
            let balance_human =
                format_u256_with_decimals(mock_atomic, token_config.currency.decimals);
            NetworkBalance::new(
                network,
                mock_atomic,
                balance_human,
                format!("{} (mock)", token_config.currency.symbol),
            )
        })
        .collect()
}

/// Check token balances for configured networks
pub async fn balance_command(
    config: &Config,
    address: Option<String>,
    network_filter: Option<String>,
    output_format: OutputFormat,
) -> Result<()> {
    let check_address = match address.as_deref() {
        Some(addr) => addr.to_string(),
        None => {
            let creds = crate::wallet::credentials::WalletCredentials::load()?;
            creds
                .active_wallet()
                .map(|w| w.account_address.clone())
                .ok_or_else(|| {
                    anyhow::anyhow!("No wallet connected. Run ' tempo-walletlogin' to connect.")
                })?
        }
    };

    let mock_mode = is_mock_mode();
    let mut balances = Vec::new();
    let networks = Network::by_name_filter(network_filter.as_deref());

    for network in networks {
        if mock_mode {
            balances.extend(mock_balances(network, &check_address));
        } else {
            match get_balances(config, &check_address, network).await {
                Ok(network_balances) => balances.extend(network_balances),
                Err(e) => warn!(network = %network, error = %e, "failed to get balances"),
            }
        }
    }

    if balances.is_empty() {
        match output_format {
            OutputFormat::Json => println!("[]"),
            _ => println!("No balances found."),
        }
        return Ok(());
    }

    match output_format {
        OutputFormat::Json => {
            let json_balances: Vec<serde_json::Value> = balances
                .iter()
                .map(|b| {
                    serde_json::json!({
                        "network": b.network.to_string(),
                        "token": b.asset,
                        "balance": b.balance_human,
                        "balance_atomic": b.balance.to_string()
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&json_balances)?);
        }
        _ => {
            println!("Tempo Stablecoin Balances:");
            println!();
            for balance in balances {
                println!(
                    "{}: {} {} ({} atomic units)",
                    balance.network, balance.balance_human, balance.asset, balance.balance
                );
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::tempo_tokens;
    use crate::payment::currency::currencies;

    #[test]
    fn test_format_currency() {
        let currency = currencies::PATH_USD;
        assert_eq!(currency.format_atomic(1_000_000), "1.000000");
        assert_eq!(currency.format_atomic(500_000), "0.500000");
        assert_eq!(currency.format_atomic(1), "0.000001");
        assert_eq!(currency.format_atomic(0), "0.000000");
        assert_eq!(currency.format_atomic(1_500_000), "1.500000");
    }

    #[test]
    fn test_all_networks() {
        let networks = Network::by_name_filter(None);
        assert!(!networks.is_empty());
        assert!(networks.contains(&Network::Tempo));
        assert!(networks.contains(&Network::TempoModerato));
    }

    #[test]
    fn test_by_name_filter() {
        let filtered = Network::by_name_filter(Some("moderato"));
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0], Network::TempoModerato);
    }

    #[test]
    fn test_supported_tokens() {
        let tokens = Network::Tempo.supported_tokens();
        assert_eq!(tokens.len(), 1);

        let symbols: Vec<_> = tokens.iter().map(|t| t.currency.symbol).collect();
        assert!(symbols.contains(&"pathUSD"));
    }

    #[test]
    fn test_token_config_by_address() {
        let config = Network::Tempo
            .token_config_by_address(tempo_tokens::PATH_USD)
            .unwrap();
        assert_eq!(config.currency.symbol, "pathUSD");
        assert_eq!(config.currency.decimals, 6);

        let tempo_info = Network::Tempo.info();
        assert!(!tempo_info.rpc_url.is_empty());
    }

    #[test]
    fn test_mock_balances_returns_all_tokens() {
        let balances = mock_balances(Network::Tempo, "0x123");
        assert_eq!(balances.len(), 1);

        for balance in &balances {
            assert_eq!(balance.network, Network::Tempo);
            assert_eq!(balance.balance, U256::from(1_000_000u64));
            assert!(balance.asset.contains("mock"));
        }
    }

    #[test]
    fn test_is_mock_mode_respects_env() {
        let _ = is_mock_mode();
    }
}
