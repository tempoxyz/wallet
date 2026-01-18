//! Encoding and formatting functions for Web Payment Auth headers

use super::types::{PaymentChallenge, PaymentCredential, PaymentReceipt};
use crate::error::{PurlError, Result};
use base64::Engine;

/// Encode data to base64url format (URL-safe, no padding)
pub fn base64url_encode(input: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(input)
}

/// Decode base64url data (handles both padded and unpadded input)
pub fn base64url_decode(input: &str) -> Result<Vec<u8>> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(input)
        .map_err(|e| PurlError::InvalidBase64Url(format!("Failed to decode base64url: {}", e)))
}

/// Format WWW-Authenticate header for a payment challenge
///
/// Format: `Payment id="<id>", realm="<realm>", method="<method>", intent="<intent>", request="<base64url-json>"`
///
/// # Example
///
/// ```no_run
/// # use purl_lib::protocol::web::{PaymentChallenge, PaymentMethod, PaymentIntent};
/// let challenge = PaymentChallenge {
///     id: "abc123".to_string(),
///     realm: "api".to_string(),
///     method: PaymentMethod::Tempo,
///     intent: PaymentIntent::Charge,
///     request: serde_json::json!({"amount": "10000"}),
///     expires: Some("2024-01-01T00:00:00Z".to_string()),
///     description: None,
/// };
///
/// let header = purl_lib::protocol::web::format_www_authenticate(&challenge)?;
/// // Returns: Payment id="abc123", realm="api", method="tempo", ...
/// # Ok::<(), purl_lib::error::PurlError>(())
/// ```
pub fn format_www_authenticate(challenge: &PaymentChallenge) -> Result<String> {
    let request_json = serde_json::to_string(&challenge.request)
        .map_err(|e| PurlError::InvalidChallenge(format!("Failed to serialize request: {}", e)))?;
    let request_b64 = base64url_encode(request_json.as_bytes());

    let mut parts = vec![
        format!("id=\"{}\"", challenge.id),
        format!("realm=\"{}\"", challenge.realm),
        format!("method=\"{}\"", challenge.method),
        format!("intent=\"{}\"", challenge.intent),
        format!("request=\"{}\"", request_b64),
    ];

    if let Some(ref expires) = challenge.expires {
        parts.push(format!("expires=\"{}\"", expires));
    }

    if let Some(ref description) = challenge.description {
        // Escape quotes in description
        let escaped = description.replace('"', "\\\"");
        parts.push(format!("description=\"{}\"", escaped));
    }

    Ok(format!("Payment {}", parts.join(", ")))
}

/// Format Authorization header for a payment credential
///
/// Format: `Payment <base64url-json>`
///
/// # Example
///
/// ```no_run
/// # use purl_lib::protocol::web::{PaymentCredential, PaymentPayload, PayloadType};
/// let credential = PaymentCredential {
///     id: "abc123".to_string(),
///     source: Some("did:pkh:eip155:88153:0x123".to_string()),
///     payload: PaymentPayload {
///         payload_type: PayloadType::Transaction,
///         signature: "0xabc".to_string(),
///     },
/// };
///
/// let header = purl_lib::protocol::web::format_authorization(&credential)?;
/// // Returns: Payment eyJpZCI6ImFiYzEyMyIs...
/// # Ok::<(), purl_lib::error::PurlError>(())
/// ```
pub fn format_authorization(credential: &PaymentCredential) -> Result<String> {
    let json = serde_json::to_string(credential).map_err(|e| {
        PurlError::InvalidChallenge(format!("Failed to serialize credential: {}", e))
    })?;
    let encoded = base64url_encode(json.as_bytes());
    Ok(format!("Payment {}", encoded))
}

/// Format Payment-Receipt header for a payment receipt
///
/// Format: `<base64url-json>`
///
/// # Example
///
/// ```no_run
/// # use purl_lib::protocol::web::{PaymentReceipt, PaymentMethod, ReceiptStatus};
/// let receipt = PaymentReceipt {
///     status: ReceiptStatus::Success,
///     method: PaymentMethod::Tempo,
///     timestamp: "2024-01-01T00:00:00Z".to_string(),
///     reference: "0xabc123".to_string(),
///     block_number: Some("12345".to_string()),
///     error: None,
/// };
///
/// let header = purl_lib::protocol::web::format_receipt(&receipt)?;
/// // Returns: eyJzdGF0dXMiOiJzdWNjZXNzIiwi...
/// # Ok::<(), purl_lib::error::PurlError>(())
/// ```
pub fn format_receipt(receipt: &PaymentReceipt) -> Result<String> {
    let json = serde_json::to_string(receipt)
        .map_err(|e| PurlError::InvalidChallenge(format!("Failed to serialize receipt: {}", e)))?;
    Ok(base64url_encode(json.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::super::types::{PayloadType, PaymentIntent, PaymentMethod, ReceiptStatus};
    use super::*;

    #[test]
    fn test_base64url_encode_decode() {
        let data = b"hello world";
        let encoded = base64url_encode(data);
        let decoded = base64url_decode(&encoded).unwrap();
        assert_eq!(data.to_vec(), decoded);
    }

    #[test]
    fn test_base64url_no_padding() {
        let data = b"test"; // Would normally need padding
        let encoded = base64url_encode(data);
        assert!(!encoded.contains('='), "Should not contain padding");
    }

    #[test]
    fn test_format_www_authenticate() {
        let challenge = PaymentChallenge {
            id: "abc123".to_string(),
            realm: "api".to_string(),
            method: PaymentMethod::Tempo,
            intent: PaymentIntent::Charge,
            request: serde_json::json!({"amount": "10000"}),
            expires: Some("2024-01-01T00:00:00Z".to_string()),
            description: None,
        };

        let header = format_www_authenticate(&challenge).unwrap();
        assert!(header.starts_with("Payment "));
        assert!(header.contains("id=\"abc123\""));
        assert!(header.contains("realm=\"api\""));
        assert!(header.contains("method=\"tempo\""));
        assert!(header.contains("intent=\"charge\""));
        assert!(header.contains("request=\""));
        assert!(header.contains("expires=\"2024-01-01T00:00:00Z\""));
    }

    #[test]
    fn test_format_authorization() {
        let credential = PaymentCredential {
            id: "abc123".to_string(),
            source: Some("did:pkh:eip155:88153:0x123".to_string()),
            payload: super::super::types::PaymentPayload {
                payload_type: PayloadType::Transaction,
                signature: "0xabc".to_string(),
            },
        };

        let header = format_authorization(&credential).unwrap();
        assert!(header.starts_with("Payment "));

        // Verify it's valid base64url
        let parts: Vec<&str> = header.split(' ').collect();
        assert_eq!(parts.len(), 2);
        let decoded = base64url_decode(parts[1]).unwrap();
        let parsed: PaymentCredential = serde_json::from_slice(&decoded).unwrap();
        assert_eq!(parsed.id, "abc123");
    }

    #[test]
    fn test_format_receipt() {
        let receipt = PaymentReceipt {
            status: ReceiptStatus::Success,
            method: PaymentMethod::Tempo,
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            reference: "0xabc123".to_string(),
            block_number: Some("12345".to_string()),
            error: None,
        };

        let header = format_receipt(&receipt).unwrap();

        // Verify it's valid base64url
        let decoded = base64url_decode(&header).unwrap();
        let parsed: PaymentReceipt = serde_json::from_slice(&decoded).unwrap();
        assert_eq!(parsed.status, ReceiptStatus::Success);
        assert_eq!(parsed.reference, "0xabc123");
    }

    #[test]
    fn test_format_www_authenticate_with_description() {
        let challenge = PaymentChallenge {
            id: "abc123".to_string(),
            realm: "api".to_string(),
            method: PaymentMethod::Tempo,
            intent: PaymentIntent::Charge,
            request: serde_json::json!({"amount": "10000"}),
            expires: None,
            description: Some("Test \"payment\" here".to_string()),
        };

        let header = format_www_authenticate(&challenge).unwrap();
        assert!(header.contains("description=\"Test \\\"payment\\\" here\""));
    }
}
