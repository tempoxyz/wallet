//! Shared Tempo transaction signing and broadcast helpers.
//!
//! Low-level Tempo type-0x76 transaction construction and receipt polling.
//! All transactions use expiring nonces (nonceKey=MAX, nonce=0) so no
//! on-chain nonce fetch is needed.

use alloy::{
    primitives::{Address, Bytes, TxKind, B256, U256},
    providers::Provider,
    sol,
    sol_types::SolCall,
};
use tempo_primitives::transaction::Call;

use mpp::client::tempo::{charge::tx_builder, signing, signing::keychain};

use crate::{
    error::{KeyError, NetworkError, TempoError},
    keys::Signer,
};

type ChannelResult<T> = Result<T, TempoError>;

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
        function topUp(bytes32 channelId, uint128 additionalDeposit) external;
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

/// Compute the expiring nonce validity window.
fn expiring_valid_before() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        + VALID_BEFORE_SECS
}

/// On-chain key status from the keychain precompile.
pub enum KeyStatus {
    /// Key is active and provisioned on-chain.
    Active,
    /// Key has never been provisioned (expiry == 0).
    Missing,
    /// Key exists but has been revoked.
    Revoked,
    /// RPC call failed — status unknown.
    Unknown,
}

/// Query the on-chain key status via the keychain precompile.
pub async fn query_key_status(
    provider: &alloy::providers::RootProvider<mpp::client::TempoNetwork>,
    wallet: Address,
    key: Address,
) -> KeyStatus {
    let kc = keychain::IAccountKeychain::new(keychain::KEYCHAIN_ADDRESS, provider);
    match kc.getKey(wallet, key).call().await {
        Ok(info) if info.expiry == 0 => KeyStatus::Missing,
        Ok(info) if info.isRevoked => KeyStatus::Revoked,
        Ok(_) => KeyStatus::Active,
        Err(_) => KeyStatus::Unknown,
    }
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
) -> ChannelResult<Vec<u8>> {
    resolve_and_sign_tx_with_fee_payer(provider, wallet, chain_id, fee_token, from, calls, false)
        .await
}

/// Estimate gas, build and sign a Tempo type-0x76 transaction, optionally in fee-payer mode.
///
/// When `fee_payer` is `true`, the transaction is constructed without a fee token and with
/// a placeholder fee-payer signature so a sponsor can co-sign server-side.
pub async fn resolve_and_sign_tx_with_fee_payer(
    provider: &alloy::providers::RootProvider<mpp::client::TempoNetwork>,
    wallet: &Signer,
    chain_id: u64,
    fee_token: Address,
    from: Address,
    calls: Vec<tempo_primitives::transaction::Call>,
    fee_payer: bool,
) -> ChannelResult<Vec<u8>> {
    let nonce = 0u64;
    let valid_before = Some(expiring_valid_before());
    let effective_fee_token = if fee_payer { Address::ZERO } else { fee_token };

    // Optimistic: assume key is already provisioned (no key_authorization).
    let mut key_auth = wallet.signing_mode.key_authorization();
    let mut effective_wallet = wallet;
    // Hold the provisioning-retry signer if we need to rebuild.
    let provisioning_signer;

    let gas_result = tx_builder::estimate_gas(
        provider,
        from,
        chain_id,
        nonce,
        effective_fee_token,
        &calls,
        MAX_FEE_PER_GAS,
        MAX_PRIORITY_FEE_PER_GAS,
        key_auth,
        EXPIRING_NONCE_KEY,
        valid_before,
    )
    .await;

    let gas_limit = match gas_result {
        Ok(gas) => gas,
        Err(e) if wallet.has_stored_key_authorization() => {
            // Gas estimation failed and we have a stored authorization.
            // Check on-chain whether the key is registered; only retry with
            // key_authorization if the key is definitively missing.
            match query_key_status(provider, from, wallet.signer.address()).await {
                KeyStatus::Missing => {
                    provisioning_signer = wallet.with_key_authorization().unwrap();
                    effective_wallet = &provisioning_signer;
                    key_auth = effective_wallet.signing_mode.key_authorization();
                    tx_builder::estimate_gas(
                        provider,
                        from,
                        chain_id,
                        nonce,
                        effective_fee_token,
                        &calls,
                        MAX_FEE_PER_GAS,
                        MAX_PRIORITY_FEE_PER_GAS,
                        key_auth,
                        EXPIRING_NONCE_KEY,
                        valid_before,
                    )
                    .await
                    .map_err(|source| KeyError::SigningOperationSource {
                        operation: "estimate gas (retry with key provisioning)",
                        source: Box::new(source),
                    })?
                }
                // Key is active, revoked, or status unknown — surface the
                // original error instead of retrying with auth.
                _ => {
                    return Err(KeyError::SigningOperationSource {
                        operation: "estimate gas",
                        source: Box::new(e),
                    }
                    .into());
                }
            }
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
        fee_token: effective_fee_token,
        nonce,
        nonce_key: EXPIRING_NONCE_KEY,
        gas_limit,
        max_fee_per_gas: MAX_FEE_PER_GAS,
        max_priority_fee_per_gas: MAX_PRIORITY_FEE_PER_GAS,
        fee_payer,
        valid_before,
        key_authorization: key_auth.cloned(),
    });

    Ok(
        signing::sign_and_encode_async(
            tx,
            &effective_wallet.signer,
            &effective_wallet.signing_mode,
        )
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
) -> ChannelResult<String> {
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
/// 1. `approve(escrow_contract, deposit)` on the token token
/// 2. `IEscrow::open(payee, token, deposit, salt, authorizedSigner)` on the escrow contract
#[must_use]
pub fn build_open_calls(
    token: Address,
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
        IEscrow::openCall::new((payee, token, deposit, salt, authorized_signer)).abi_encode(),
    );

    vec![
        Call {
            to: TxKind::Call(token),
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

/// Build the escrow top-up calls: approve + topUp.
#[must_use]
pub fn build_top_up_calls(
    token: Address,
    escrow_contract: Address,
    channel_id: B256,
    additional_deposit: u128,
) -> Vec<Call> {
    let approve_data = Bytes::from(
        ITIP20::approveCall {
            spender: escrow_contract,
            amount: U256::from(additional_deposit),
        }
        .abi_encode(),
    );
    let top_up_data =
        Bytes::from(IEscrow::topUpCall::new((channel_id, additional_deposit)).abi_encode());

    vec![
        Call {
            to: TxKind::Call(token),
            value: U256::ZERO,
            input: approve_data,
        },
        Call {
            to: TxKind::Call(escrow_contract),
            value: U256::ZERO,
            input: top_up_data,
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
    fn test_build_top_up_calls_shape() {
        let calls = build_top_up_calls(
            Address::from([0x11; 20]),
            Address::from([0x22; 20]),
            B256::from([0x33; 32]),
            42,
        );
        assert_eq!(calls.len(), 2);
    }
}
