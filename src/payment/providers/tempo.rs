use crate::config::{Config, WalletConfig};
use crate::error::{PgetError, Result, ResultExt, SigningContext};
use crate::network::{get_network, ChainType, GasConfig, Network};
use crate::payment::currency::Currency;
use crate::payment::provider::{
    AddressProvider, BalanceProvider, NetworkBalance, PaymentProvider, Provider,
};
use alloy::primitives::{Address, U256};
use alloy::signers::{local::PrivateKeySigner, SignerSync};
use async_trait::async_trait;
use std::str::FromStr;

use tempo_primitives::transaction::{
    AASigned, Call, KeychainSignature, PrimitiveSignature, TempoSignature, TempoTransaction,
};

const PROVIDER_NAME: &str = "Tempo";

/// Check if a network name refers to a Tempo network.
pub fn is_tempo_network(name: &str) -> bool {
    matches!(name.to_lowercase().as_str(), "tempo" | "tempo-moderato")
}

#[derive(Default)]
pub struct TempoProvider;

impl TempoProvider {
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

impl Provider for TempoProvider {
    fn name(&self) -> &str {
        PROVIDER_NAME
    }

    fn supports_network(&self, network: &str) -> bool {
        get_network(network)
            .map(|n| n.chain_type == ChainType::Evm && is_tempo_network(network))
            .unwrap_or(false)
    }
}

impl AddressProvider for TempoProvider {
    fn get_address(&self, config: &Config) -> Result<String> {
        config.require_evm()?.get_address()
    }
}

#[async_trait]
impl BalanceProvider for TempoProvider {
    async fn get_balance(
        &self,
        address: &str,
        network: Network,
        currency: Currency,
        config: &Config,
    ) -> Result<NetworkBalance> {
        // Delegate to shared EVM balance logic
        super::evm::EvmProvider
            .get_balance(address, network, currency, config)
            .await
    }
}

#[async_trait]
impl PaymentProvider for TempoProvider {
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
        use alloy::sol;
        use alloy::sol_types::SolCall;

        let charge_req: ChargeRequest = challenge
            .request
            .decode()
            .map_err(|e| PgetError::InvalidChallenge(format!("Invalid charge request: {}", e)))?;

        let signer = Self::load_signer(config)?;
        let evm_config = config.require_evm()?;

        // If wallet_address is set, use keychain signing mode:
        // - from = wallet address (the passkey wallet that holds funds)
        // - signature = keychain-wrapped signature with access key's inner signature
        let wallet_address = evm_config
            .wallet_address
            .as_ref()
            .map(|addr| {
                Address::from_str(addr)
                    .map_err(|e| PgetError::InvalidConfig(format!("Invalid wallet address: {}", e)))
            })
            .transpose()?;

        // The 'from' address is the wallet address if in keychain mode, otherwise the signer address
        let from = wallet_address.unwrap_or_else(|| signer.address());

        let currency = charge_req.currency_address()?;
        let recipient = charge_req.recipient_address()?;
        let amount = charge_req.amount_u256()?;
        let memo = charge_req.memo();

        let transfer_data = if let Some(memo_bytes) = memo {
            sol! {
                function transferWithMemo(address to, uint256 amount, bytes32 memo) external returns (bool);
            }
            let call = transferWithMemoCall {
                to: recipient,
                amount,
                memo: memo_bytes.into(),
            };
            Bytes::from(call.abi_encode())
        } else {
            sol! {
                function transfer(address to, uint256 amount) external returns (bool);
            }
            let call = transferCall {
                to: recipient,
                amount,
            };
            Bytes::from(call.abi_encode())
        };

        let network_name = method_to_network(&challenge.method).ok_or_else(|| {
            PgetError::UnsupportedPaymentMethod(format!(
                "Unsupported payment method: {}",
                challenge.method
            ))
        })?;

        let network_info = config.resolve_network(network_name)?;
        let chain_id = network_info.chain_id.ok_or_else(|| {
            PgetError::InvalidConfig(format!("{} network missing chain ID", network_name))
        })?;

        let gas_config = Network::from_str(network_name)
            .ok()
            .and_then(|n| n.gas_config())
            .unwrap_or(GasConfig::DEFAULT);

        let provider = alloy::providers::ProviderBuilder::new().connect_http(
            network_info.rpc_url.parse().map_err(|e| {
                PgetError::InvalidConfig(format!("Invalid RPC URL for {}: {}", network_name, e))
            })?,
        );

        let nonce = provider
            .get_transaction_count(from)
            .pending()
            .await
            .with_signing_context(SigningContext {
                network: Some(network_name.to_string()),
                address: Some(format!("{:#x}", from)),
                operation: "get_nonce",
            })?;

        let signed_tx = create_tempo_transaction(
            &signer,
            chain_id,
            nonce,
            currency,
            transfer_data,
            &gas_config,
            wallet_address,
        )?;

        let did = format!("did:pkh:eip155:{}:{:#x}", chain_id, from);

        Ok(PaymentCredential {
            challenge: challenge.to_echo(),
            source: Some(did),
            payload: PaymentPayload::transaction(format!("0x{}", signed_tx)),
        })
    }
}

/// Create a Tempo transaction (type 0x76) with network-specific gas configuration.
///
/// If `wallet_address` is provided, uses keychain signing mode where:
/// - The private key is treated as an access key
/// - The signature is wrapped in keychain format (0x03 || wallet_address || inner_signature)
fn create_tempo_transaction(
    signer: &PrivateKeySigner,
    chain_id: u64,
    nonce: u64,
    asset: Address,
    transfer_data: alloy::primitives::Bytes,
    gas_config: &GasConfig,
    wallet_address: Option<Address>,
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
    let inner_signature =
        signer
            .sign_hash_sync(&signing_hash)
            .with_signing_context(SigningContext {
                network: Some(format!("chain_id:{}", chain_id)),
                address: None,
                operation: "sign_tempo_transaction",
            })?;

    // Build the final signature - either keychain-wrapped or direct
    let tempo_signature: TempoSignature = if let Some(wallet_addr) = wallet_address {
        // Keychain signing mode: wrap the signature with the wallet address
        // Format: 0x03 || wallet_address (20 bytes) || inner_signature (65 bytes)
        let keychain_sig =
            KeychainSignature::new(wallet_addr, PrimitiveSignature::Secp256k1(inner_signature));
        TempoSignature::Keychain(keychain_sig)
    } else {
        // Direct signing mode: use the signature as-is
        TempoSignature::Primitive(PrimitiveSignature::Secp256k1(inner_signature))
    };

    let signed_tx: AASigned = tx.into_signed(tempo_signature);
    let mut buf = Vec::new();
    signed_tx.eip2718_encode(&mut buf);

    Ok(hex::encode(&buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tempo_provider_supports_only_tempo_networks() {
        let provider = TempoProvider::new();

        assert!(provider.supports_network("tempo"));
        assert!(provider.supports_network("tempo-moderato"));

        assert!(!provider.supports_network("ethereum"));
        assert!(!provider.supports_network("base"));
        assert!(!provider.supports_network("unknown-network"));
    }

    #[test]
    fn test_tempo_provider_name() {
        let provider = TempoProvider::new();
        assert_eq!(provider.name(), "Tempo");
    }

    #[test]
    fn test_is_tempo_network() {
        assert!(is_tempo_network("tempo"));
        assert!(is_tempo_network("tempo-moderato"));
        assert!(is_tempo_network("Tempo"));
        assert!(is_tempo_network("TEMPO-MODERATO"));

        assert!(!is_tempo_network("ethereum"));
        assert!(!is_tempo_network("base"));
        assert!(!is_tempo_network("tempo-invalid"));
    }

    #[test]
    fn test_tempo_get_address_without_config() {
        let provider = TempoProvider::new();
        let config = Config::default();

        let address = provider.get_address(&config);
        assert!(address.is_err());
    }

    #[test]
    fn test_create_tempo_transaction_direct_signing() {
        use crate::network::GasConfig;

        let signer: PrivateKeySigner =
            "0x1234567890123456789012345678901234567890123456789012345678901234"
                .parse()
                .unwrap();

        let asset = Address::ZERO;
        let transfer_data = alloy::primitives::Bytes::new();

        let result = create_tempo_transaction(
            &signer,
            42431, // tempo-moderato chain id
            0,
            asset,
            transfer_data,
            &GasConfig::DEFAULT,
            None, // No wallet address = direct signing
        );

        assert!(result.is_ok());
        let tx_hex = result.unwrap();
        // Tempo tx type is 0x76, should start with 76
        assert!(tx_hex.starts_with("76"));
    }

    #[test]
    fn test_create_tempo_transaction_keychain_signing() {
        use crate::network::GasConfig;

        let signer: PrivateKeySigner =
            "0x1234567890123456789012345678901234567890123456789012345678901234"
                .parse()
                .unwrap();

        let asset = Address::ZERO;
        let transfer_data = alloy::primitives::Bytes::new();
        let wallet_address = Address::repeat_byte(0xAB);

        let result = create_tempo_transaction(
            &signer,
            42431, // tempo-moderato chain id
            0,
            asset,
            transfer_data,
            &GasConfig::DEFAULT,
            Some(wallet_address), // Keychain signing mode
        );

        assert!(result.is_ok());
        let tx_hex = result.unwrap();
        // Tempo tx type is 0x76, should start with 76
        assert!(tx_hex.starts_with("76"));
        // The transaction should be different due to keychain signature format
        // (0x03 prefix in signature instead of direct secp256k1)
    }

    #[test]
    fn test_keychain_vs_direct_signing_produces_different_tx() {
        use crate::network::GasConfig;

        let signer: PrivateKeySigner =
            "0x1234567890123456789012345678901234567890123456789012345678901234"
                .parse()
                .unwrap();

        let asset = Address::ZERO;
        let transfer_data = alloy::primitives::Bytes::new();
        let wallet_address = Address::repeat_byte(0xAB);

        let direct_tx = create_tempo_transaction(
            &signer,
            42431,
            0,
            asset,
            transfer_data.clone(),
            &GasConfig::DEFAULT,
            None,
        )
        .unwrap();

        let keychain_tx = create_tempo_transaction(
            &signer,
            42431,
            0,
            asset,
            transfer_data,
            &GasConfig::DEFAULT,
            Some(wallet_address),
        )
        .unwrap();

        // Keychain signature is longer due to the extra wallet address bytes
        assert!(keychain_tx.len() > direct_tx.len());
    }
}
