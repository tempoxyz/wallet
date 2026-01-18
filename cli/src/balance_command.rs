//! Balance command for checking token wallet balances on configured networks

use anyhow::{Context, Result};
use purl_lib::currency::currencies;
use purl_lib::network::{ChainType, Network};
use purl_lib::payment_provider::NetworkBalance;
use purl_lib::{Config, PaymentMethod, PROVIDER_REGISTRY};

/// Check if mock mode is enabled for testing
fn is_mock_mode() -> bool {
    std::env::var("PURL_MOCK_NETWORK").is_ok()
}

/// Generate mock balance data for testing
fn mock_balance(
    network: Network,
    _address: &str,
    currency: &purl_lib::currency::Currency,
) -> NetworkBalance {
    // Return a realistic-looking mock balance
    let mock_atomic = match network {
        Network::Base | Network::Ethereum => "1000000", // 1 USDC
        Network::BaseSepolia | Network::EthereumSepolia => "5000000", // 5 USDC
        Network::Solana => "2500000",                   // 2.5 USDC
        Network::SolanaDevnet => "10000000",            // 10 USDC
        _ => "0",
    };

    NetworkBalance {
        network: network.to_string(),
        balance_atomic: mock_atomic.to_string(),
        balance_human: currency.format_atomic(mock_atomic.parse().unwrap_or(0)),
        asset: format!("{} (mock)", currency.symbol),
    }
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
        anyhow::bail!("No payment methods configured. Run 'purl init' to configure.");
    }

    let mock_mode = is_mock_mode();
    let mut balances = Vec::new();

    for method in available_methods {
        let chain_type = match method {
            PaymentMethod::Evm => ChainType::Evm,
            PaymentMethod::Solana => ChainType::Solana,
        };

        let networks = Network::by_chain_type(chain_type, network_filter.as_deref());

        for network in networks {
            let provider = PROVIDER_REGISTRY
                .find_provider(network.as_str())
                .context(format!("No provider found for network: {network}"))?;

            let check_address = match address.as_deref() {
                Some(addr) => addr.to_string(),
                None => provider
                    .get_address(config)
                    .context(format!("Failed to get address for {}", provider.name()))?,
            };

            if mock_mode {
                // Return mock data instead of making network calls
                balances.push(mock_balance(network, &check_address, &currency));
            } else {
                match provider
                    .get_balance(&check_address, network, currency)
                    .await
                {
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
            balance.network, balance.balance_human, balance.asset, balance.balance_atomic
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
        assert!(evm_networks.contains(&Network::Base));
        assert!(evm_networks.contains(&Network::Ethereum));

        let solana_networks = Network::by_chain_type(ChainType::Solana, None);
        assert!(!solana_networks.is_empty());
        assert!(solana_networks.contains(&Network::Solana));
        assert!(solana_networks.contains(&Network::SolanaDevnet));
    }

    #[test]
    fn test_by_chain_type_with_filter() {
        let filtered = Network::by_chain_type(ChainType::Evm, Some("base"));
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0], Network::Base);

        let filtered = Network::by_chain_type(ChainType::Solana, Some("solana-devnet"));
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0], Network::SolanaDevnet);
    }

    #[test]
    fn test_usdc_config_presence() {
        // Networks with USDC support
        assert!(Network::Base.usdc_config().is_some());
        assert!(Network::BaseSepolia.usdc_config().is_some());
        assert!(Network::Ethereum.usdc_config().is_some());
        assert!(Network::EthereumSepolia.usdc_config().is_some());
        assert!(Network::Solana.usdc_config().is_some());
        assert!(Network::SolanaDevnet.usdc_config().is_some());

        // Tempo has AlphaUSD support (testnet stablecoin)
        assert!(Network::TempoModerato.usdc_config().is_some());

        // Networks without token support yet
        assert!(Network::Avalanche.usdc_config().is_none());
        assert!(Network::Polygon.usdc_config().is_none());
    }

    #[test]
    fn test_usdc_config_structure() {
        let base_config = Network::Base.usdc_config().unwrap();
        assert!(!base_config.address.is_empty());
        assert!(base_config.address.starts_with("0x"));
        assert_eq!(base_config.currency.symbol, "USDC");
        assert_eq!(base_config.currency.decimals, 6);

        let base_info = Network::Base.info();
        assert!(!base_info.rpc_url.is_empty());

        let solana_config = Network::Solana.usdc_config().unwrap();
        assert!(!solana_config.address.is_empty());
        assert!(!solana_config.address.starts_with("0x"));
        assert_eq!(solana_config.currency.symbol, "USDC");
        assert_eq!(solana_config.currency.decimals, 6);

        let solana_info = Network::Solana.info();
        assert!(!solana_info.rpc_url.is_empty());
    }

    #[test]
    fn test_mock_balance_returns_data() {
        let usdc = currencies::USDC;

        let balance = mock_balance(Network::Base, "0x123", &usdc);
        assert_eq!(balance.network, "base");
        assert_eq!(balance.balance_atomic, "1000000");
        assert!(balance.asset.contains("mock"));

        let balance = mock_balance(Network::SolanaDevnet, "abc123", &usdc);
        assert_eq!(balance.network, "solana-devnet");
        assert_eq!(balance.balance_atomic, "10000000");
    }

    #[test]
    fn test_is_mock_mode_respects_env() {
        // This test doesn't set the env var, so should return false
        // Note: Other tests might set it, so we check the function works
        let _ = is_mock_mode(); // Just verify it doesn't panic
    }
}
