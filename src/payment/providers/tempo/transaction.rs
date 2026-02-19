//! Tempo transaction construction and signing.

use crate::error::{Result, ResultExt, SigningContext};
use crate::network::GasConfig;
use alloy::primitives::{Address, U256};
use alloy::signers::{local::PrivateKeySigner, SignerSync};
use tracing::debug;

use tempo_primitives::transaction::{
    AASigned, KeychainSignature, PrimitiveSignature, SignedKeyAuthorization, TempoSignature,
    TempoTransaction,
};

/// Create a Tempo transaction with multiple calls.
///
/// When `key_authorization` is `Some`, it is included in the transaction to
/// atomically provision the access key on-chain alongside the payment.
#[allow(clippy::too_many_arguments)]
pub(super) fn create_tempo_transaction_with_calls(
    signer: &PrivateKeySigner,
    chain_id: u64,
    nonce: u64,
    fee_token: Address,
    calls: Vec<tempo_primitives::transaction::Call>,
    gas_config: &GasConfig,
    gas_limit: u64,
    wallet_address: Option<Address>,
    key_authorization: Option<SignedKeyAuthorization>,
) -> Result<String> {
    debug!(
        chain_id,
        nonce,
        fee_token = %format!("{:#x}", fee_token),
        gas_limit,
        max_fee_per_gas = gas_config.max_fee_per_gas,
        max_priority_fee_per_gas = gas_config.max_priority_fee_per_gas,
        num_calls = calls.len(),
        signing_mode = if wallet_address.is_some() { "keychain" } else { "direct" },
        has_key_authorization = key_authorization.is_some(),
        "constructing tempo tx (type 0x76)"
    );

    let tx = TempoTransaction {
        chain_id,
        fee_token: Some(fee_token),
        max_priority_fee_per_gas: gas_config.max_priority_fee_per_gas_u128(),
        max_fee_per_gas: gas_config.max_fee_per_gas_u128(),
        gas_limit,
        calls,
        access_list: Default::default(),
        nonce_key: U256::ZERO,
        nonce,
        fee_payer_signature: None,
        valid_before: None,
        valid_after: None,
        key_authorization,
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

    debug!(tx_size_bytes = buf.len(), "signed tempo tx");

    Ok(hex::encode(&buf))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempo_primitives::transaction::Call;

    #[allow(clippy::too_many_arguments)]
    fn create_tempo_transaction(
        signer: &PrivateKeySigner,
        chain_id: u64,
        nonce: u64,
        asset: Address,
        transfer_data: alloy::primitives::Bytes,
        gas_config: &GasConfig,
        wallet_address: Option<Address>,
        key_authorization: Option<SignedKeyAuthorization>,
    ) -> Result<String> {
        use alloy::primitives::TxKind;

        let calls = vec![Call {
            to: TxKind::Call(asset),
            value: U256::ZERO,
            input: transfer_data,
        }];

        create_tempo_transaction_with_calls(
            signer,
            chain_id,
            nonce,
            asset,
            calls,
            gas_config,
            500_000,
            wallet_address,
            key_authorization,
        )
    }

    fn decode_gas_limit(tx_hex: &str) -> u64 {
        use alloy::eips::eip2718::Decodable2718;
        let bytes = hex::decode(tx_hex).unwrap();
        let signed = AASigned::decode_2718(&mut bytes.as_slice()).unwrap();
        signed.tx().gas_limit
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
            None,
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
            None,
        )
        .unwrap();

        assert!(keychain_tx.len() > direct_tx.len());
    }

    #[test]
    fn test_create_tempo_transaction_with_key_authorization_produces_longer_tx() {
        use tempo_primitives::transaction::{KeyAuthorization, SignatureType};

        let signer: PrivateKeySigner =
            "0x1234567890123456789012345678901234567890123456789012345678901234"
                .parse()
                .unwrap();

        let asset = Address::ZERO;
        let transfer_data = alloy::primitives::Bytes::new();
        let wallet_address = Address::repeat_byte(0xAB);

        let key_auth = KeyAuthorization {
            chain_id: 42431,
            key_type: SignatureType::Secp256k1,
            key_id: signer.address(),
            expiry: Some(9999999999),
            limits: None,
        };

        let inner_sig = signer.sign_hash_sync(&key_auth.signature_hash()).unwrap();
        let signed_auth = key_auth.into_signed(PrimitiveSignature::Secp256k1(inner_sig));

        let tx_without = create_tempo_transaction(
            &signer,
            42431,
            0,
            asset,
            transfer_data.clone(),
            &GasConfig::DEFAULT,
            Some(wallet_address),
            None,
        )
        .unwrap();

        let tx_with = create_tempo_transaction(
            &signer,
            42431,
            0,
            asset,
            transfer_data,
            &GasConfig::DEFAULT,
            Some(wallet_address),
            Some(signed_auth),
        )
        .unwrap();

        assert!(tx_with.len() > tx_without.len());
        assert!(tx_with.starts_with("76"));
    }

    #[test]
    fn test_create_tempo_transaction_without_key_authorization_still_works() {
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
            None,
        );

        assert!(result.is_ok());
        let tx_hex = result.unwrap();
        assert!(tx_hex.starts_with("76"));
    }

    #[test]
    fn test_nonce_zero_uses_default_gas_limit() {
        let signer: PrivateKeySigner =
            "0x1234567890123456789012345678901234567890123456789012345678901234"
                .parse()
                .unwrap();

        let asset = Address::ZERO;
        let transfer_data = alloy::primitives::Bytes::new();
        let gas = GasConfig::DEFAULT;

        let tx_nonce_0 = create_tempo_transaction(
            &signer,
            42431,
            0,
            asset,
            transfer_data.clone(),
            &gas,
            None,
            None,
        )
        .unwrap();

        let tx_nonce_1 =
            create_tempo_transaction(&signer, 42431, 1, asset, transfer_data, &gas, None, None)
                .unwrap();

        assert_eq!(
            decode_gas_limit(&tx_nonce_0),
            500_000,
            "nonce 0 should use default gas limit (estimation is done via RPC)"
        );
        assert_eq!(
            decode_gas_limit(&tx_nonce_1),
            500_000,
            "nonce > 0 should use default gas limit"
        );
    }
}
