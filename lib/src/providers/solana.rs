use crate::config::{Config, WalletConfig};
use crate::currency::Currency;
use crate::error::{PurlError, Result};
use crate::network::{get_network, ChainType, Network};
use crate::payment_provider::{NetworkBalance, PaymentProvider};
use async_trait::async_trait;
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

#[derive(Default)]
pub struct SolanaProvider;

impl SolanaProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl PaymentProvider for SolanaProvider {
    fn supports_network(&self, network: &str) -> bool {
        get_network(network)
            .map(|n| n.chain_type == ChainType::Solana)
            .unwrap_or(false)
    }

    fn name(&self) -> &str {
        "Solana"
    }

    fn get_address(&self, config: &Config) -> Result<String> {
        config.require_solana()?.get_address()
    }

    async fn get_balance(
        &self,
        address: &str,
        network: Network,
        currency: Currency,
    ) -> Result<NetworkBalance> {
        let token_config = network.usdc_config().ok_or_else(|| {
            PurlError::UnsupportedToken(format!(
                "Network {} does not support {}",
                network, currency.symbol
            ))
        })?;

        let network_info = network.info();
        let client = RpcClient::new(network_info.rpc_url);

        let owner_pubkey = Pubkey::from_str(address)
            .map_err(|e| PurlError::invalid_address(format!("Invalid Solana public key: {e}")))?;
        let token_mint = Pubkey::from_str(token_config.address).map_err(|e| {
            PurlError::invalid_address(format!(
                "Invalid {} mint address for {}: {}",
                token_config.currency.symbol, network, e
            ))
        })?;

        let associated_token_address =
            spl_associated_token_account::get_associated_token_address(&owner_pubkey, &token_mint);

        let balance = match client.get_token_account_balance(&associated_token_address) {
            Ok(token_balance) => token_balance.ui_amount.unwrap_or(0.0),
            Err(_) => 0.0,
        };

        let balance_atomic = (balance * token_config.currency.divisor as f64) as u64;
        let balance_human = token_config.currency.format_atomic(balance_atomic as u128);

        Ok(NetworkBalance {
            network: network.to_string(),
            balance_atomic: balance_atomic.to_string(),
            balance_human,
            asset: token_config.currency.symbol.to_string(),
        })
    }

    async fn create_web_payment(
        &self,
        _challenge: &crate::protocol::web::PaymentChallenge,
        _config: &Config,
    ) -> Result<crate::protocol::web::PaymentCredential> {
        Err(PurlError::UnsupportedPaymentMethod(
            "Web payment protocol not yet supported for Solana".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supports_network() {
        let provider = SolanaProvider::new();

        assert!(provider.supports_network("solana"));
        assert!(provider.supports_network("solana-devnet"));

        assert!(!provider.supports_network("base"));
        assert!(!provider.supports_network("base-sepolia"));
        assert!(!provider.supports_network("ethereum"));
        assert!(!provider.supports_network("devnet"));
        assert!(!provider.supports_network("unknown"));
    }

    #[test]
    fn test_provider_name() {
        let provider = SolanaProvider::new();
        assert_eq!(provider.name(), "Solana");
    }
}
