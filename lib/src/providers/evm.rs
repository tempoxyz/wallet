use crate::config::{Config, WalletConfig};
use crate::currency::Currency;
use crate::error::{PurlError, Result};
use crate::network::{get_evm_chain_id, get_network, ChainType, Network};
use crate::payment_provider::{DryRunInfo, NetworkBalance, PaymentProvider};
use crate::x402::{PaymentPayload, PaymentRequirements};
use alloy::primitives::{Address, B256, U256};
use alloy::providers::ProviderBuilder;
use alloy::signers::{local::PrivateKeySigner, SignerSync};
use alloy::sol;
use alloy::sol_types::{eip712_domain, SolStruct};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

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

        // Create version-appropriate payload based on requirements version
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
        challenge: &crate::web::PaymentChallenge,
        config: &Config,
    ) -> Result<crate::web::PaymentCredential> {
        use crate::web::{ChargeRequest, PayloadType, PaymentCredential, PaymentPayload};
        use alloy::network::TxSigner;
        use alloy::primitives::{Bytes, U256};
        use alloy::providers::Provider;
        use alloy::sol_types::SolCall;

        // Validate method and intent
        if challenge.method != crate::web::PaymentMethod::Tempo {
            return Err(PurlError::UnsupportedPaymentMethod(format!(
                "Only 'tempo' method is supported, got: {:?}",
                challenge.method
            )));
        }
        if challenge.intent != crate::web::PaymentIntent::Charge {
            return Err(PurlError::UnsupportedPaymentIntent(format!(
                "Only 'charge' intent is supported, got: {:?}",
                challenge.intent
            )));
        }

        // Parse challenge request as ChargeRequest
        let charge_req: ChargeRequest = serde_json::from_value(challenge.request.clone())
            .map_err(|e| PurlError::InvalidChallenge(format!("Invalid charge request: {}", e)))?;

        // Load signer
        let signer = Self::load_signer(config)?;
        let from = signer.address();

        // Parse addresses
        let asset = Address::from_str(&charge_req.asset)
            .map_err(|e| PurlError::invalid_address(format!("Invalid asset address: {}", e)))?;
        let destination = Address::from_str(&charge_req.destination).map_err(|e| {
            PurlError::invalid_address(format!("Invalid destination address: {}", e))
        })?;

        // Parse amount
        let amount = U256::from_str(&charge_req.amount)
            .map_err(|e| PurlError::InvalidAmount(format!("Invalid amount: {}", e)))?;

        // Encode transfer function call
        sol! {
            function transfer(address to, uint256 amount) external returns (bool);
        }
        let transfer_call = transferCall {
            to: destination,
            amount,
        };
        let transfer_data = Bytes::from(transfer_call.abi_encode());

        // Get network info for Tempo Moderato
        let network = crate::network::get_network("tempo-moderato")
            .ok_or_else(|| PurlError::UnknownNetwork("tempo-moderato not found".to_string()))?;
        let chain_id = network.chain_id.ok_or_else(|| {
            PurlError::InvalidConfig("Tempo network missing chain ID".to_string())
        })?;

        // Create RPC provider
        let provider =
            ProviderBuilder::new().connect_http(network.rpc_url.parse().map_err(|e| {
                PurlError::InvalidConfig(format!("Invalid RPC URL for tempo-moderato: {}", e))
            })?);

        // Get nonce
        let nonce = provider
            .get_transaction_count(from)
            .await
            .map_err(|e| PurlError::signing(format!("Failed to get nonce: {}", e)))?;

        // Build Tempo transaction
        // Note: This is a simplified version. Tempo transactions have a special format
        // with multi-call support and custom fee tokens. For now, we'll create a
        // standard EIP-1559 transaction and mark it for future Tempo-specific handling.

        // TODO: Implement proper Tempo transaction format with:
        // - type: "tempo" (0x76)
        // - calls: [{to: asset, data: transfer_data}]
        // - feeToken: asset
        // For now, using standard EIP-1559 as a placeholder

        let tx = alloy::consensus::TxEip1559 {
            chain_id,
            nonce,
            gas_limit: 100_000,
            max_fee_per_gas: 10_000_000_000,         // 10 gwei
            max_priority_fee_per_gas: 1_000_000_000, // 1 gwei
            to: alloy::primitives::TxKind::Call(asset),
            value: U256::ZERO,
            access_list: Default::default(),
            input: transfer_data,
        };

        // Sign the transaction
        let mut tx_mut = tx.clone();
        let signature = signer
            .sign_transaction(&mut tx_mut)
            .await
            .map_err(|e| PurlError::signing(format!("Failed to sign transaction: {}", e)))?;

        // Encode signed transaction using RLP
        let mut buf = Vec::new();
        // EIP-1559 transaction type (0x02)
        buf.push(0x02);
        // Encode transaction with signature
        let signed = alloy::consensus::Signed::new_unchecked(tx, signature, Default::default());
        signed.rlp_encode(&mut buf);
        let signed_tx = hex::encode(&buf);

        // Create DID for source
        let did = format!("did:pkh:eip155:{}:{:#x}", chain_id, from);

        // Create payment credential
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
