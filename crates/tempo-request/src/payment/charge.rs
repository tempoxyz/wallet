//! MPP charge payment handling.
//!
//! This module handles the MPP protocol (<https://mpp.dev>) which uses
//! WWW-Authenticate and Authorization headers for HTTP-native payments.

use anyhow::{Context, Result};
use mpp::client::PaymentProvider;

use crate::http::{HttpClient, HttpResponse};
use tempo_common::error::{ConfigError, PaymentError};
use tempo_common::keys::Signer;

use super::error::{classify_payment_error, map_mpp_validation_error};
use super::router::{PaymentResult, ResolvedChallenge};

/// Handle an MPP charge payment flow (402 with intent="charge").
///
/// Validates the challenge, builds and signs the transaction,
/// submits the payment, and returns the result.
pub(super) async fn handle_charge_request(
    http: &HttpClient,
    url: &str,
    resolved: ResolvedChallenge,
    signer: Signer,
) -> Result<PaymentResult> {
    let challenge = &resolved.challenge;

    challenge
        .validate_for_charge("tempo")
        .map_err(|e| map_mpp_validation_error(e, challenge))?;

    let provider =
        mpp::client::TempoProvider::new(signer.signer.clone(), resolved.rpc_url.as_str())
            .map_err(|e| ConfigError::Invalid(e.to_string()))?
            .with_signing_mode(signer.signing_mode);

    let credential = provider
        .pay(challenge)
        .await
        .map_err(|e| classify_payment_error(e, &resolved.network_id))?;

    let auth_header =
        mpp::format_authorization(&credential).context("Failed to format Authorization header")?;

    if http.dry_run {
        eprintln!("[DRY RUN] Signed transaction ready, skipping submission.");
        return Ok(PaymentResult {
            tx_hash: String::new(),
            session_id: None,
            status_code: 200,
            response: None,
        });
    }

    if http.log_enabled() {
        eprintln!("Submitting payment...");
    }

    let headers = vec![("Authorization".to_string(), auth_header)];
    let resp = http.execute(url, &headers).await?;

    if resp.status_code >= 400 {
        return Err(parse_payment_rejection(&resp).into());
    }

    if http.log_enabled() {
        eprintln!("Payment accepted: HTTP {}", resp.status_code);
    }

    let tx_hash = resp
        .header("payment-receipt")
        .and_then(|h| {
            mpp::protocol::core::extract_tx_hash(h)
                .or_else(|| mpp::parse_receipt(h).ok().map(|r| r.reference))
        })
        .unwrap_or_default();

    Ok(PaymentResult {
        tx_hash,
        session_id: None,
        status_code: resp.status_code,
        response: Some(resp),
    })
}

/// Parse a non-200 response after payment submission into a descriptive error.
fn parse_payment_rejection(response: &HttpResponse) -> PaymentError {
    let reason = if let Ok(body) = response.body_string() {
        if let Some(msg) = super::error::extract_json_error(&body) {
            msg
        } else if serde_json::from_str::<serde_json::Value>(&body).is_ok() {
            // Valid JSON but no known error field
            format!("HTTP {}", response.status_code)
        } else if !body.trim().is_empty() {
            body.chars().take(200).collect()
        } else {
            format!("HTTP {}", response.status_code)
        }
    } else {
        format!("HTTP {}", response.status_code)
    };

    PaymentError::PaymentRejected {
        reason,
        status_code: response.status_code,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_payment_rejection_json_error_field() {
        let body = br#"{"error":"insufficient funds"}"#;
        let resp = HttpResponse::for_test(400, body);
        let err = parse_payment_rejection(&resp);
        match err {
            PaymentError::PaymentRejected {
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
        let resp = HttpResponse::for_test(400, body);
        let err = parse_payment_rejection(&resp);
        match err {
            PaymentError::PaymentRejected { reason, .. } => {
                assert_eq!(reason, "bad request");
            }
            _ => panic!("expected PaymentRejected"),
        }
    }

    #[test]
    fn test_parse_payment_rejection_json_detail_field() {
        let body = br#"{"detail":"validation failed"}"#;
        let resp = HttpResponse::for_test(422, body);
        let err = parse_payment_rejection(&resp);
        match err {
            PaymentError::PaymentRejected {
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
        let resp = HttpResponse::for_test(500, body);
        let err = parse_payment_rejection(&resp);
        match err {
            PaymentError::PaymentRejected { reason, .. } => {
                assert_eq!(reason, "HTTP 500");
            }
            _ => panic!("expected PaymentRejected"),
        }
    }

    #[test]
    fn test_parse_payment_rejection_json_error_precedence() {
        let body = br#"{"error":"e","message":"m","detail":"d"}"#;
        let resp = HttpResponse::for_test(400, body);
        let err = parse_payment_rejection(&resp);
        match err {
            PaymentError::PaymentRejected { reason, .. } => {
                assert_eq!(reason, "e");
            }
            _ => panic!("expected PaymentRejected"),
        }
    }

    #[test]
    fn test_parse_payment_rejection_plain_text() {
        let body = b"Transaction reverted";
        let resp = HttpResponse::for_test(500, body);
        let err = parse_payment_rejection(&resp);
        match err {
            PaymentError::PaymentRejected { reason, .. } => {
                assert_eq!(reason, "Transaction reverted");
            }
            _ => panic!("expected PaymentRejected"),
        }
    }

    #[test]
    fn test_parse_payment_rejection_plain_text_truncated() {
        let body = "a".repeat(500);
        let resp = HttpResponse::for_test(500, body.as_bytes());
        let err = parse_payment_rejection(&resp);
        match err {
            PaymentError::PaymentRejected { reason, .. } => {
                assert_eq!(reason.len(), 200);
            }
            _ => panic!("expected PaymentRejected"),
        }
    }

    #[test]
    fn test_parse_payment_rejection_empty_body() {
        let resp = HttpResponse::for_test(500, b"");
        let err = parse_payment_rejection(&resp);
        match err {
            PaymentError::PaymentRejected { reason, .. } => {
                assert_eq!(reason, "HTTP 500");
            }
            _ => panic!("expected PaymentRejected"),
        }
    }

    #[test]
    fn test_parse_payment_rejection_whitespace_body() {
        let resp = HttpResponse::for_test(503, b"   \n\t  ");
        let err = parse_payment_rejection(&resp);
        match err {
            PaymentError::PaymentRejected { reason, .. } => {
                assert_eq!(reason, "HTTP 503");
            }
            _ => panic!("expected PaymentRejected"),
        }
    }

    #[test]
    fn test_parse_payment_rejection_invalid_utf8() {
        let resp = HttpResponse::for_test(500, &[0xff, 0xfe, 0xfd]);
        let err = parse_payment_rejection(&resp);
        match err {
            PaymentError::PaymentRejected { reason, .. } => {
                assert_eq!(reason, "HTTP 500");
            }
            _ => panic!("expected PaymentRejected"),
        }
    }
}
