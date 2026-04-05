//! EIP-1186 state proof verification.
//!
//! Provides verified on-chain reads by fetching Merkle-Patricia trie proofs
//! via `eth_getProof` and verifying them against a pinned block's state root.
//!
//! This detects inconsistent or forged proof data from the RPC provider.
//! For full malicious-RPC resistance, the state root should be anchored
//! to an independent trust source (e.g., Tempo L1).

mod verify;

pub use verify::{
    pin_latest_block, verified_account_balance, verified_storage_at, verified_token_balance,
    verify_account_proof, verify_storage_proof, PinnedBlock, ProofError,
};
