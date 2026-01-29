//! Tempo payment provider implementation.
//!
//! This module provides Tempo-specific payment functionality with support for:
//! - Type 0x76 (Tempo) transactions
//! - Keychain (access key) signing mode
//! - Memo support via transferWithMemo

use crate::config::Config;
use crate::error::{PgetError, Result, ResultExt, SigningContext};
use crate::network::{GasConfig, Network};
use crate::payment::mpay_ext::{ChargeRequestExt, TempoChargeExt};
use alloy::primitives::{Address, U256};
use alloy::signers::{local::PrivateKeySigner, SignerSync};
use std::str::FromStr;

use tempo_primitives::transaction::{
    AASigned, Call, KeychainSignature, PrimitiveSignature, TempoSignature, TempoTransaction,
};

/// Check if a network name refers to a Tempo network.
#[allow(dead_code)]
pub fn is_tempo_network(name: &str) -> bool {
    matches!(name.to_lowercase().as_str(), "tempo" | "tempo-moderato")
}

/// Create a Tempo payment credential for a Web Payment Auth challenge.
///
/// Supports keychain signing mode when `wallet_address` is configured.
pub async fn create_tempo_payment(
    config: &Config,
    challenge: &mpay::PaymentChallenge,
) -> Result<mpay::PaymentCredential> {
    use mpay::{ChargeRequest, PaymentCredential, PaymentPayload};

    use crate::payment::mpay_ext::method_to_network;
    use crate::wallet::signer::WalletSource;
    use alloy::primitives::Bytes;
    use alloy::providers::Provider;
    use alloy::sol;
    use alloy::sol_types::SolCall;

    let charge_req: ChargeRequest = challenge
        .request
        .decode()
        .map_err(|e| PgetError::InvalidChallenge(format!("Invalid charge request: {}", e)))?;

    let evm_config = config.require_evm()?;
    let signer = evm_config.load_signer(None)?;

    // If wallet_address is set, use keychain signing mode
    let wallet_address = evm_config
        .wallet_address
        .as_ref()
        .map(|addr| {
            Address::from_str(addr)
                .map_err(|e| PgetError::InvalidConfig(format!("Invalid wallet address: {}", e)))
        })
        .transpose()?;

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

/// Create a Tempo transaction (type 0x76) with network-specific gas configuration.
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

    let tempo_signature: TempoSignature = if let Some(wallet_addr) = wallet_address {
        let keychain_sig =
            KeychainSignature::new(wallet_addr, PrimitiveSignature::Secp256k1(inner_signature));
        TempoSignature::Keychain(keychain_sig)
    } else {
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
    fn test_create_tempo_transaction_direct_signing() {
        let signer: PrivateKeySigner =
            "0x1234567890123456789012345678901234567890123456789012345678901234"
                .parse()
                .unwrap();

        let asset = Address::ZERO;
        let transfer_data = alloy::primitives::Bytes::new();

        let result = create_tempo_transaction(
            &signer,
            42431,
            0,
            asset,
            transfer_data,
            &GasConfig::DEFAULT,
            None,
        );

        assert!(result.is_ok());
        let tx_hex = result.unwrap();
        assert!(tx_hex.starts_with("76"));
    }

    #[test]
    fn test_create_tempo_transaction_keychain_signing() {
        let signer: PrivateKeySigner =
            "0x1234567890123456789012345678901234567890123456789012345678901234"
                .parse()
                .unwrap();

        let asset = Address::ZERO;
        let transfer_data = alloy::primitives::Bytes::new();
        let wallet_address = Address::repeat_byte(0xAB);

        let result = create_tempo_transaction(
            &signer,
            42431,
            0,
            asset,
            transfer_data,
            &GasConfig::DEFAULT,
            Some(wallet_address),
        );

        assert!(result.is_ok());
        let tx_hex = result.unwrap();
        assert!(tx_hex.starts_with("76"));
    }

    #[test]
    fn test_keychain_vs_direct_signing_produces_different_tx() {
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

        assert!(keychain_tx.len() > direct_tx.len());
    }
}
