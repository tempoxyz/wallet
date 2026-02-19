//! Signing context setup for Tempo transactions.

use crate::config::Config;
use crate::error::{PrestoError, Result, ResultExt, SigningContext};
use crate::network::GasConfig;
use crate::wallet::signer::load_signer_for_network;
use alloy::primitives::Address;
use alloy::signers::local::PrivateKeySigner;
use std::str::FromStr;
use tracing::debug;

use tempo_primitives::transaction::SignedKeyAuthorization;

pub(super) type HttpProvider = alloy::providers::RootProvider;

/// Common signing context shared between charge and session payment flows.
pub(super) struct SigningSetupContext {
    pub signer: PrivateKeySigner,
    pub wallet_address: Option<Address>,
    pub key_authorization: Option<SignedKeyAuthorization>,
    pub from: Address,
    pub chain_id: u64,
    pub nonce: u64,
    pub gas_config: GasConfig,
    pub provider: HttpProvider,
}

impl SigningSetupContext {
    /// Load signer, resolve network, fetch nonce, and set up all common signing context.
    pub async fn from_challenge(
        config: &Config,
        challenge: &mpp::PaymentChallenge,
    ) -> Result<Self> {
        use crate::payment::mpp_ext::network_from_charge_request;
        use alloy::providers::Provider;
        use alloy::rlp::Decodable;

        // Decode charge request and resolve network first (needed for signer loading)
        let charge_req: mpp::ChargeRequest = challenge
            .request
            .decode()
            .map_err(|e| PrestoError::InvalidConfig(format!("Invalid charge request: {}", e)))?;
        let network = network_from_charge_request(&charge_req)?;
        let network_name = network.as_str();

        // Load signer from Tempo wallet credentials for this network
        let signer_ctx = load_signer_for_network(network_name)?;
        let signer = signer_ctx.signer;
        let provisioned = signer_ctx.provisioned;

        // If wallet_address is set, use keychain signing mode
        let wallet_address = signer_ctx
            .wallet_address
            .as_ref()
            .map(|addr| {
                Address::from_str(addr).map_err(|e| {
                    PrestoError::InvalidConfig(format!("Invalid wallet address: {}", e))
                })
            })
            .transpose()?;

        // Decode key authorization from hex
        let key_authorization = signer_ctx
            .key_authorization
            .as_ref()
            .map(|hex_str| {
                let hex_str = hex_str.strip_prefix("0x").unwrap_or(hex_str);
                let bytes = hex::decode(hex_str).map_err(|e| {
                    PrestoError::InvalidConfig(format!("Invalid key authorization hex: {}", e))
                })?;
                let mut slice = bytes.as_slice();
                SignedKeyAuthorization::decode(&mut slice).map_err(|e| {
                    PrestoError::InvalidConfig(format!("Invalid key authorization RLP: {}", e))
                })
            })
            .transpose()?;

        let from = wallet_address.unwrap_or_else(|| signer.address());

        let network_info = config.resolve_network(network_name)?;
        let chain_id = network_info.chain_id.ok_or_else(|| {
            PrestoError::InvalidConfig(format!("{} network missing chain ID", network_name))
        })?;

        let gas_config = network.gas_config();

        let rpc_url: reqwest::Url = network_info.rpc_url.parse().map_err(|e| {
            PrestoError::InvalidConfig(format!("Invalid RPC URL for {}: {}", network_name, e))
        })?;
        let provider = HttpProvider::new_http(rpc_url);

        // Use confirmed nonce (not pending) so we replace any stuck transactions.
        let nonce = provider
            .get_transaction_count(from)
            .await
            .with_signing_context(SigningContext {
                network: Some(network_name.to_string()),
                address: Some(format!("{:#x}", from)),
                operation: "get_nonce",
            })?;

        // Check for stuck pending txs and bump gas aggressively to replace them.
        let pending_nonce = provider
            .get_transaction_count(from)
            .pending()
            .await
            .unwrap_or(nonce);

        let gas_config = if pending_nonce > nonce {
            debug!(
                confirmed_nonce = nonce,
                pending_nonce, "stuck pending txs detected, bumping gas to replace"
            );

            // Try to read the stuck tx's gas to bid just above it.
            // Use txpool_content to find the tx at the confirmed nonce.
            let stuck_gas = async {
                let pool: serde_json::Value = provider
                    .raw_request("txpool_content".into(), ())
                    .await
                    .ok()?;
                let from_hex = format!("{:#x}", from);
                let nonce_str = format!("{}", nonce);
                let tx = pool
                    .get("pending")?
                    .get(&from_hex)
                    .or_else(|| {
                        // txpool keys may use checksummed addresses
                        pool.get("pending")?
                            .as_object()?
                            .iter()
                            .find(|(k, _)| k.to_lowercase() == from_hex.to_lowercase())
                            .map(|(_, v)| v)
                    })?
                    .get(&nonce_str)?;
                let max_fee = u64::from_str_radix(
                    tx.get("maxFeePerGas")?.as_str()?.trim_start_matches("0x"),
                    16,
                )
                .ok()?;
                let max_priority = u64::from_str_radix(
                    tx.get("maxPriorityFeePerGas")?
                        .as_str()?
                        .trim_start_matches("0x"),
                    16,
                )
                .ok()?;
                Some((max_fee, max_priority))
            }
            .await;

            if let Some((stuck_max_fee, stuck_priority)) = stuck_gas {
                debug!(
                    stuck_max_fee,
                    stuck_priority, "found stuck tx gas, bidding 2x to replace"
                );
                GasConfig {
                    max_fee_per_gas: std::cmp::max(stuck_max_fee * 2, gas_config.max_fee_per_gas),
                    max_priority_fee_per_gas: std::cmp::max(
                        stuck_priority * 2,
                        gas_config.max_priority_fee_per_gas,
                    ),
                }
            } else {
                // Can't read stuck tx — use 10x default as safe fallback
                debug!("could not read stuck tx gas, using 10x default");
                GasConfig {
                    max_priority_fee_per_gas: gas_config.max_priority_fee_per_gas * 10,
                    max_fee_per_gas: gas_config.max_fee_per_gas * 10,
                }
            }
        } else if let Ok(latest_block) = provider.get_block_number().await {
            if let Ok(Some(block)) = provider.get_block_by_number(latest_block.into()).await {
                if let Some(base_fee) = block.header.base_fee_per_gas {
                    let min_max_fee = base_fee * 2 + gas_config.max_priority_fee_per_gas;
                    if min_max_fee > gas_config.max_fee_per_gas {
                        debug!(
                            base_fee,
                            bumped_max_fee = min_max_fee,
                            default_max_fee = gas_config.max_fee_per_gas,
                            "bumping max_fee_per_gas to cover current base fee"
                        );
                        GasConfig {
                            max_fee_per_gas: min_max_fee,
                            ..gas_config
                        }
                    } else {
                        gas_config
                    }
                } else {
                    gas_config
                }
            } else {
                gas_config
            }
        } else {
            gas_config
        };

        // Include key authorization only if not yet provisioned on this network.
        let key_authorization = if key_authorization.is_some() && !provisioned {
            key_authorization
        } else {
            None
        };

        Ok(Self {
            signer,
            wallet_address,
            key_authorization,
            from,
            chain_id,
            nonce,
            gas_config,
            provider,
        })
    }
}
