use crate::config::{Config, WalletConfig};
use crate::error::{PurlError, Result, ResultExt, SigningContext};
use crate::network::{get_network, ChainType, GasConfig, Network};
use crate::payment::currency::Currency;
use crate::payment::money::format_u256_with_decimals;
use crate::payment::provider::{
    AddressProvider, BalanceProvider, NetworkBalance, PaymentProvider, Provider,
};
use crate::payment::providers::tempo::is_tempo_network;
use alloy::primitives::Address;
use alloy::providers::ProviderBuilder;
use alloy::signers::local::PrivateKeySigner;
use alloy::sol;
use async_trait::async_trait;
use std::str::FromStr;

const PROVIDER_NAME: &str = "EVM";

#[derive(Default)]
pub struct EvmProvider;

impl EvmProvider {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self
    }

    fn load_signer(config: &Config) -> Result<PrivateKeySigner> {
        use crate::wallet::signer::WalletSource;
        let evm_config = config.require_evm()?;
        evm_config.load_signer(None)
    }
}

impl Provider for EvmProvider {
    fn name(&self) -> &str {
        PROVIDER_NAME
    }

    fn supports_network(&self, network: &str) -> bool {
        get_network(network)
            .map(|n| n.chain_type == ChainType::Evm && !is_tempo_network(network))
            .unwrap_or(false)
    }
}

impl AddressProvider for EvmProvider {
    fn get_address(&self, config: &Config) -> Result<String> {
        config.require_evm()?.get_address()
    }
}

#[async_trait]
impl BalanceProvider for EvmProvider {
    async fn get_balance(
        &self,
        address: &str,
        network: Network,
        currency: Currency,
        config: &Config,
    ) -> Result<NetworkBalance> {
        sol! {
            #[sol(rpc)]
            interface IERC20 {
                function balanceOf(address account) external view returns (uint256);
            }
        }

        let token_config = network.usdc_config().ok_or_else(|| {
            PurlError::UnsupportedToken(format!(
                "Network {} does not support {}",
                network, currency.symbol
            ))
        })?;

        // Use config.resolve_network() to respect RPC overrides and custom networks
        let network_info = config.resolve_network(network.as_str())?;
        let provider =
            ProviderBuilder::new().connect_http(network_info.rpc_url.parse().map_err(|e| {
                PurlError::InvalidConfig(format!("Invalid RPC URL for {network}: {e}"))
            })?);

        let user_addr = Address::from_str(address)
            .map_err(|e| PurlError::invalid_address(format!("Invalid Ethereum address: {e}")))?;
        let token_addr = Address::from_str(token_config.address).map_err(|e| {
            PurlError::invalid_address(format!(
                "Invalid {} contract address for {}: {}",
                token_config.currency.symbol, network, e
            ))
        })?;

        let contract = IERC20::new(token_addr, &provider);

        let balance = contract.balanceOf(user_addr).call().await.map_err(|e| {
            PurlError::BalanceQuery(format!(
                "Failed to get {} balance for {} on {}: {}",
                token_config.currency.symbol, address, network, e
            ))
        })?;

        // Format balance directly from U256 to avoid truncation for large values
        let balance_human = format_u256_with_decimals(balance, token_config.currency.decimals);

        Ok(NetworkBalance::new(
            network,
            balance,
            balance_human,
            token_config.currency.symbol.to_string(),
        ))
    }
}

#[async_trait]
impl PaymentProvider for EvmProvider {
    async fn create_web_payment(
        &self,
        challenge: &mpay::Challenge::PaymentChallenge,
        config: &Config,
    ) -> Result<mpay::Credential::PaymentCredential> {
        use mpay::Credential::{PaymentCredential, PaymentPayload};
        use mpay::Intent::ChargeRequest;

        use crate::payment::mpay_ext::{method_to_network, ChargeRequestExt};
        use alloy::primitives::Bytes;
        use alloy::providers::Provider;
        use alloy::sol_types::SolCall;

        let charge_req: ChargeRequest = challenge
            .request
            .decode()
            .map_err(|e| PurlError::InvalidChallenge(format!("Invalid charge request: {}", e)))?;

        let signer = Self::load_signer(config)?;
        let from = signer.address();

        let currency = charge_req.currency_address()?;
        let recipient = charge_req.recipient_address()?;
        let amount = charge_req.amount_u256()?;

        sol! {
            function transfer(address to, uint256 amount) external returns (bool);
        }
        let transfer_call = transferCall {
            to: recipient,
            amount,
        };
        let transfer_data = Bytes::from(transfer_call.abi_encode());

        let network_name = method_to_network(&challenge.method).ok_or_else(|| {
            PurlError::UnsupportedPaymentMethod(format!(
                "Unsupported payment method: {}",
                challenge.method
            ))
        })?;

        let network_info = config.resolve_network(network_name)?;
        let chain_id = network_info.chain_id.ok_or_else(|| {
            PurlError::InvalidConfig(format!("{} network missing chain ID", network_name))
        })?;

        let gas_config = Network::from_str(network_name)
            .ok()
            .and_then(|n| n.gas_config())
            .unwrap_or(GasConfig::DEFAULT);

        let provider =
            ProviderBuilder::new().connect_http(network_info.rpc_url.parse().map_err(|e| {
                PurlError::InvalidConfig(format!("Invalid RPC URL for {}: {}", network_name, e))
            })?);

        let nonce = provider
            .get_transaction_count(from)
            .pending()
            .await
            .with_signing_context(SigningContext {
                network: Some(network_name.to_string()),
                address: Some(format!("{:#x}", from)),
                operation: "get_nonce",
            })?;

        let signed_tx = create_eip1559_transaction(
            &signer,
            chain_id,
            nonce,
            currency,
            transfer_data,
            &gas_config,
        )
        .await?;

        let did = format!("did:pkh:eip155:{}:{:#x}", chain_id, from);

        Ok(PaymentCredential {
            challenge: challenge.to_echo(),
            source: Some(did),
            payload: PaymentPayload::transaction(format!("0x{}", signed_tx)),
        })
    }
}

/// Create a standard EIP-1559 transaction with network-specific gas configuration.
async fn create_eip1559_transaction(
    signer: &PrivateKeySigner,
    chain_id: u64,
    nonce: u64,
    asset: Address,
    transfer_data: alloy::primitives::Bytes,
    gas_config: &GasConfig,
) -> Result<String> {
    use alloy::consensus::Signed;
    use alloy::network::TxSigner;
    use alloy::primitives::U256;

    let tx = alloy::consensus::TxEip1559 {
        chain_id,
        nonce,
        gas_limit: gas_config.gas_limit,
        max_fee_per_gas: gas_config.max_fee_per_gas_u128(),
        max_priority_fee_per_gas: gas_config.max_priority_fee_per_gas_u128(),
        to: alloy::primitives::TxKind::Call(asset),
        value: U256::ZERO,
        access_list: Default::default(),
        input: transfer_data,
    };

    let mut tx_mut = tx.clone();
    let signature = signer
        .sign_transaction(&mut tx_mut)
        .await
        .with_signing_context(SigningContext {
            network: Some(format!("chain_id:{}", chain_id)),
            address: None,
            operation: "sign_eip1559_transaction",
        })?;

    let mut buf = Vec::new();
    buf.push(0x02);
    let signed = Signed::new_unchecked(tx, signature, Default::default());
    signed.rlp_encode(&mut buf);

    Ok(hex::encode(&buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evm_provider_supports_non_tempo_evm_networks() {
        let provider = EvmProvider::new();

        assert!(provider.supports_network("ethereum"));
        assert!(provider.supports_network("base"));

        assert!(!provider.supports_network("tempo"));
        assert!(!provider.supports_network("tempo-moderato"));
        assert!(!provider.supports_network("unknown-network"));
        assert!(!provider.supports_network("nonexistent-chain"));
        assert!(!provider.supports_network("invalid"));
    }

    #[test]
    fn test_evm_provider_name() {
        let provider = EvmProvider::new();
        assert_eq!(provider.name(), "EVM");
    }

    #[test]
    fn test_evm_get_address_without_config() {
        let provider = EvmProvider::new();
        let config = Config::default();

        let address = provider.get_address(&config);
        assert!(address.is_err());
    }

    #[test]
    fn test_gas_config_defaults() {
        let gas_config = GasConfig::DEFAULT;
        assert_eq!(gas_config.gas_limit, 100_000);
        assert_eq!(gas_config.max_priority_fee_per_gas, 1_000_000_000);
        assert_eq!(gas_config.max_fee_per_gas, 10_000_000_000);

        assert_eq!(
            gas_config.max_fee_per_gas_u128(),
            gas_config.max_fee_per_gas as u128
        );
        assert_eq!(
            gas_config.max_priority_fee_per_gas_u128(),
            gas_config.max_priority_fee_per_gas as u128
        );
    }

    #[test]
    fn test_network_gas_config() {
        assert!(Network::Base.gas_config().is_some());
        assert!(Network::TempoModerato.gas_config().is_some());
        assert!(Network::Ethereum.gas_config().is_some());
    }
}
