//! EIP-3009 `TransferWithAuthorization` signing via EIP-712.

use alloy::{
    primitives::{Address, Bytes, U256},
    signers::{local::PrivateKeySigner, Signer},
    sol,
    sol_types::SolStruct,
};

use tempo_common::error::{KeyError, TempoError};

use super::types::X402Authorization;

sol! {
    /// EIP-3009 TransferWithAuthorization struct for EIP-712 typed signing.
    #[derive(Debug)]
    struct TransferWithAuthorization {
        address from;
        address to;
        uint256 value;
        uint256 validAfter;
        uint256 validBefore;
        bytes32 nonce;
    }
}

/// Result of signing an EIP-3009 authorization.
pub(super) struct SignedAuthorization {
    pub(super) signature_hex: String,
    pub(super) authorization: X402Authorization,
}

/// Sign an EIP-3009 `TransferWithAuthorization` for the destination chain.
///
/// Builds the EIP-712 domain from the x402 challenge extra fields and signs
/// with the provided `PrivateKeySigner`.
#[allow(clippy::too_many_arguments)]
pub(super) async fn sign_transfer_authorization(
    signer: &PrivateKeySigner,
    from: Address,
    to: Address,
    value: U256,
    valid_after: U256,
    valid_before: U256,
    nonce: [u8; 32],
    // EIP-712 domain fields from x402 challenge
    domain_name: &str,
    domain_version: &str,
    chain_id: u64,
    verifying_contract: Address,
) -> Result<SignedAuthorization, TempoError> {
    let transfer = TransferWithAuthorization {
        from,
        to,
        value,
        validAfter: valid_after,
        validBefore: valid_before,
        nonce: nonce.into(),
    };

    let domain = alloy::sol_types::Eip712Domain {
        name: Some(std::borrow::Cow::Owned(domain_name.to_string())),
        version: Some(std::borrow::Cow::Owned(domain_version.to_string())),
        chain_id: Some(U256::from(chain_id)),
        verifying_contract: Some(verifying_contract),
        salt: None,
    };

    let signing_hash = transfer.eip712_signing_hash(&domain);

    let signature = signer.sign_hash(&signing_hash).await.map_err(|source| {
        KeyError::SigningOperationSource {
            operation: "EIP-3009 TransferWithAuthorization",
            source: Box::new(source),
        }
    })?;

    let sig_bytes: Bytes = signature.as_bytes().into();
    let signature_hex = format!("0x{}", hex::encode(&sig_bytes));

    let authorization = X402Authorization {
        from: from.to_checksum(None),
        to: to.to_checksum(None),
        value: value.to_string(),
        valid_after: valid_after.to_string(),
        valid_before: valid_before.to_string(),
        nonce: format!("0x{}", hex::encode(nonce)),
    };

    Ok(SignedAuthorization {
        signature_hex,
        authorization,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::address;

    #[tokio::test]
    async fn test_sign_transfer_authorization() {
        // Use a well-known test key
        let signer: PrivateKeySigner =
            "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
                .parse()
                .unwrap();
        let from = signer.address();
        let to = address!("1111111111111111111111111111111111111111");
        let value = U256::from(1_000_000u64);
        let valid_after = U256::ZERO;
        let valid_before = U256::from(u64::MAX);
        let nonce = [0u8; 32];

        let result = sign_transfer_authorization(
            &signer,
            from,
            to,
            value,
            valid_after,
            valid_before,
            nonce,
            "USD Coin",
            "2",
            8453,
            address!("833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"),
        )
        .await
        .unwrap();

        assert!(result.signature_hex.starts_with("0x"));
        // 65-byte signature = 130 hex chars + "0x" prefix
        assert_eq!(result.signature_hex.len(), 132);
        assert_eq!(result.authorization.from, from.to_checksum(None));
        assert_eq!(result.authorization.to, to.to_checksum(None));
    }
}
