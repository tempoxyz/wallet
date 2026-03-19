//! x402 `PAYMENT-SIGNATURE` header construction.

use base64::{engine::general_purpose::STANDARD, Engine};

use super::{
    eip3009::SignedAuthorization,
    types::{X402PaymentPayloadV1, X402PaymentPayloadV2, X402SignedPayload},
};

/// Build the base64-encoded value for the `PAYMENT-SIGNATURE` header.
///
/// v1: top-level `scheme` + `network` fields.
/// v2: top-level `resource` + `accepted` fields.
pub(super) fn build_payment_signature_header(
    x402_version: u32,
    scheme: &str,
    network: &str,
    resource: Option<serde_json::Value>,
    accepted: serde_json::Value,
    signed: SignedAuthorization,
) -> String {
    let inner = X402SignedPayload {
        signature: signed.signature_hex,
        authorization: signed.authorization,
    };

    let json = if x402_version >= 2 {
        // v2: include resource + accepted
        let payload = X402PaymentPayloadV2 {
            x402_version,
            resource: resource.unwrap_or(serde_json::Value::Null),
            accepted,
            payload: inner,
        };
        serde_json::to_string(&payload).expect("X402PaymentPayloadV2 is always serializable")
    } else {
        // v1: include scheme + network
        let payload = X402PaymentPayloadV1 {
            x402_version,
            scheme: scheme.to_string(),
            network: network.to_string(),
            payload: inner,
        };
        serde_json::to_string(&payload).expect("X402PaymentPayloadV1 is always serializable")
    };

    STANDARD.encode(json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::payment::x402::types::X402Authorization;

    fn test_authorization() -> SignedAuthorization {
        SignedAuthorization {
            signature_hex: "0xdeadbeef".to_string(),
            authorization: X402Authorization {
                from: "0xaaa".to_string(),
                to: "0xbbb".to_string(),
                value: "1000000".to_string(),
                valid_after: "0".to_string(),
                valid_before: "999999".to_string(),
                nonce: "0x00".to_string(),
            },
        }
    }

    #[test]
    fn test_v2_payload_has_resource_and_accepted() {
        let header = build_payment_signature_header(
            2,
            "exact",
            "eip155:8453",
            Some(serde_json::json!({"url": "https://example.com"})),
            serde_json::json!({"scheme": "exact"}),
            test_authorization(),
        );

        let decoded = STANDARD.decode(&header).unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
        assert_eq!(parsed["x402Version"], 2);
        assert_eq!(parsed["resource"]["url"], "https://example.com");
        assert_eq!(parsed["accepted"]["scheme"], "exact");
        assert_eq!(parsed["payload"]["signature"], "0xdeadbeef");
        // v2 must not have scheme/network at top level
        assert!(parsed.get("scheme").is_none());
        assert!(parsed.get("network").is_none());
    }

    #[test]
    fn test_v1_payload_has_scheme_and_network() {
        let header = build_payment_signature_header(
            1,
            "exact",
            "base",
            None,
            serde_json::json!({"scheme": "exact"}),
            test_authorization(),
        );

        let decoded = STANDARD.decode(&header).unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
        assert_eq!(parsed["x402Version"], 1);
        assert_eq!(parsed["scheme"], "exact");
        assert_eq!(parsed["network"], "base");
        assert_eq!(parsed["payload"]["signature"], "0xdeadbeef");
        // v1 must not have resource/accepted at top level
        assert!(parsed.get("resource").is_none());
        assert!(parsed.get("accepted").is_none());
    }
}
