//! Zero-dollar proof authentication for MPP charges.
//!
//! When a server issues a charge challenge with `amount = "0"`, the client
//! signs an EIP-712 typed data message instead of creating a transaction.
//! This proves wallet ownership without moving funds.
//!
//! Matches the mppx TypeScript SDK's proof credential flow:
//! - Domain: `{ name: "MPP", version: "1", chainId }`
//! - Types: `{ Proof: [{ name: "challengeId", type: "string" }] }`
//! - Payload: `{ type: "proof", signature: "0x..." }`
//! - Source: `did:pkh:eip155:{chainId}:{address}`

use alloy::{
    primitives::Address,
    signers::Signer,
    sol_types::{eip712_domain, SolStruct},
};

use mpp::PaymentChallenge;

use tempo_common::error::{KeyError, TempoError};

alloy::sol! {
    #[derive(Debug)]
    struct Proof {
        string challengeId;
    }
}

/// Proof credential payload matching mppx's `{ type: "proof", signature: "0x..." }`.
#[derive(serde::Serialize)]
pub(super) struct ProofPayload {
    #[serde(rename = "type")]
    pub payload_type: &'static str,
    pub signature: String,
}

/// Build a proof credential for a zero-amount charge challenge.
///
/// Signs EIP-712 typed data with domain `{ name: "MPP", version: "1", chainId }`
/// and message `{ challengeId }`, then constructs a `PaymentCredential` with
/// `type: "proof"` payload and `did:pkh:eip155:{chainId}:{address}` source.
pub(super) async fn build_proof_credential(
    signer: &impl Signer,
    challenge: &PaymentChallenge,
    chain_id: u64,
    address: Address,
) -> Result<mpp::PaymentCredential, TempoError> {
    let domain = eip712_domain! {
        name: "MPP",
        version: "1",
        chain_id: chain_id,
    };

    let proof = Proof {
        challengeId: challenge.id.clone(),
    };

    let signing_hash = proof.eip712_signing_hash(&domain);
    let signature = signer.sign_hash(&signing_hash).await.map_err(|source| {
        KeyError::SigningOperationSource {
            operation: "sign proof credential",
            source: Box::new(source),
        }
    })?;

    let payload = ProofPayload {
        payload_type: "proof",
        signature: format!("0x{}", hex::encode(signature.as_bytes())),
    };

    let source = format!("did:pkh:eip155:{chain_id}:{address:#x}");

    Ok(mpp::PaymentCredential::with_source(
        challenge.to_echo(),
        source,
        payload,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_proof_credential_structure() {
        let signer = alloy::signers::local::PrivateKeySigner::random();
        let address = signer.address();
        let chain_id = 4217u64;

        let request = mpp::Base64UrlJson::from_value(&serde_json::json!({
            "amount": "0",
            "currency": "0x20c0000000000000000000000000000000000000",
            "methodDetails": { "chainId": chain_id }
        }))
        .unwrap();
        let challenge = mpp::PaymentChallenge::new(
            "test-challenge-id",
            "test-realm",
            "tempo",
            "charge",
            request,
        );

        let credential = build_proof_credential(&signer, &challenge, chain_id, address)
            .await
            .unwrap();

        // Verify payload structure
        let payload = credential.payload;
        assert_eq!(payload["type"], "proof");
        assert!(payload["signature"].as_str().unwrap().starts_with("0x"));

        // Verify source DID
        let source = credential.source.unwrap();
        assert!(source.starts_with("did:pkh:eip155:4217:0x"));
        assert!(source.contains(&format!("{address:#x}")));
    }

    #[tokio::test]
    async fn test_proof_credential_serializes_to_valid_authorization() {
        let signer = alloy::signers::local::PrivateKeySigner::random();
        let address = signer.address();

        let request = mpp::Base64UrlJson::from_value(&serde_json::json!({
            "amount": "0",
            "currency": "0x20c0000000000000000000000000000000000000",
            "methodDetails": { "chainId": 4217 }
        }))
        .unwrap();
        let challenge = mpp::PaymentChallenge::new("chal-id", "realm", "tempo", "charge", request);

        let credential = build_proof_credential(&signer, &challenge, 4217, address)
            .await
            .unwrap();

        let auth_header = mpp::format_authorization(&credential);
        assert!(
            auth_header.is_ok(),
            "credential must serialize to valid Authorization header"
        );
        assert!(auth_header.unwrap().starts_with("Payment "));
    }
}
