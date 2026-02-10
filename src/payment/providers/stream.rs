//! Streaming payment support for tempoctl.
//!
//! Handles the lifecycle of payment channels: opening channels on first use
//! and issuing cumulative vouchers on subsequent requests.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use alloy::primitives::{keccak256, Address, B256, U256};
use alloy::signers::local::PrivateKeySigner;
use alloy::signers::SignerSync;
use alloy::sol_types::SolValue;
use serde::{Deserialize, Serialize};

use tracing::warn;

use crate::error::{Result, TempoCtlError};

/// Default escrow contract address (same on mainnet and moderato).
const DEFAULT_ESCROW: &str = "0x9d136eEa063eDE5418A6BC7bEafF009bBb6CFa70";

/// Stream channel state file name.
const STREAM_STATE_FILE: &str = "stream_channels.json";

/// Default deposit in atomic units (10 tokens with 6 decimals = 10_000_000).
const DEFAULT_DEPOSIT_ATOMIC: u128 = 10_000_000;

/// Persisted stream channel entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelEntry {
    pub channel_id: String,
    pub salt: String,
    pub deposit: String,
    pub cumulative_amount: String,
    pub payer: String,
    pub payee: String,
    pub token: String,
    pub escrow_contract: String,
    pub chain_id: u64,
    pub authorized_signer: String,
    /// Unix timestamp when close was requested on-chain (0 = not requested).
    #[serde(default)]
    pub close_requested_at: u64,
}

/// Stream channel state store (backed by a JSON file).
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct StreamState {
    pub channels: HashMap<String, ChannelEntry>,
}

impl StreamState {
    /// Load state from the default location.
    ///
    /// If the file exists but contains corrupt JSON, it is renamed to `.bak`
    /// and a default (empty) state is returned, enabling the on-chain recovery path.
    pub fn load() -> Result<Self> {
        let path = Self::state_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let data = fs::read_to_string(&path).map_err(|e| {
            TempoCtlError::InvalidConfig(format!("Failed to read stream state: {}", e))
        })?;
        match serde_json::from_str(&data) {
            Ok(state) => Ok(state),
            Err(e) => {
                let backup = path.with_extension("json.bak");
                warn!(
                    error = %e,
                    backup = %backup.display(),
                    "stream state file is corrupt, renaming to .bak and starting fresh"
                );
                let _ = fs::rename(&path, &backup);
                Ok(Self::default())
            }
        }
    }

    /// Save state to the default location.
    pub fn save(&self) -> Result<()> {
        let path = Self::state_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                TempoCtlError::InvalidConfig(format!("Failed to create state dir: {}", e))
            })?;
        }
        let data = serde_json::to_string_pretty(self).map_err(|e| {
            TempoCtlError::InvalidConfig(format!("Failed to serialize stream state: {}", e))
        })?;
        fs::write(&path, data).map_err(|e| {
            TempoCtlError::InvalidConfig(format!("Failed to write stream state: {}", e))
        })?;
        Ok(())
    }

    /// Get the state file path.
    fn state_path() -> Result<PathBuf> {
        crate::util::constants::tempoctl_data_dir()
            .map(|d| d.join(STREAM_STATE_FILE))
            .ok_or(TempoCtlError::NoConfigDir)
    }

    /// Generate a channel key from parameters.
    pub fn channel_key(
        payer: &Address,
        payee: &Address,
        token: &Address,
        escrow: &Address,
        chain_id: u64,
    ) -> String {
        format!(
            "{:#x}:{:#x}:{:#x}:{:#x}:{}",
            payer, payee, token, escrow, chain_id
        )
    }
}

/// Compute a channel ID matching the on-chain `computeChannelId`.
///
/// `keccak256(abi.encode(payer, payee, token, deposit(uint128), salt(bytes32), authorizedSigner, escrowContract, chainId(uint256)))`
#[allow(clippy::too_many_arguments)]
pub fn compute_channel_id(
    payer: Address,
    payee: Address,
    token: Address,
    deposit: u128,
    salt: B256,
    authorized_signer: Address,
    escrow_contract: Address,
    chain_id: u64,
) -> B256 {
    let encoded = (
        payer,
        payee,
        token,
        deposit,
        salt,
        authorized_signer,
        escrow_contract,
        U256::from(chain_id),
    )
        .abi_encode();
    keccak256(&encoded)
}

/// Sign an EIP-712 voucher for a stream payment channel.
///
/// Domain: name="Tempo Stream Channel", version="1", chainId, verifyingContract=escrowContract
/// Types: Voucher(bytes32 channelId, uint128 cumulativeAmount)
pub fn sign_voucher(
    signer: &PrivateKeySigner,
    channel_id: B256,
    cumulative_amount: u128,
    escrow_contract: Address,
    chain_id: u64,
) -> Result<String> {
    use alloy::sol;
    use alloy::sol_types::eip712_domain;

    sol! {
        #[derive(Default)]
        struct Voucher {
            bytes32 channelId;
            uint128 cumulativeAmount;
        }
    }

    let domain = eip712_domain! {
        name: "Tempo Stream Channel",
        version: "1",
        chain_id: chain_id,
        verifying_contract: escrow_contract,
    };

    let voucher = Voucher {
        channelId: channel_id,
        cumulativeAmount: cumulative_amount,
    };

    use alloy::sol_types::SolStruct;
    let signing_hash = voucher.eip712_signing_hash(&domain);

    let sig = signer
        .sign_hash_sync(&signing_hash)
        .map_err(|e| TempoCtlError::SigningFailed(format!("Failed to sign voucher: {}", e)))?;

    Ok(format!("0x{}", hex::encode(sig.as_bytes())))
}

/// Resolve the escrow contract address from the challenge or use default.
pub fn resolve_escrow_contract(method_details: Option<&serde_json::Value>) -> Address {
    method_details
        .and_then(|md| md.get("escrowContract"))
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<Address>().ok())
        .unwrap_or_else(|| DEFAULT_ESCROW.parse().unwrap())
}

/// Resolve the chain ID from method details.
pub fn resolve_chain_id(method_details: Option<&serde_json::Value>, fallback: u64) -> u64 {
    method_details
        .and_then(|md| md.get("chainId"))
        .and_then(|v| v.as_u64())
        .unwrap_or(fallback)
}

/// Get the deposit amount (atomic units) from the stream request or use default.
pub fn resolve_deposit(suggested_deposit: Option<&str>) -> u128 {
    suggested_deposit
        .and_then(|s| s.parse::<u128>().ok())
        .unwrap_or(DEFAULT_DEPOSIT_ATOMIC)
}

/// On-chain channel state recovered from the escrow contract.
#[derive(Debug, Clone)]
pub struct OnChainChannel {
    pub payer: Address,
    pub payee: Address,
    pub token: Address,
    pub authorized_signer: Address,
    pub deposit: u128,
    pub settled: u128,
    #[allow(dead_code)]
    pub close_requested_at: u64,
    pub finalized: bool,
}

/// Query the escrow contract for on-chain channel state.
pub async fn query_on_chain_channel<P: alloy::providers::Provider>(
    provider: &P,
    escrow_contract: Address,
    channel_id: B256,
) -> Result<Option<OnChainChannel>> {
    use crate::payment::abi::IEscrow;

    let contract = IEscrow::new(escrow_contract, provider);
    match contract.getChannel(channel_id).call().await {
        Ok(result) => {
            if result.payer == Address::ZERO {
                return Ok(None);
            }
            Ok(Some(OnChainChannel {
                payer: result.payer,
                payee: result.payee,
                token: result.token,
                authorized_signer: result.authorizedSigner,
                deposit: result.deposit,
                settled: result.settled,
                close_requested_at: result.closeRequestedAt,
                finalized: result.finalized,
            }))
        }
        Err(_) => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_channel_id_deterministic() {
        let payer: Address = "0x1111111111111111111111111111111111111111"
            .parse()
            .unwrap();
        let payee: Address = "0x2222222222222222222222222222222222222222"
            .parse()
            .unwrap();
        let token: Address = "0x3333333333333333333333333333333333333333"
            .parse()
            .unwrap();
        let deposit = 1_000_000u128;
        let salt = B256::ZERO;
        let auth: Address = "0x4444444444444444444444444444444444444444"
            .parse()
            .unwrap();
        let escrow: Address = "0x5555555555555555555555555555555555555555"
            .parse()
            .unwrap();
        let chain_id = 42431u64;

        let id1 = compute_channel_id(payer, payee, token, deposit, salt, auth, escrow, chain_id);
        let id2 = compute_channel_id(payer, payee, token, deposit, salt, auth, escrow, chain_id);
        assert_eq!(id1, id2);
        assert_ne!(id1, B256::ZERO);
    }

    #[test]
    fn test_compute_channel_id_different_params() {
        let payer: Address = "0x1111111111111111111111111111111111111111"
            .parse()
            .unwrap();
        let payee: Address = "0x2222222222222222222222222222222222222222"
            .parse()
            .unwrap();
        let token: Address = "0x3333333333333333333333333333333333333333"
            .parse()
            .unwrap();
        let salt = B256::ZERO;
        let auth: Address = "0x4444444444444444444444444444444444444444"
            .parse()
            .unwrap();
        let escrow: Address = "0x5555555555555555555555555555555555555555"
            .parse()
            .unwrap();

        let id1 = compute_channel_id(payer, payee, token, 1_000_000, salt, auth, escrow, 42431);
        let id2 = compute_channel_id(payer, payee, token, 2_000_000, salt, auth, escrow, 42431);
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_sign_voucher_produces_65_byte_sig() {
        let signer: PrivateKeySigner =
            "0x1234567890123456789012345678901234567890123456789012345678901234"
                .parse()
                .unwrap();
        let channel_id = B256::ZERO;
        let escrow: Address = "0x9d136eEa063eDE5418A6BC7bEafF009bBb6CFa70"
            .parse()
            .unwrap();

        let sig = sign_voucher(&signer, channel_id, 1000, escrow, 42431).unwrap();
        assert!(sig.starts_with("0x"));
        // 65 bytes = 130 hex chars + 0x prefix = 132
        assert_eq!(sig.len(), 132);
    }

    #[test]
    fn test_sign_voucher_deterministic() {
        let signer: PrivateKeySigner =
            "0x1234567890123456789012345678901234567890123456789012345678901234"
                .parse()
                .unwrap();
        let channel_id = B256::ZERO;
        let escrow: Address = "0x9d136eEa063eDE5418A6BC7bEafF009bBb6CFa70"
            .parse()
            .unwrap();

        let sig1 = sign_voucher(&signer, channel_id, 1000, escrow, 42431).unwrap();
        let sig2 = sign_voucher(&signer, channel_id, 1000, escrow, 42431).unwrap();
        assert_eq!(sig1, sig2);
    }

    #[test]
    fn test_sign_voucher_different_amount_different_sig() {
        let signer: PrivateKeySigner =
            "0x1234567890123456789012345678901234567890123456789012345678901234"
                .parse()
                .unwrap();
        let channel_id = B256::ZERO;
        let escrow: Address = "0x9d136eEa063eDE5418A6BC7bEafF009bBb6CFa70"
            .parse()
            .unwrap();

        let sig1 = sign_voucher(&signer, channel_id, 1000, escrow, 42431).unwrap();
        let sig2 = sign_voucher(&signer, channel_id, 2000, escrow, 42431).unwrap();
        assert_ne!(sig1, sig2);
    }

    #[test]
    fn test_channel_key_format() {
        let payer: Address = "0x1111111111111111111111111111111111111111"
            .parse()
            .unwrap();
        let payee: Address = "0x2222222222222222222222222222222222222222"
            .parse()
            .unwrap();
        let token: Address = "0x3333333333333333333333333333333333333333"
            .parse()
            .unwrap();
        let escrow: Address = "0x4444444444444444444444444444444444444444"
            .parse()
            .unwrap();

        let key = StreamState::channel_key(&payer, &payee, &token, &escrow, 42431);
        assert!(key.contains(":"));
        assert!(key.contains("42431"));
    }

    #[test]
    fn test_resolve_escrow_contract_from_method_details() {
        let md =
            serde_json::json!({"escrowContract": "0x1234567890123456789012345678901234567890"});
        let addr = resolve_escrow_contract(Some(&md));
        assert_eq!(
            format!("{:#x}", addr),
            "0x1234567890123456789012345678901234567890"
        );
    }

    #[test]
    fn test_resolve_escrow_contract_default() {
        let addr = resolve_escrow_contract(None);
        assert_eq!(
            format!("{:#x}", addr).to_lowercase(),
            DEFAULT_ESCROW.to_lowercase()
        );
    }

    #[test]
    fn test_resolve_deposit_from_suggestion() {
        assert_eq!(resolve_deposit(Some("5000000")), 5_000_000);
    }

    #[test]
    fn test_resolve_deposit_default() {
        assert_eq!(resolve_deposit(None), DEFAULT_DEPOSIT_ATOMIC);
    }
}
