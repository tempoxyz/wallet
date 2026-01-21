use crate::config::{Config, WalletConfig};
use crate::currency::Currency;
use crate::error::{PurlError, Result, ResultExt, SigningContext};
use crate::money::format_u256_with_decimals;
use crate::network::{get_network, ChainType, GasConfig, Network};
use crate::payment_provider::{
    AddressProvider, BalanceProvider, NetworkBalance, PaymentProvider, Provider,
};
use alloy::primitives::{Address, U256};
use alloy::providers::ProviderBuilder;
use alloy::signers::{local::PrivateKeySigner, SignerSync};
use alloy::sol;
use async_trait::async_trait;
use std::str::FromStr;

// Tempo transaction types from tempo-primitives
use tempo_primitives::transaction::{AASigned, Call, TempoTransaction};

const PROVIDER_NAME: &str = "EVM";

#[derive(Default)]
pub struct EvmProvider;

impl EvmProvider {
    pub fn new() -> Self {
        Self
    }

    fn load_signer(config: &Config) -> Result<PrivateKeySigner> {
        use crate::signer::WalletSource;
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
            .map(|n| n.chain_type == ChainType::Evm)
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
        challenge: &crate::protocol::web::PaymentChallenge,
        config: &Config,
    ) -> Result<crate::protocol::web::PaymentCredential> {
        use crate::protocol::web::{ChargeRequest, PayloadType, PaymentCredential, PaymentPayload};
        use alloy::primitives::Bytes;
        use alloy::providers::Provider;
        use alloy::sol_types::SolCall;

        let charge_req: ChargeRequest = serde_json::from_value(challenge.request.clone())
            .map_err(|e| PurlError::InvalidChallenge(format!("Invalid charge request: {}", e)))?;

        let signer = Self::load_signer(config)?;
        let from = signer.address();

        // Use typed accessor methods for type-safe parsing
        let asset = charge_req.asset_address()?;
        let destination = charge_req.destination_address()?;
        let amount = charge_req.amount_u256()?;

        sol! {
            function transfer(address to, uint256 amount) external returns (bool);
        }
        let transfer_call = transferCall {
            to: destination,
            amount,
        };
        let transfer_data = Bytes::from(transfer_call.abi_encode());

        let network_name = challenge
            .method
            .network_name()
            .ok_or_else(|| PurlError::unsupported_method(&challenge.method))?;
        // Use config.resolve_network() to respect RPC overrides and custom networks
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

        let signed_tx = match &challenge.method {
            crate::protocol::web::PaymentMethod::Tempo => create_tempo_transaction(
                &signer,
                chain_id,
                nonce,
                asset,
                transfer_data,
                &gas_config,
            )?,
            crate::protocol::web::PaymentMethod::Base => {
                create_eip1559_transaction(
                    &signer,
                    chain_id,
                    nonce,
                    asset,
                    transfer_data,
                    &gas_config,
                )
                .await?
            }
            crate::protocol::web::PaymentMethod::Custom(name) => {
                return Err(PurlError::unsupported_method(
                    &crate::protocol::web::PaymentMethod::Custom(name.clone()),
                ));
            }
        };

        let did = format!("did:pkh:eip155:{}:{:#x}", chain_id, from);

        Ok(PaymentCredential {
            id: challenge.id.clone(),
            source: Some(did),
            payload: PaymentPayload {
                payload_type: PayloadType::Transaction,
                signature: format!("0x{}", signed_tx),
            },
        })
    }
}

/// Create a Tempo transaction (type 0x76) with network-specific gas configuration.
fn create_tempo_transaction(
    signer: &PrivateKeySigner,
    chain_id: u64,
    nonce: u64,
    asset: Address,
    transfer_data: alloy::primitives::Bytes,
    gas_config: &GasConfig,
) -> Result<String> {
    use alloy::primitives::TxKind;

    let tx = TempoTransaction {
        chain_id,
        fee_token: Some(asset),
        max_priority_fee_per_gas: gas_config.max_priority_fee_per_gas_u128(),
        max_fee_per_gas: gas_config.max_fee_per_gas_u128(),
        gas_limit: gas_config.gas_limit,
        calls: vec![Call {
            to: TxKind::Call(asset),
            value: U256::ZERO,
            input: transfer_data,
        }],
        access_list: Default::default(),
        nonce_key: U256::ZERO,
        nonce,
        fee_payer_signature: None,
        valid_before: None,
        valid_after: None,
        key_authorization: None,
        tempo_authorization_list: vec![],
    };

    let signing_hash = tx.signature_hash();
    let signature = signer
        .sign_hash_sync(&signing_hash)
        .with_signing_context(SigningContext {
            network: Some(format!("chain_id:{}", chain_id)),
            address: None,
            operation: "sign_tempo_transaction",
        })?;

    let signed_tx: AASigned = tx.into_signed(signature.into());
    let mut buf = Vec::new();
    signed_tx.eip2718_encode(&mut buf);

    Ok(hex::encode(&buf))
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

    /// Test EVM private key (DO NOT use in production)
    const TEST_EVM_KEY: &str = "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";

    fn test_config_with_evm_key() -> Config {
        Config {
            evm: Some(crate::config::EvmConfig {
                keystore: None,
                private_key: Some(TEST_EVM_KEY.to_string()),
            }),
            ..Default::default()
        }
    }

    #[test]
    fn test_evm_provider_supports_evm_networks() {
        let provider = EvmProvider::new();

        assert!(provider.supports_network("base"));
        assert!(provider.supports_network("base-sepolia"));
        assert!(provider.supports_network("ethereum"));
        assert!(provider.supports_network("tempo-moderato"));

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
    fn test_evm_get_address_with_private_key() {
        let provider = EvmProvider::new();
        let config = test_config_with_evm_key();

        let address = provider.get_address(&config);
        assert!(address.is_ok());

        let addr = address.expect("Address should be valid");
        assert!(addr.starts_with("0x"));
        assert_eq!(addr.len(), 42);
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
