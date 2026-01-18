use crate::config::{Config, WalletConfig};
use crate::currency::Currency;
use crate::error::{PurlError, Result};
use crate::network::{get_evm_chain_id, get_network, ChainType, Network};
use crate::passkey::PasskeyConfig;
use crate::payment_provider::{DryRunInfo, NetworkBalance, PaymentProvider};
use crate::protocol::x402::{PaymentPayload, PaymentRequirements};
use alloy::primitives::{Address, B256, U256};
use alloy::providers::ProviderBuilder;
use alloy::signers::{local::PrivateKeySigner, SignerSync};
use alloy::sol;
use alloy::sol_types::{eip712_domain, SolStruct};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

// Tempo transaction types from tempo-primitives
use tempo_primitives::transaction::{AASigned, Call, TempoSignature, TempoTransaction};

use crate::network::GasConfig;

const PROVIDER_NAME: &str = "EVM";

sol! {
    #[derive(Debug, Serialize, Deserialize)]
    struct TransferWithAuthorization {
        address from;
        address to;
        uint256 value;
        uint256 validAfter;
        uint256 validBefore;
        bytes32 nonce;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvmPayload {
    pub signature: String,
    pub authorization: Authorization,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Authorization {
    pub from: String,
    pub nonce: String,
    pub to: String,
    pub valid_after: String,
    pub valid_before: String,
    pub value: String,
}

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

#[async_trait]
impl PaymentProvider for EvmProvider {
    fn supports_network(&self, network: &str) -> bool {
        get_network(network)
            .map(|n| n.chain_type == ChainType::Evm)
            .unwrap_or(false)
    }

    async fn create_payment(
        &self,
        requirements: &PaymentRequirements,
        config: &Config,
    ) -> Result<PaymentPayload> {
        let signer = Self::load_signer(config)?;

        let nonce_bytes = rand::random::<[u8; 32]>();
        let nonce = B256::from(nonce_bytes);

        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

        // Set validAfter to 10 minutes ago to account for clock skew and match
        // the official EVM client:
        // https://github.com/coinbase/x402/blob/c23d94eabec89de92b0229d7006d82097eec8b34/typescript/packages/mechanisms/evm/src/exact/client/scheme.ts#L40
        let valid_after = U256::from(now.saturating_sub(600));
        let valid_before = U256::from(now + requirements.max_timeout_seconds());

        let amount = requirements.parse_max_amount().map_err(|e| {
            PurlError::InvalidAmount(format!("Failed to parse maxAmountRequired: {e}"))
        })?;
        let value = U256::from(amount.as_atomic_units());

        let from = signer.address();
        let to = Address::from_str(requirements.pay_to()).map_err(|e| {
            PurlError::invalid_address(format!("Failed to parse payTo address: {e}"))
        })?;

        let _ = crate::constants::get_token_decimals(requirements.network(), requirements.asset())?;

        let (token_name, token_version) = requirements.evm_token_metadata().ok_or_else(|| {
            PurlError::MissingRequirement(
                "EVM payments require token name and version in extra field for EIP-712 signing"
                    .to_string(),
            )
        })?;

        let verifying_contract = Address::from_str(requirements.asset()).map_err(|e| {
            PurlError::invalid_address(format!("Failed to parse asset address: {e}"))
        })?;

        let chain_id = get_evm_chain_id(requirements.network()).ok_or_else(|| {
            PurlError::UnknownNetwork(format!(
                "Failed to get chain ID for network: {}",
                requirements.network()
            ))
        })?;

        let authorization = TransferWithAuthorization {
            from,
            to,
            value,
            validAfter: valid_after,
            validBefore: valid_before,
            nonce,
        };

        let domain = eip712_domain! {
            name: token_name,
            version: token_version,
            chain_id: chain_id,
            verifying_contract: verifying_contract,
        };

        let signing_hash = authorization.eip712_signing_hash(&domain);

        let signature = signer
            .sign_hash_sync(&signing_hash)
            .map_err(|e| PurlError::signing(format!("Failed to sign EIP-712 message: {e}")))?;

        let evm_payload = EvmPayload {
            signature: signature.to_string(),
            authorization: Authorization {
                from: from.to_checksum(None),
                nonce: format!("{nonce:#x}"),
                to: to.to_checksum(None),
                valid_after: valid_after.to_string(),
                valid_before: valid_before.to_string(),
                value: value.to_string(),
            },
        };

        let payment_payload = match requirements {
            PaymentRequirements::V1(_) => PaymentPayload::new_v1(
                requirements.scheme().to_string(),
                requirements.network().to_string(),
                serde_json::to_value(evm_payload)?,
            ),
            PaymentRequirements::V2 {
                requirements: req,
                resource_info,
            } => PaymentPayload::new_v2(
                Some(resource_info.clone()),
                req.clone(),
                serde_json::to_value(evm_payload)?,
                None,
            ),
        };

        Ok(payment_payload)
    }

    fn name(&self) -> &str {
        PROVIDER_NAME
    }

    fn dry_run(&self, requirements: &PaymentRequirements, config: &Config) -> Result<DryRunInfo> {
        let evm_config = config.require_evm()?;

        let amount = requirements
            .parse_max_amount()
            .map_err(|e| PurlError::InvalidAmount(format!("Failed to parse max amount: {e}")))?;

        Ok(DryRunInfo {
            provider: PROVIDER_NAME.to_owned(),
            network: requirements.network().to_string(),
            amount: amount.to_string(),
            asset: requirements.asset().to_string(),
            from: evm_config.get_address()?,
            to: requirements.pay_to().to_string(),
            estimated_fee: Some("0".to_string()), // EIP-3009 has no gas cost for sender
        })
    }

    fn get_address(&self, config: &Config) -> Result<String> {
        config.require_evm()?.get_address()
    }

    async fn get_balance(
        &self,
        address: &str,
        network: Network,
        currency: Currency,
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

        let network_info = network.info();
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

        let balance_atomic: u128 = balance.to_string().parse().unwrap_or(0);
        let balance_human = token_config.currency.format_atomic(balance_atomic);

        Ok(NetworkBalance {
            network: network.to_string(),
            balance_atomic: balance.to_string(),
            balance_human,
            asset: token_config.currency.symbol.to_string(),
        })
    }

    async fn create_web_payment(
        &self,
        challenge: &crate::protocol::web::PaymentChallenge,
        config: &Config,
    ) -> Result<crate::protocol::web::PaymentCredential> {
        use crate::protocol::web::{ChargeRequest, PayloadType, PaymentCredential, PaymentPayload};
        use alloy::primitives::{Bytes, U256};
        use alloy::providers::Provider;
        use alloy::sol_types::SolCall;

        let charge_req: ChargeRequest = serde_json::from_value(challenge.request.clone())
            .map_err(|e| PurlError::InvalidChallenge(format!("Invalid charge request: {}", e)))?;

        // Check if passkey config is available and configured
        let passkey_config = &config.tempo;
        let use_passkey = passkey_config.is_configured();

        // Load signer (may be None if using passkey)
        let signer = if use_passkey {
            Self::load_signer(config).ok()
        } else {
            Some(Self::load_signer(config)?)
        };

        // Determine the sender address
        let from = if use_passkey {
            Address::from_str(passkey_config.account_address.as_ref().unwrap())
                .map_err(|e| PurlError::invalid_address(format!("Invalid root account: {}", e)))?
        } else {
            signer.as_ref().unwrap().address()
        };

        let asset = Address::from_str(&charge_req.asset)
            .map_err(|e| PurlError::invalid_address(format!("Invalid asset address: {}", e)))?;
        let destination = Address::from_str(&charge_req.destination).map_err(|e| {
            PurlError::invalid_address(format!("Invalid destination address: {}", e))
        })?;

        let amount = U256::from_str(&charge_req.amount)
            .map_err(|e| PurlError::InvalidAmount(format!("Invalid amount: {}", e)))?;

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
        let network_info = crate::network::get_network(network_name)
            .ok_or_else(|| PurlError::UnknownNetwork(format!("{} not found", network_name)))?;
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
            .await
            .map_err(|e| PurlError::signing(format!("Failed to get nonce: {}", e)))?;

        let signed_tx = match &challenge.method {
            crate::protocol::web::PaymentMethod::Tempo => create_tempo_transaction(
                signer.as_ref(),
                chain_id,
                nonce,
                asset,
                transfer_data,
                &gas_config,
                Some(passkey_config),
            )?,
            crate::protocol::web::PaymentMethod::Base => {
                let s = signer.as_ref().ok_or_else(|| {
                    PurlError::ConfigMissing(
                        "EVM signer required for Base transactions".to_string(),
                    )
                })?;
                create_eip1559_transaction(s, chain_id, nonce, asset, transfer_data, &gas_config)
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

/// Create a Keychain signature for Tempo transactions using an access key.
///
/// The Keychain signature format is:
/// `0x03 || root_address (20 bytes) || r (32 bytes) || s (32 bytes) || v (1 byte)`
fn sign_with_access_key(
    tx_hash: B256,
    access_key_private_key: &str,
    root_account: Address,
) -> Result<Vec<u8>> {
    let signer = PrivateKeySigner::from_str(access_key_private_key)
        .map_err(|e| PurlError::InvalidKey(format!("Invalid access key: {}", e)))?;

    let signature = signer
        .sign_hash_sync(&tx_hash)
        .map_err(|e| PurlError::signing(format!("Failed to sign with access key: {}", e)))?;

    // Build Keychain signature: 0x03 || root_address (20) || r (32) || s (32) || v (1)
    let mut keychain_sig = Vec::with_capacity(86);
    keychain_sig.push(0x03);
    keychain_sig.extend_from_slice(root_account.as_slice());
    keychain_sig.extend_from_slice(&signature.r().to_be_bytes::<32>());
    keychain_sig.extend_from_slice(&signature.s().to_be_bytes::<32>());
    keychain_sig.push(signature.v() as u8);

    Ok(keychain_sig)
}

/// Create a Tempo transaction (type 0x76) with network-specific gas configuration.
///
/// If `passkey_config` is provided and configured with a valid (non-expired) access key,
/// the transaction will be signed using the Keychain signature format (0x03).
/// Otherwise, falls back to direct signing with the provided signer.
fn create_tempo_transaction(
    signer: Option<&PrivateKeySigner>,
    chain_id: u64,
    nonce: u64,
    asset: Address,
    transfer_data: alloy::primitives::Bytes,
    gas_config: &GasConfig,
    passkey_config: Option<&PasskeyConfig>,
) -> Result<String> {
    use alloy::primitives::TxKind;

    // Determine the sender address based on signing method
    let _sender_address = if let Some(passkey_cfg) = passkey_config {
        if passkey_cfg.is_configured() {
            let access_key = passkey_cfg
                .active_key()
                .ok_or_else(|| PurlError::ConfigMissing("No active access key".to_string()))?;

            if access_key.is_expired() {
                return Err(PurlError::ConfigMissing(
                    "Access key expired. Run `purl passkey refresh` to get a new key.".to_string(),
                ));
            }

            Address::from_str(passkey_cfg.account_address.as_ref().unwrap())
                .map_err(|e| PurlError::invalid_address(format!("Invalid root account: {}", e)))?
        } else if let Some(s) = signer {
            s.address()
        } else {
            return Err(PurlError::ConfigMissing(
                "No signer or passkey configured".to_string(),
            ));
        }
    } else if let Some(s) = signer {
        s.address()
    } else {
        return Err(PurlError::ConfigMissing(
            "No signer or passkey configured".to_string(),
        ));
    };

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

    // Sign based on available method
    if let Some(passkey_cfg) = passkey_config {
        if passkey_cfg.is_configured() {
            let access_key = passkey_cfg.active_key().ok_or_else(|| {
                PurlError::InvalidConfig("No active access key configured".into())
            })?;
            let root_account =
                Address::from_str(passkey_cfg.account_address.as_ref().ok_or_else(|| {
                    PurlError::InvalidConfig("No account address configured".into())
                })?)
                .map_err(|e| PurlError::invalid_address(format!("Invalid account address: {e}")))?;

            let signing_hash = tx.signature_hash();
            let keychain_sig_bytes =
                sign_with_access_key(signing_hash, &access_key.private_key, root_account)?;

            let tempo_sig = TempoSignature::from_bytes(&keychain_sig_bytes)
                .map_err(|e| PurlError::signing(format!("Invalid keychain signature: {}", e)))?;

            let signed_tx: AASigned = tx.into_signed(tempo_sig);
            let mut buf = Vec::new();
            signed_tx.eip2718_encode(&mut buf);

            return Ok(hex::encode(&buf));
        }
    }

    // Fall back to direct signing
    let signer = signer
        .ok_or_else(|| PurlError::ConfigMissing("No signer or passkey configured".to_string()))?;

    let signing_hash = tx.signature_hash();
    let signature = signer
        .sign_hash_sync(&signing_hash)
        .map_err(|e| PurlError::signing(format!("Failed to sign Tempo transaction: {}", e)))?;

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
        .map_err(|e| PurlError::signing(format!("Failed to sign transaction: {}", e)))?;

    let mut buf = Vec::new();
    buf.push(0x02);
    let signed = Signed::new_unchecked(tx, signature, Default::default());
    signed.rlp_encode(&mut buf);

    Ok(hex::encode(&buf))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::x402::{v1, PaymentRequirements};

    /// Test EVM private key (DO NOT use in production)
    const TEST_EVM_KEY: &str = "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";

    fn test_config_with_evm_key() -> Config {
        Config {
            evm: Some(crate::config::EvmConfig {
                keystore: None,
                private_key: Some(TEST_EVM_KEY.to_string()),
            }),
            solana: None,
            ..Default::default()
        }
    }

    fn mock_v1_requirements() -> PaymentRequirements {
        let v1_req = v1::PaymentRequirements {
            scheme: "exact".to_string(),
            network: "base".to_string(),
            max_amount_required: "1000000".to_string(),
            resource: "https://example.com/resource".to_string(),
            description: "Test payment".to_string(),
            mime_type: "application/json".to_string(),
            output_schema: None,
            pay_to: "0x1234567890123456789012345678901234567890".to_string(),
            max_timeout_seconds: 300,
            asset: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".to_string(),
            extra: Some(serde_json::json!({
                "name": "USD Coin",
                "version": "2"
            })),
        };
        PaymentRequirements::V1(v1_req)
    }

    #[test]
    fn test_evm_provider_supports_evm_networks() {
        let provider = EvmProvider::new();

        assert!(provider.supports_network("base"));
        assert!(provider.supports_network("base-sepolia"));
        assert!(provider.supports_network("ethereum"));
        assert!(provider.supports_network("tempo-moderato"));

        assert!(!provider.supports_network("solana"));
        assert!(!provider.supports_network("solana-devnet"));
        assert!(!provider.supports_network("unknown-network"));
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

        let addr = address.unwrap();
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
    fn test_evm_dry_run() {
        let provider = EvmProvider::new();
        let config = test_config_with_evm_key();
        let requirements = mock_v1_requirements();

        let result = provider.dry_run(&requirements, &config);
        assert!(result.is_ok());

        let dry_run_info = result.unwrap();
        assert_eq!(dry_run_info.provider, "EVM");
        assert_eq!(dry_run_info.network, "base");
        assert_eq!(dry_run_info.amount, "1000000");
        assert!(dry_run_info.from.starts_with("0x"));
        assert_eq!(
            dry_run_info.to,
            "0x1234567890123456789012345678901234567890"
        );
        assert_eq!(dry_run_info.estimated_fee, Some("0".to_string()));
    }

    #[test]
    fn test_evm_dry_run_without_config() {
        let provider = EvmProvider::new();
        let config = Config::default();
        let requirements = mock_v1_requirements();

        let result = provider.dry_run(&requirements, &config);
        assert!(result.is_err());
    }

    #[test]
    fn test_authorization_serialization() {
        let auth = Authorization {
            from: "0xABCDEF1234567890ABCDEF1234567890ABCDEF12".to_string(),
            nonce: "0x1234".to_string(),
            to: "0x9876543210987654321098765432109876543210".to_string(),
            valid_after: "1000".to_string(),
            valid_before: "2000".to_string(),
            value: "1000000".to_string(),
        };

        let json = serde_json::to_string(&auth).unwrap();
        assert!(json.contains("validAfter"));
        assert!(json.contains("validBefore"));
        assert!(!json.contains("valid_after"));

        let parsed: Authorization = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.from, auth.from);
        assert_eq!(parsed.value, auth.value);
    }

    #[test]
    fn test_evm_payload_structure() {
        let payload = EvmPayload {
            signature: "0xabcdef...".to_string(),
            authorization: Authorization {
                from: "0xABCDEF1234567890ABCDEF1234567890ABCDEF12".to_string(),
                nonce: "0x1234".to_string(),
                to: "0x9876543210987654321098765432109876543210".to_string(),
                valid_after: "0".to_string(),
                valid_before: "9999999999".to_string(),
                value: "1000000".to_string(),
            },
        };

        let json = serde_json::to_value(&payload).unwrap();
        assert!(json.get("signature").is_some());
        assert!(json.get("authorization").is_some());

        let auth = json.get("authorization").unwrap();
        assert!(auth.get("from").is_some());
        assert!(auth.get("to").is_some());
        assert!(auth.get("value").is_some());
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

        assert!(Network::Solana.gas_config().is_none());
        assert!(Network::SolanaDevnet.gas_config().is_none());
    }
}
