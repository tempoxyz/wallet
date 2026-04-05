//! Core EIP-1186 Merkle-Patricia trie proof verification.
//!
//! Verifies `eth_getProof` responses against the RPC's reported block state
//! root. This detects inconsistent or forged proof data from the provider.
//!
//! **Note:** The state root is fetched from the same RPC that returns the
//! proof, so this provides *consistency verification* (the proof matches
//! the provider's claimed state), not full trustless verification. For full
//! malicious-RPC resistance, the state root should be anchored to an
//! independent trust source (e.g., Tempo L1).

use std::error::Error;

use alloy::{
    eips::BlockNumberOrTag,
    primitives::{keccak256, Address, B256, U256},
    providers::Provider,
    rpc::types::{EIP1186AccountProofResponse, EIP1186StorageProof},
};
use alloy_rlp::Encodable;
use alloy_trie::proof::{verify_proof, ProofVerificationError};
use nybbles::Nibbles;
use thiserror::Error as ThisError;
use tracing::debug;

use crate::error::{NetworkError, TempoError};

// ==================== Types ====================

/// A block pinned by number and state root, used as the verification anchor.
#[derive(Debug, Clone, Copy)]
pub struct PinnedBlock {
    /// Block number.
    pub block_number: u64,
    /// State root at this block.
    pub state_root: B256,
}

/// Errors that can occur during proof verification.
#[derive(ThisError, Debug)]
pub enum ProofError {
    /// An RPC call failed.
    #[error("RPC failed during {operation}: {source}")]
    RpcFailed {
        operation: &'static str,
        #[source]
        source: Box<dyn Error + Send + Sync>,
    },
    /// The account proof did not verify against the state root.
    #[error("account proof invalid for {address}: {source}")]
    AccountProofInvalid {
        address: Address,
        #[source]
        source: Box<ProofVerificationError>,
    },
    /// A storage proof did not verify against the account's storage hash.
    #[error("storage proof invalid for {address} slot {slot}: {source}")]
    StorageProofInvalid {
        address: Address,
        slot: B256,
        #[source]
        source: Box<ProofVerificationError>,
    },
    /// The storage proof returned by the RPC does not match the requested slot.
    #[error("storage proof key mismatch: requested {expected}, got {actual}")]
    StorageKeyMismatch { expected: B256, actual: B256 },
    /// The RPC returned an unexpected number of storage proofs.
    #[error("expected {expected} storage proof(s), got {actual}")]
    StorageProofCount { expected: usize, actual: usize },
    /// The block header was missing from the RPC response.
    #[error("block header missing from RPC response")]
    MissingBlockHeader,
}

impl From<ProofError> for TempoError {
    fn from(err: ProofError) -> Self {
        match err {
            ProofError::RpcFailed { operation, source } => {
                NetworkError::RpcSource { operation, source }.into()
            }
            ProofError::AccountProofInvalid { address, source } => NetworkError::Rpc {
                operation: "verify account proof",
                reason: format!("invalid proof for {address}: {source}"),
            }
            .into(),
            ProofError::StorageProofInvalid {
                address,
                slot,
                source,
            } => NetworkError::Rpc {
                operation: "verify storage proof",
                reason: format!("invalid proof for {address} slot {slot}: {source}"),
            }
            .into(),
            ProofError::StorageKeyMismatch { expected, actual } => NetworkError::Rpc {
                operation: "verify storage proof",
                reason: format!("storage proof key mismatch: requested {expected}, got {actual}"),
            }
            .into(),
            ProofError::StorageProofCount { expected, actual } => NetworkError::Rpc {
                operation: "verify storage proof",
                reason: format!("expected {expected} storage proof(s), got {actual}"),
            }
            .into(),
            ProofError::MissingBlockHeader => NetworkError::ResponseMissingField {
                context: "eth_getBlockByNumber",
                field: "state_root",
            }
            .into(),
        }
    }
}

// ==================== Block Pinning ====================

/// Pin the latest block by fetching its header and extracting the state root.
///
/// The returned [`PinnedBlock`] serves as the verification anchor for
/// subsequent proof verifications within the same logical operation.
///
/// # Errors
///
/// Returns [`ProofError::RpcFailed`] if the RPC call fails, or
/// [`ProofError::MissingBlockHeader`] if no block is returned.
pub async fn pin_latest_block<P: Provider>(provider: &P) -> Result<PinnedBlock, ProofError> {
    let block = provider
        .get_block_by_number(BlockNumberOrTag::Latest)
        .await
        .map_err(|e| ProofError::RpcFailed {
            operation: "eth_getBlockByNumber(latest)",
            source: Box::new(e),
        })?
        .ok_or(ProofError::MissingBlockHeader)?;

    let header = &block.header;
    let pinned = PinnedBlock {
        block_number: header.number,
        state_root: header.state_root,
    };
    debug!(
        block_number = pinned.block_number,
        %pinned.state_root,
        "pinned block for proof verification"
    );
    Ok(pinned)
}

// ==================== Proof Verification ====================

/// Verify an account proof against the given state root.
///
/// The key is `keccak256(address)` unpacked to nibbles, and the expected value
/// is the RLP-encoded account tuple `(nonce, balance, storage_hash, code_hash)`.
///
/// # Errors
///
/// Returns [`ProofError::AccountProofInvalid`] if verification fails.
pub fn verify_account_proof(
    state_root: B256,
    address: Address,
    proof_response: &EIP1186AccountProofResponse,
) -> Result<(), ProofError> {
    let key = Nibbles::unpack(keccak256(address));
    let expected_value = rlp_encode_account(proof_response);

    verify_proof(
        state_root,
        key,
        Some(expected_value),
        &proof_response.account_proof,
    )
    .map_err(|source| ProofError::AccountProofInvalid {
        address,
        source: Box::new(source),
    })
}

/// Verify a single storage proof against the account's storage hash.
///
/// Checks that the proof's key matches the `expected_slot`, then verifies the
/// MPT proof. For non-zero values the expected value is the RLP encoding of the
/// value; for zero values this is an exclusion proof (`expected = None`).
///
/// # Errors
///
/// Returns [`ProofError::StorageKeyMismatch`] if the proof's key differs from
/// the requested slot, or [`ProofError::StorageProofInvalid`] if the MPT proof
/// does not verify.
pub fn verify_storage_proof(
    storage_hash: B256,
    address: Address,
    storage_proof: &EIP1186StorageProof,
    expected_slot: B256,
) -> Result<(), ProofError> {
    let returned_slot = B256::from(storage_proof.key.as_b256());
    if returned_slot != expected_slot {
        return Err(ProofError::StorageKeyMismatch {
            expected: expected_slot,
            actual: returned_slot,
        });
    }

    let key = Nibbles::unpack(keccak256(expected_slot));

    let expected_value = if storage_proof.value.is_zero() {
        None
    } else {
        Some(alloy_rlp::encode(storage_proof.value))
    };

    verify_proof(storage_hash, key, expected_value, &storage_proof.proof).map_err(|source| {
        ProofError::StorageProofInvalid {
            address,
            slot: expected_slot,
            source: Box::new(source),
        }
    })
}

// ==================== High-Level Verified Reads ====================

/// Fetch and verify the value of a single storage slot.
///
/// Calls `eth_getProof`, verifies both account and storage proofs against the
/// pinned block's state root, then returns the verified storage value.
///
/// Also validates that the RPC returned exactly one storage proof matching the
/// requested slot.
///
/// # Errors
///
/// Returns [`ProofError`] if the RPC call, proof verification, or response
/// validation fails.
pub async fn verified_storage_at<P: Provider>(
    provider: &P,
    address: Address,
    slot: B256,
    block: &PinnedBlock,
) -> Result<U256, ProofError> {
    let proof_response = provider
        .get_proof(address, vec![slot])
        .block_id(block.block_number.into())
        .await
        .map_err(|e| ProofError::RpcFailed {
            operation: "eth_getProof",
            source: Box::new(e),
        })?;

    verify_account_proof(block.state_root, address, &proof_response)?;

    if proof_response.storage_proof.len() != 1 {
        return Err(ProofError::StorageProofCount {
            expected: 1,
            actual: proof_response.storage_proof.len(),
        });
    }
    let storage = &proof_response.storage_proof[0];

    verify_storage_proof(proof_response.storage_hash, address, storage, slot)?;

    debug!(
        %address,
        %slot,
        value = %storage.value,
        block_number = block.block_number,
        "verified storage read"
    );
    Ok(storage.value)
}

/// Fetch and verify the native (TEMPO) balance of an account.
///
/// Calls `eth_getProof` (with no storage keys), verifies the account proof,
/// then returns the balance from the verified proof response.
///
/// # Errors
///
/// Returns [`ProofError`] if the RPC call or proof verification fails.
pub async fn verified_account_balance<P: Provider>(
    provider: &P,
    address: Address,
    block: &PinnedBlock,
) -> Result<U256, ProofError> {
    let proof_response = provider
        .get_proof(address, vec![])
        .block_id(block.block_number.into())
        .await
        .map_err(|e| ProofError::RpcFailed {
            operation: "eth_getProof",
            source: Box::new(e),
        })?;

    verify_account_proof(block.state_root, address, &proof_response)?;

    debug!(
        %address,
        balance = %proof_response.balance,
        block_number = block.block_number,
        "verified native balance"
    );
    Ok(proof_response.balance)
}

/// Fetch and verify a TIP-20 (ERC-20) token balance via storage proof.
///
/// Computes the storage slot for `balanceOf[account]` using the given mapping
/// slot index, then fetches and verifies the storage proof against the pinned
/// block's state root.
///
/// # Arguments
///
/// * `balance_slot_index` — The Solidity storage slot of the `balanceOf`
///   mapping. Use `0` for standard OpenZeppelin ERC-20 layout, or the
///   appropriate index for the specific contract (see [`crate::network`] for
///   Tempo-known token slot indices).
///
/// # Errors
///
/// Returns [`ProofError`] if the RPC call or proof verification fails.
pub async fn verified_token_balance<P: Provider>(
    provider: &P,
    token: Address,
    account: Address,
    balance_slot_index: u8,
    block: &PinnedBlock,
) -> Result<U256, ProofError> {
    let slot = mapping_slot(account, balance_slot_index);
    debug!(%token, %account, %slot, balance_slot_index, "computed TIP-20 balance storage slot");
    verified_storage_at(provider, token, slot, block).await
}

// ==================== Helpers ====================

/// Compute the storage slot for `mapping[key]` at the given slot index.
///
/// For a Solidity `mapping(address => T)` at storage slot `slot_index`, the
/// storage key for `key` is `keccak256(abi.encode(key, uint256(slot_index)))`.
fn mapping_slot(key: Address, slot_index: u8) -> B256 {
    let mut buf = [0u8; 64];
    // Left-pad address to 32 bytes (abi.encode pads address to 32 bytes).
    buf[12..32].copy_from_slice(key.as_slice());
    // Slot index in the second 32 bytes (big-endian).
    buf[63] = slot_index;
    keccak256(buf)
}

/// RLP-encode an account as `[nonce, balance, storage_hash, code_hash]`.
fn rlp_encode_account(proof: &EIP1186AccountProofResponse) -> Vec<u8> {
    let items_len = proof.nonce.length()
        + proof.balance.length()
        + proof.storage_hash.length()
        + proof.code_hash.length();

    let mut buf = Vec::with_capacity(items_len + alloy_rlp::length_of_length(items_len) + 1);
    alloy_rlp::Header {
        list: true,
        payload_length: items_len,
    }
    .encode(&mut buf);
    proof.nonce.encode(&mut buf);
    proof.balance.encode(&mut buf);
    proof.storage_hash.encode(&mut buf);
    proof.code_hash.encode(&mut buf);
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping_slot_computation_slot_zero() {
        // For address 0x0000...0001 with mapping slot 0, the storage key should
        // be keccak256(abi.encode(address(1), uint256(0))).
        let account = Address::from_word(B256::with_last_byte(1));
        let slot = mapping_slot(account, 0);

        let mut expected_input = [0u8; 64];
        expected_input[31] = 1; // address(1) left-padded to 32 bytes
                                // second 32 bytes are slot 0, already zeroed
        let expected = keccak256(expected_input);

        assert_eq!(slot, expected);
    }

    #[test]
    fn mapping_slot_computation_slot_one() {
        // For address 0x0000...0001 with mapping slot 1.
        let account = Address::from_word(B256::with_last_byte(1));
        let slot = mapping_slot(account, 1);

        let mut expected_input = [0u8; 64];
        expected_input[31] = 1; // address(1)
        expected_input[63] = 1; // slot index 1
        let expected = keccak256(expected_input);

        assert_eq!(slot, expected);
    }

    #[test]
    fn different_slot_indices_produce_different_keys() {
        let account: Address = "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045"
            .parse()
            .unwrap();
        let slot0 = mapping_slot(account, 0);
        let slot1 = mapping_slot(account, 1);
        assert_ne!(slot0, slot1, "slot 0 and slot 1 must differ");
    }

    #[test]
    fn proof_error_display_account() {
        let err = ProofError::AccountProofInvalid {
            address: Address::ZERO,
            source: Box::new(ProofVerificationError::UnexpectedEmptyRoot),
        };
        let msg = err.to_string();
        assert!(msg.contains("account proof invalid"), "got: {msg}");
        assert!(msg.contains(&Address::ZERO.to_string()), "got: {msg}");
    }

    #[test]
    fn proof_error_display_storage() {
        let err = ProofError::StorageProofInvalid {
            address: Address::ZERO,
            slot: B256::ZERO,
            source: Box::new(ProofVerificationError::UnexpectedEmptyRoot),
        };
        let msg = err.to_string();
        assert!(msg.contains("storage proof invalid"), "got: {msg}");
    }

    #[test]
    fn proof_error_display_key_mismatch() {
        let err = ProofError::StorageKeyMismatch {
            expected: B256::with_last_byte(1),
            actual: B256::with_last_byte(2),
        };
        let msg = err.to_string();
        assert!(msg.contains("mismatch"), "got: {msg}");
    }

    #[test]
    fn proof_error_display_count() {
        let err = ProofError::StorageProofCount {
            expected: 1,
            actual: 0,
        };
        assert_eq!(err.to_string(), "expected 1 storage proof(s), got 0");
    }

    #[test]
    fn proof_error_display_rpc_failed() {
        let err = ProofError::RpcFailed {
            operation: "eth_getProof",
            source: Box::new(std::io::Error::other("connection refused")),
        };
        assert_eq!(
            err.to_string(),
            "RPC failed during eth_getProof: connection refused"
        );
    }

    #[test]
    fn proof_error_display_missing_header() {
        let err = ProofError::MissingBlockHeader;
        assert_eq!(err.to_string(), "block header missing from RPC response");
    }

    #[test]
    fn proof_error_converts_to_tempo_error() {
        let err = ProofError::MissingBlockHeader;
        let tempo_err: TempoError = err.into();
        let msg = tempo_err.to_string();
        assert!(
            msg.contains("eth_getBlockByNumber"),
            "expected NetworkError mapping, got: {msg}"
        );
    }

    #[test]
    fn key_mismatch_converts_to_tempo_error() {
        let err = ProofError::StorageKeyMismatch {
            expected: B256::ZERO,
            actual: B256::with_last_byte(1),
        };
        let tempo_err: TempoError = err.into();
        assert!(
            tempo_err.to_string().contains("mismatch"),
            "got: {tempo_err}"
        );
    }

    #[test]
    fn count_mismatch_converts_to_tempo_error() {
        let err = ProofError::StorageProofCount {
            expected: 1,
            actual: 3,
        };
        let tempo_err: TempoError = err.into();
        assert!(
            tempo_err.to_string().contains("expected 1"),
            "got: {tempo_err}"
        );
    }
}
