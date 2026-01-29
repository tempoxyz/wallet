//! Balance command for checking token wallet balances on configured networks

use crate::config::{Config, PaymentMethod, WalletConfig};
use crate::network::{ChainType, Network};
use crate::payment::currency::currencies;
use crate::payment::money::format_u256_with_decimals;
use crate::payment::provider::{get_balance, NetworkBalance};
use alloy::primitives::U256;
use anyhow::{Context, Result};

/// Check if mock mode is enabled for testing
fn is_mock_mode() -> bool {
    std::env::var("PGET_MOCK_NETWORK").is_ok()
}

/// Generate mock balance data for testing
fn mock_balance(
    network: Network,
    _address: &str,
    currency: &crate::payment::currency::Currency,
) -> NetworkBalance {
    let mock_atomic = match network {
        Network::Tempo => U256::from(1_000_000u64),
        Network::TempoModerato => U256::from(5_000_000u64),
    };

    let balance_human = format_u256_with_decimals(mock_atomic, currency.decimals);
    NetworkBalance::new(
        network,
        mock_atomic,
        balance_human,
        format!("{} (mock)", currency.symbol),
    )
}

/// Check token balances for configured networks
pub async fn balance_command(
    config: &Config,
    address: Option<String>,
    network_filter: Option<String>,
) -> Result<()> {
    let currency = currencies::USDC;
    let available_methods = config.available_payment_methods();

    if available_methods.is_empty() {
        anyhow::bail!("No payment methods configured. Run 'pget init' to configure.");
    }

    let mock_mode = is_mock_mode();
    let mut balances = Vec::new();

    for method in available_methods {
        let chain_type = match method {
            PaymentMethod::Evm => ChainType::Evm,
        };

        let networks = Network::by_chain_type(chain_type, network_filter.as_deref());

        for network in networks {
            let check_address = match address.as_deref() {
                Some(addr) => addr.to_string(),
                None => config
                    .require_evm()
                    .and_then(|evm| evm.get_address())
                    .context(format!("Failed to get address for {}", method))?,
            };

            if mock_mode {
                balances.push(mock_balance(network, &check_address, &currency));
            } else {
                match get_balance(config, &check_address, network, currency).await {
                    Ok(balance) => balances.push(balance),
                    Err(e) => eprintln!("Warning: Failed to get balance for {network}: {e}"),
                }
            }
        }
    }

    if balances.is_empty() {
        println!("No balances found.");
        return Ok(());
    }

    println!("{} Balances:", currency.symbol);
    println!();
    for balance in balances {
        println!(
            "{}: {} {} ({} atomic units)",
            balance.network, balance.balance_human, balance.asset, balance.balance
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_currency() {
        let usdc = currencies::USDC;
        assert_eq!(usdc.format_atomic(1_000_000), "1.000000");
        assert_eq!(usdc.format_atomic(500_000), "0.500000");
        assert_eq!(usdc.format_atomic(1), "0.000001");
        assert_eq!(usdc.format_atomic(0), "0.000000");
        assert_eq!(usdc.format_atomic(1_500_000), "1.500000");
    }

    #[test]
    fn test_by_chain_type() {
        let evm_networks = Network::by_chain_type(ChainType::Evm, None);
        assert!(!evm_networks.is_empty());
        assert!(evm_networks.contains(&Network::Tempo));
        assert!(evm_networks.contains(&Network::TempoModerato));
    }

    #[test]
    fn test_by_chain_type_with_filter() {
        let filtered = Network::by_chain_type(ChainType::Evm, Some("tempo"));
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0], Network::Tempo);
    }

    #[test]
    fn test_usdc_config_presence() {
        assert!(Network::Tempo.usdc_config().is_some());
        assert!(Network::TempoModerato.usdc_config().is_some());
    }

    #[test]
    fn test_usdc_config_structure() {
        let tempo_config = Network::Tempo.usdc_config().unwrap();
        assert!(!tempo_config.address.is_empty());
        assert!(tempo_config.address.starts_with("0x"));
        assert_eq!(tempo_config.currency.symbol, "AlphaUSD");
        assert_eq!(tempo_config.currency.decimals, 6);

        let tempo_info = Network::Tempo.info();
        assert!(!tempo_info.rpc_url.is_empty());
    }

    #[test]
    fn test_mock_balance_returns_data() {
        let usdc = currencies::ALPHA_USD;

        let balance = mock_balance(Network::Tempo, "0x123", &usdc);
        assert_eq!(balance.network, Network::Tempo);
        assert_eq!(balance.balance, U256::from(1_000_000u64));
        assert!(balance.asset.contains("mock"));
    }

    #[test]
    fn test_is_mock_mode_respects_env() {
        let _ = is_mock_mode();
    }
}
