//! Shared Tempo transaction signing and broadcast helpers.
//!
//! Low-level Tempo type-0x76 transaction construction and receipt polling.
//! All transactions use expiring nonces (nonceKey=MAX, nonce=0) so no
//! on-chain nonce fetch is needed.

use alloy::primitives::{Address, Bytes, TxKind, B256, U256};
use alloy::providers::Provider;
use alloy::sol;
use alloy::sol_types::SolCall;
use tempo_primitives::transaction::Call;

use mpp::client::tempo::{charge::tx_builder, signing};

use crate::error::{KeyError, NetworkError, TempoError};
use crate::keys::Signer;

type SessionResult<T> = Result<T, TempoError>;

// ==================== ABI Definitions ====================

sol! {
    interface ITIP20 {
        function approve(address spender, uint256 amount) external returns (bool);
    }
    interface IEscrow {
        function open(
            address payee,
            address token,
            uint128 deposit,
            bytes32 salt,
            address authorizedSigner
        ) external;
    }
}

/// Static max fee per gas (41 gwei) — Tempo uses a fixed 20 gwei base fee.
const MAX_FEE_PER_GAS: u128 = mpp::client::tempo::MAX_FEE_PER_GAS;

/// Static max priority fee per gas (1 gwei).
const MAX_PRIORITY_FEE_PER_GAS: u128 = mpp::client::tempo::MAX_PRIORITY_FEE_PER_GAS;

/// Expiring nonce key (`U256::MAX`).
const EXPIRING_NONCE_KEY: U256 = U256::MAX;

/// Validity window (in seconds) for expiring nonce transactions.
const VALID_BEFORE_SECS: u64 = 25;

/// Marker used by provider errors when an authorization key is already present on-chain.
const KEY_ALREADY_EXISTS_MARKER: &str = "KeyAlreadyExists";

/// Compute the expiring nonce validity window.
fn expiring_valid_before() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        + VALID_BEFORE_SECS
}

fn is_key_already_exists_error(err: &impl std::fmt::Display) -> bool {
    err.to_string().contains(KEY_ALREADY_EXISTS_MARKER)
}

/// Estimate gas, build and sign a Tempo type-0x76 transaction.
///
/// Uses expiring nonces (nonceKey=MAX, nonce=0) and static gas fees
/// (Tempo has a fixed 20 gwei base fee), so only a single RPC call
/// (`eth_estimateGas`) is needed.
///
/// # Errors
///
/// Returns an error when gas estimation, transaction signing, or encoding fails.
pub async fn resolve_and_sign_tx(
    provider: &alloy::providers::RootProvider<mpp::client::TempoNetwork>,
    wallet: &Signer,
    chain_id: u64,
    fee_token: Address,
    from: Address,
    calls: Vec<tempo_primitives::transaction::Call>,
) -> SessionResult<Vec<u8>> {
    let nonce = 0u64;
    let valid_before = Some(expiring_valid_before());

    let mut key_auth = wallet.signing_mode.key_authorization();

    let gas_result = tx_builder::estimate_gas(
        provider,
        from,
        chain_id,
        nonce,
        fee_token,
        &calls,
        MAX_FEE_PER_GAS,
        MAX_PRIORITY_FEE_PER_GAS,
        key_auth,
        EXPIRING_NONCE_KEY,
        valid_before,
    )
    .await;

    // If gas estimation fails with KeyAlreadyExists, the key is already
    // provisioned on-chain but the local `provisioned` flag is stale.
    // Retry without key_authorization.
    let gas_limit = match gas_result {
        Ok(gas) => gas,
        Err(e) if key_auth.is_some() && is_key_already_exists_error(&e) => {
            key_auth = None;
            tx_builder::estimate_gas(
                provider,
                from,
                chain_id,
                nonce,
                fee_token,
                &calls,
                MAX_FEE_PER_GAS,
                MAX_PRIORITY_FEE_PER_GAS,
                None,
                EXPIRING_NONCE_KEY,
                valid_before,
            )
            .await
            .map_err(|source| KeyError::SigningOperationSource {
                operation: "estimate gas (retry without key authorization)",
                source: Box::new(source),
            })?
        }
        Err(e) => {
            return Err(KeyError::SigningOperationSource {
                operation: "estimate gas",
                source: Box::new(e),
            }
            .into())
        }
    };

    let tx = tx_builder::build_tempo_tx(tx_builder::TempoTxOptions {
        calls,
        chain_id,
        fee_token,
        nonce,
        nonce_key: EXPIRING_NONCE_KEY,
        gas_limit,
        max_fee_per_gas: MAX_FEE_PER_GAS,
        max_priority_fee_per_gas: MAX_PRIORITY_FEE_PER_GAS,
        fee_payer: false,
        valid_before,
        key_authorization: key_auth.cloned(),
    });

    Ok(
        signing::sign_and_encode_async(tx, &wallet.signer, &wallet.signing_mode)
            .await
            .map_err(|source| KeyError::SigningOperationSource {
                operation: "sign and encode transaction",
                source: Box::new(source),
            })?,
    )
}

/// Submit a Tempo type-0x76 transaction and return the tx hash.
///
/// Uses expiring nonces so no on-chain nonce fetch is needed.
///
/// # Errors
///
/// Returns an error when signing fails or transaction broadcast fails.
pub async fn submit_tempo_tx(
    provider: &alloy::providers::RootProvider<mpp::client::TempoNetwork>,
    wallet: &Signer,
    chain_id: u64,
    fee_token: Address,
    from: Address,
    calls: Vec<tempo_primitives::transaction::Call>,
) -> SessionResult<String> {
    let tx_bytes = resolve_and_sign_tx(provider, wallet, chain_id, fee_token, from, calls).await?;

    let pending = provider
        .send_raw_transaction(&tx_bytes)
        .await
        .map_err(|source| NetworkError::RpcSource {
            operation: "broadcast transaction",
            source: Box::new(source),
        })?;

    Ok(format!("{:#x}", pending.tx_hash()))
}

// ==================== Transaction Construction ====================

/// Build the escrow open calls: approve + open.
///
/// Constructs a 2-call sequence:
/// 1. `approve(escrow_contract, deposit)` on the currency token
/// 2. `IEscrow::open(payee, currency, deposit, salt, authorizedSigner)` on the escrow contract
#[must_use]
pub fn build_open_calls(
    currency: Address,
    escrow_contract: Address,
    deposit: u128,
    payee: Address,
    salt: B256,
    authorized_signer: Address,
) -> Vec<Call> {
    let approve_data = Bytes::from(
        ITIP20::approveCall {
            spender: escrow_contract,
            amount: U256::from(deposit),
        }
        .abi_encode(),
    );
    let open_data = Bytes::from(
        IEscrow::openCall::new((payee, currency, deposit, salt, authorized_signer)).abi_encode(),
    );

    vec![
        Call {
            to: TxKind::Call(currency),
            value: U256::ZERO,
            input: approve_data,
        },
        Call {
            to: TxKind::Call(escrow_contract),
            value: U256::ZERO,
            input: open_data,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expiring_valid_before_is_future() {
        let vb = expiring_valid_before();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        // Must be in the future (now < vb <= now + VALID_BEFORE_SECS)
        assert!(vb > now);
        assert!(vb <= now + VALID_BEFORE_SECS);
    }

    #[test]
    fn test_constants_match_mpp_rs() {
        assert_eq!(MAX_FEE_PER_GAS, 41_000_000_000); // 41 gwei
        assert_eq!(MAX_PRIORITY_FEE_PER_GAS, 1_000_000_000); // 1 gwei
        assert_eq!(EXPIRING_NONCE_KEY, U256::MAX);
    }

    #[test]
    fn test_key_already_exists_detector() {
        assert!(is_key_already_exists_error(
            &"rpc failure: KeyAlreadyExists"
        ));
        assert!(!is_key_already_exists_error(&"rpc failure: nonce too low"));
    }
}
