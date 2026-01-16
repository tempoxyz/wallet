//! Parsing functions for Web Payment Auth headers

use crate::error::{PurlError, Result};
use crate::web::encode::base64url_decode;
use crate::web::types::{
    PaymentChallenge, PaymentCredential, PaymentIntent, PaymentMethod, PaymentReceipt,
};
use regex::Regex;
use std::sync::OnceLock;

/// Get regex for parsing WWW-Authenticate header parameters
fn www_authenticate_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        // Match key="value" pairs, handling escaped quotes in values
        Regex::new(r#"(\w+)="([^"\\]*(\\.[^"\\]*)*)""#).unwrap()
    })
}

/// Parse WWW-Authenticate header
///
/// Format: `Payment id="<id>", realm="<realm>", method="<method>", intent="<intent>", request="<base64url-json>"`
///
/// # Example
///
/// ```no_run
/// # use purl_lib::web::parse_www_authenticate;
/// let header = r#"Payment id="abc123", realm="api", method="tempo", intent="charge", request="eyJhbW91bnQiOiIxMDAwMCJ9""#;
/// let challenge = parse_www_authenticate(header)?;
/// assert_eq!(challenge.id, "abc123");
/// # Ok::<(), purl_lib::error::PurlError>(())
/// ```
pub fn parse_www_authenticate(header: &str) -> Result<PaymentChallenge> {
    // Verify scheme
    if !header.starts_with("Payment ") {
        return Err(PurlError::InvalidChallenge(format!(
            "Expected 'Payment' scheme, got: {}",
            header
        )));
    }

    let params_str = &header["Payment ".len()..];
    let regex = www_authenticate_regex();

    // Extract key-value pairs
    let mut params = std::collections::HashMap::new();
    for cap in regex.captures_iter(params_str) {
        let key = cap[1].to_string();
        let value = cap[2].replace("\\\"", "\""); // Unescape quotes
        params.insert(key, value);
    }

    // Extract required fields
    let id = params
        .get("id")
        .ok_or_else(|| PurlError::InvalidChallenge("Missing 'id' field".to_string()))?
        .clone();

    let realm = params
        .get("realm")
        .ok_or_else(|| PurlError::InvalidChallenge("Missing 'realm' field".to_string()))?
        .clone();

    let method_str = params
        .get("method")
        .ok_or_else(|| PurlError::InvalidChallenge("Missing 'method' field".to_string()))?;
    let method: PaymentMethod =
        serde_json::from_value(serde_json::Value::String(method_str.clone()))
            .map_err(|e| PurlError::InvalidChallenge(format!("Invalid method: {}", e)))?;

    let intent_str = params
        .get("intent")
        .ok_or_else(|| PurlError::InvalidChallenge("Missing 'intent' field".to_string()))?;
    let intent: PaymentIntent =
        serde_json::from_value(serde_json::Value::String(intent_str.clone()))
            .map_err(|e| PurlError::InvalidChallenge(format!("Invalid intent: {}", e)))?;

    let request_b64 = params
        .get("request")
        .ok_or_else(|| PurlError::InvalidChallenge("Missing 'request' field".to_string()))?;
    let request_bytes = base64url_decode(request_b64)?;
    let request: serde_json::Value = serde_json::from_slice(&request_bytes)
        .map_err(|e| PurlError::InvalidChallenge(format!("Invalid request JSON: {}", e)))?;

    let expires = params.get("expires").cloned();
    let description = params.get("description").cloned();

    Ok(PaymentChallenge {
        id,
        realm,
        method,
        intent,
        request,
        expires,
        description,
    })
}

/// Parse Authorization header
///
/// Format: `Payment <base64url-json>`
///
/// # Example
///
/// ```no_run
/// # use purl_lib::web::parse_authorization;
/// let header = "Payment eyJpZCI6ImFiYzEyMyIsInNvdXJjZSI6ImRpZDpwa2g6ZWlwMTU1Ojg4MTUzOjB4MTIzIiwicGF5bG9hZCI6eyJ0eXBlIjoidHJhbnNhY3Rpb24iLCJzaWduYXR1cmUiOiIweGFiYyJ9fQ";
/// let credential = parse_authorization(header)?;
/// assert_eq!(credential.id, "abc123");
/// # Ok::<(), purl_lib::error::PurlError>(())
/// ```
pub fn parse_authorization(header: &str) -> Result<PaymentCredential> {
    // Verify scheme
    if !header.starts_with("Payment ") {
        return Err(PurlError::InvalidChallenge(format!(
            "Expected 'Payment' scheme, got: {}",
            header
        )));
    }

    let token = &header["Payment ".len()..].trim();
    let decoded = base64url_decode(token)?;
    let credential: PaymentCredential = serde_json::from_slice(&decoded)
        .map_err(|e| PurlError::InvalidChallenge(format!("Invalid credential JSON: {}", e)))?;

    Ok(credential)
}

/// Parse Payment-Receipt header
///
/// Format: `<base64url-json>`
///
/// # Example
///
/// ```no_run
/// # use purl_lib::web::parse_receipt;
/// let header = "eyJzdGF0dXMiOiJzdWNjZXNzIiwibWV0aG9kIjoidGVtcG8iLCJ0aW1lc3RhbXAiOiIyMDI0LTAxLTAxVDAwOjAwOjAwWiIsInJlZmVyZW5jZSI6IjB4YWJjMTIzIn0";
/// let receipt = parse_receipt(header)?;
/// # Ok::<(), purl_lib::error::PurlError>(())
/// ```
pub fn parse_receipt(header: &str) -> Result<PaymentReceipt> {
    let decoded = base64url_decode(header)?;
    let receipt: PaymentReceipt = serde_json::from_slice(&decoded)
        .map_err(|e| PurlError::InvalidChallenge(format!("Invalid receipt JSON: {}", e)))?;

    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::encode::{format_authorization, format_receipt, format_www_authenticate};
    use crate::web::types::{PayloadType, PaymentPayload, ReceiptStatus};

    #[test]
    fn test_parse_www_authenticate() {
        let challenge = PaymentChallenge {
            id: "abc123".to_string(),
            realm: "api".to_string(),
            method: PaymentMethod::Tempo,
            intent: PaymentIntent::Charge,
            request: serde_json::json!({"amount": "10000", "asset": "0x123"}),
            expires: Some("2024-01-01T00:00:00Z".to_string()),
            description: None,
        };

        let header = format_www_authenticate(&challenge).unwrap();
        let parsed = parse_www_authenticate(&header).unwrap();

        assert_eq!(parsed.id, "abc123");
        assert_eq!(parsed.realm, "api");
        assert_eq!(parsed.method, PaymentMethod::Tempo);
        assert_eq!(parsed.intent, PaymentIntent::Charge);
        assert_eq!(parsed.request["amount"], "10000");
        assert_eq!(parsed.expires, Some("2024-01-01T00:00:00Z".to_string()));
    }

    #[test]
    fn test_parse_www_authenticate_with_description() {
        let challenge = PaymentChallenge {
            id: "test123".to_string(),
            realm: "test".to_string(),
            method: PaymentMethod::Base,
            intent: PaymentIntent::Authorize,
            request: serde_json::json!({}),
            expires: None,
            description: Some("Test \"quoted\" text".to_string()),
        };

        let header = format_www_authenticate(&challenge).unwrap();
        let parsed = parse_www_authenticate(&header).unwrap();

        assert_eq!(parsed.description, Some("Test \"quoted\" text".to_string()));
    }

    #[test]
    fn test_parse_authorization() {
        let credential = PaymentCredential {
            id: "abc123".to_string(),
            source: Some("did:pkh:eip155:88153:0x123".to_string()),
            payload: PaymentPayload {
                payload_type: PayloadType::Transaction,
                signature: "0xabc".to_string(),
            },
        };

        let header = format_authorization(&credential).unwrap();
        let parsed = parse_authorization(&header).unwrap();

        assert_eq!(parsed.id, "abc123");
        assert_eq!(
            parsed.source,
            Some("did:pkh:eip155:88153:0x123".to_string())
        );
        assert_eq!(parsed.payload.payload_type, PayloadType::Transaction);
        assert_eq!(parsed.payload.signature, "0xabc");
    }

    #[test]
    fn test_parse_receipt() {
        let receipt = PaymentReceipt {
            status: ReceiptStatus::Success,
            method: PaymentMethod::Tempo,
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            reference: "0xabc123".to_string(),
            block_number: Some("12345".to_string()),
            error: None,
        };

        let header = format_receipt(&receipt).unwrap();
        let parsed = parse_receipt(&header).unwrap();

        assert_eq!(parsed.status, ReceiptStatus::Success);
        assert_eq!(parsed.method, PaymentMethod::Tempo);
        assert_eq!(parsed.reference, "0xabc123");
        assert_eq!(parsed.block_number, Some("12345".to_string()));
    }

    #[test]
    fn test_parse_invalid_scheme() {
        let result = parse_www_authenticate("Basic realm=\"test\"");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PurlError::InvalidChallenge(_)
        ));
    }

    #[test]
    fn test_parse_missing_required_field() {
        let result = parse_www_authenticate("Payment id=\"abc123\", realm=\"api\"");
        assert!(result.is_err());
    }

    #[test]
    fn test_roundtrip_www_authenticate() {
        let original = PaymentChallenge {
            id: "roundtrip123".to_string(),
            realm: "roundtrip".to_string(),
            method: PaymentMethod::Custom("custom".to_string()),
            intent: PaymentIntent::Subscription,
            request: serde_json::json!({
                "amount": "5000",
                "interval": 86400,
                "nested": {
                    "key": "value"
                }
            }),
            expires: Some("2025-12-31T23:59:59Z".to_string()),
            description: Some("Complex description with symbols: @#$%".to_string()),
        };

        let header = format_www_authenticate(&original).unwrap();
        let parsed = parse_www_authenticate(&header).unwrap();

        assert_eq!(parsed.id, original.id);
        assert_eq!(parsed.realm, original.realm);
        assert_eq!(parsed.request, original.request);
        assert_eq!(parsed.expires, original.expires);
    }
}
