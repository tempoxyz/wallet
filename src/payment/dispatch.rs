//! Payment dispatch: route 402 flows to charge or session payment paths.
//!
//! This module is crate-internal and intentionally decoupled from CLI types.

use anyhow::Result;

use crate::config::Config;
use crate::error::PrestoError;
use crate::http::{HttpClient, HttpResponse, RequestContext};

use super::charge::prepare_charge;
use super::session::{handle_session_request, SessionResult};

/// Result of a successful payment dispatch.
pub(crate) struct PaymentResult {
    pub tx_hash: String,
    pub session_id: Option<String>,
    pub status_code: u16,
    pub response: Option<HttpResponse>,
}

/// Dispatch to charge or session payment flow.
pub(crate) async fn dispatch_payment(
    config: &Config,
    request_ctx: &RequestContext,
    http_client: &HttpClient,
    is_session: bool,
    url: &str,
    response: &HttpResponse,
) -> Result<PaymentResult> {
    if is_session {
        let result =
            handle_session_request(config, request_ctx, http_client, url, response).await?;
        return match result {
            SessionResult::Streamed { channel_id } => Ok(PaymentResult {
                tx_hash: String::new(),
                session_id: Some(channel_id),
                status_code: 200,
                response: None,
            }),
            SessionResult::Response {
                response: resp,
                channel_id,
            } => Ok(PaymentResult {
                tx_hash: String::new(),
                session_id: Some(channel_id),
                status_code: resp.status_code,
                response: Some(resp),
            }),
        };
    }

    let auth_header = prepare_charge(config, &request_ctx.runtime, response).await?;

    if request_ctx.runtime.dry_run {
        eprintln!("[DRY RUN] Signed transaction ready, skipping submission.");
        return Ok(PaymentResult {
            tx_hash: String::new(),
            session_id: None,
            status_code: 200,
            response: None,
        });
    }

    if request_ctx.log_enabled() {
        eprintln!("Submitting payment...");
    }

    let headers = vec![("Authorization".to_string(), (*auth_header).clone())];
    let resp = request_ctx
        .execute_with_client(http_client, url, &headers)
        .await?;

    if resp.status_code >= 400 {
        return Err(parse_payment_rejection(&resp).into());
    }

    if request_ctx.log_enabled() {
        eprintln!("Payment accepted: HTTP {}", resp.status_code);
    }

    // Extract a raw transaction reference (hex hash) for analytics if present
    let tx_hash = resp
        .get_header("payment-receipt")
        .and_then(|h| {
            mpp::protocol::core::extract_tx_hash(h)
                .or_else(|| mpp::parse_receipt(h).ok().map(|r| r.reference))
        })
        .unwrap_or_default();
    let status_code = resp.status_code;
    Ok(PaymentResult {
        tx_hash,
        session_id: None,
        status_code,
        response: Some(resp),
    })
}

/// Parse a non-200 response after payment submission into a descriptive error.
pub(crate) fn parse_payment_rejection(response: &HttpResponse) -> PrestoError {
    let reason = if let Ok(body) = response.body_string() {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
            if let Some(error) = json.get("error").and_then(|e| e.as_str()) {
                error.to_string()
            } else if let Some(message) = json.get("message").and_then(|m| m.as_str()) {
                message.to_string()
            } else if let Some(detail) = json.get("detail").and_then(|d| d.as_str()) {
                detail.to_string()
            } else {
                format!("HTTP {}", response.status_code)
            }
        } else if !body.trim().is_empty() {
            body.chars().take(200).collect()
        } else {
            format!("HTTP {}", response.status_code)
        }
    } else {
        format!("HTTP {}", response.status_code)
    };

    PrestoError::PaymentRejected {
        reason,
        status_code: response.status_code,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_response(status: u16, body: &[u8]) -> HttpResponse {
        HttpResponse {
            status_code: status,
            headers: std::collections::HashMap::new(),
            body: body.to_vec(),
            final_url: None,
        }
    }

    #[test]
    fn test_parse_payment_rejection_json_error_field() {
        let body = br#"{"error":"insufficient funds"}"#;
        let resp = make_response(400, body);
        let err = parse_payment_rejection(&resp);
        match err {
            PrestoError::PaymentRejected {
                reason,
                status_code,
            } => {
                assert_eq!(reason, "insufficient funds");
                assert_eq!(status_code, 400);
            }
            _ => panic!("expected PaymentRejected"),
        }
    }

    #[test]
    fn test_parse_payment_rejection_json_message_field() {
        let body = br#"{"message":"bad request"}"#;
        let resp = make_response(400, body);
        let err = parse_payment_rejection(&resp);
        match err {
            PrestoError::PaymentRejected { reason, .. } => {
                assert_eq!(reason, "bad request");
            }
            _ => panic!("expected PaymentRejected"),
        }
    }

    #[test]
    fn test_parse_payment_rejection_json_detail_field() {
        let body = br#"{"detail":"validation failed"}"#;
        let resp = make_response(422, body);
        let err = parse_payment_rejection(&resp);
        match err {
            PrestoError::PaymentRejected {
                reason,
                status_code,
            } => {
                assert_eq!(reason, "validation failed");
                assert_eq!(status_code, 422);
            }
            _ => panic!("expected PaymentRejected"),
        }
    }

    #[test]
    fn test_parse_payment_rejection_json_no_known_field() {
        let body = br#"{"foo":"bar"}"#;
        let resp = make_response(500, body);
        let err = parse_payment_rejection(&resp);
        match err {
            PrestoError::PaymentRejected { reason, .. } => {
                assert_eq!(reason, "HTTP 500");
            }
            _ => panic!("expected PaymentRejected"),
        }
    }

    #[test]
    fn test_parse_payment_rejection_json_error_precedence() {
        let body = br#"{"error":"e","message":"m","detail":"d"}"#;
        let resp = make_response(400, body);
        let err = parse_payment_rejection(&resp);
        match err {
            PrestoError::PaymentRejected { reason, .. } => {
                assert_eq!(reason, "e");
            }
            _ => panic!("expected PaymentRejected"),
        }
    }

    #[test]
    fn test_parse_payment_rejection_plain_text() {
        let body = b"Transaction reverted";
        let resp = make_response(500, body);
        let err = parse_payment_rejection(&resp);
        match err {
            PrestoError::PaymentRejected { reason, .. } => {
                assert_eq!(reason, "Transaction reverted");
            }
            _ => panic!("expected PaymentRejected"),
        }
    }

    #[test]
    fn test_parse_payment_rejection_plain_text_truncated() {
        let body = "a".repeat(500);
        let resp = make_response(500, body.as_bytes());
        let err = parse_payment_rejection(&resp);
        match err {
            PrestoError::PaymentRejected { reason, .. } => {
                assert_eq!(reason.len(), 200);
            }
            _ => panic!("expected PaymentRejected"),
        }
    }

    #[test]
    fn test_parse_payment_rejection_empty_body() {
        let resp = make_response(500, b"");
        let err = parse_payment_rejection(&resp);
        match err {
            PrestoError::PaymentRejected { reason, .. } => {
                assert_eq!(reason, "HTTP 500");
            }
            _ => panic!("expected PaymentRejected"),
        }
    }

    #[test]
    fn test_parse_payment_rejection_whitespace_body() {
        let resp = make_response(503, b"   \n\t  ");
        let err = parse_payment_rejection(&resp);
        match err {
            PrestoError::PaymentRejected { reason, .. } => {
                assert_eq!(reason, "HTTP 503");
            }
            _ => panic!("expected PaymentRejected"),
        }
    }

    #[test]
    fn test_parse_payment_rejection_invalid_utf8() {
        let resp = make_response(500, &[0xff, 0xfe, 0xfd]);
        let err = parse_payment_rejection(&resp);
        match err {
            PrestoError::PaymentRejected { reason, .. } => {
                assert_eq!(reason, "HTTP 500");
            }
            _ => panic!("expected PaymentRejected"),
        }
    }
}
