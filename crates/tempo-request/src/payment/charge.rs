//! MPP charge payment handling.
//!
//! This module handles the MPP protocol (<https://mpp.dev>) which uses
//! WWW-Authenticate and Authorization headers for HTTP-native payments.

use mpp::client::PaymentProvider;

use crate::http::{HttpClient, HttpResponse};
use tempo_common::{
    cli::terminal::sanitize_for_terminal,
    error::{ConfigError, PaymentError, TempoError},
    keys::Signer,
    payment::session::KeyStatus,
};

use super::{
    lock::{acquire_origin_lock, origin_lock_key},
    types::{PaymentResult, ResolvedChallenge},
};
use tempo_common::payment::{classify_payment_error, map_mpp_validation_error};

/// Handle an MPP charge payment flow (402 with intent="charge").
///
/// Validates the challenge, builds and signs the transaction,
/// submits the payment, and returns the result.
pub(super) async fn handle_charge_request(
    http: &HttpClient,
    url: &str,
    resolved: ResolvedChallenge,
    signer: Signer,
) -> Result<PaymentResult, TempoError> {
    let challenge = &resolved.challenge;

    challenge
        .validate_for_charge("tempo")
        .map_err(|e| map_mpp_validation_error(e, challenge))?;

    if http.dry_run {
        eprintln!("[DRY RUN] Signed transaction ready, skipping submission.");
        return Ok(PaymentResult {
            tx_hash: None,
            channel_id: None,
            status_code: 200,
            response: None,
        });
    }

    // Serialize charge submissions per origin before building/submitting payment
    // credentials to avoid duplicate expiring-nonce tx races under overlap.
    let lock_key = origin_lock_key(url);
    let _charge_lock = acquire_origin_lock(&lock_key)?;

    let provider =
        mpp::client::TempoProvider::new(signer.signer.clone(), resolved.rpc_url.as_str())
            .map_err(|source| ConfigError::ProviderInitSource {
                provider: "tempo payment provider",
                source: Box::new(source),
            })?
            .with_signing_mode(signer.signing_mode.clone());

    let credential =
        match provider.pay(challenge).await {
            Ok(cred) => cred,
            Err(e) if signer.has_stored_key_authorization() => {
                // Payment failed — check on-chain if the key is definitively missing.
                // Only retry with key_authorization if the key hasn't been provisioned.
                let rpc_url: url::Url = resolved.rpc_url.as_str().parse().map_err(|source| {
                    ConfigError::InvalidUrl {
                        context: "RPC",
                        source,
                    }
                })?;
                let rpc_provider =
                    alloy::providers::RootProvider::<mpp::client::TempoNetwork>::new_http(rpc_url);
                let status = tempo_common::payment::session::query_key_status(
                    &rpc_provider,
                    signer.from,
                    signer.signer.address(),
                )
                .await;

                if matches!(status, KeyStatus::Missing) {
                    let provisioning_signer = signer.with_key_authorization().unwrap();
                    let retry_provider = mpp::client::TempoProvider::new(
                        provisioning_signer.signer.clone(),
                        resolved.rpc_url.as_str(),
                    )
                    .map_err(|source| ConfigError::ProviderInitSource {
                        provider: "tempo payment provider (provisioning retry)",
                        source: Box::new(source),
                    })?
                    .with_signing_mode(provisioning_signer.signing_mode);
                    retry_provider
                        .pay(challenge)
                        .await
                        .map_err(|e| classify_payment_error(e, &resolved.network_id))?
                } else {
                    return Err(classify_payment_error(e, &resolved.network_id));
                }
            }
            Err(e) => return Err(classify_payment_error(e, &resolved.network_id)),
        };

    let auth_header = mpp::format_authorization(&credential).map_err(|source| {
        PaymentError::ChallengeFormatSource {
            context: "Authorization header",
            source: Box::new(source),
        }
    })?;

    let headers = vec![("Authorization".to_string(), auth_header)];
    let resp = http.execute(url, &headers).await?;

    if resp.status_code >= 400 {
        return Err(parse_payment_rejection(&resp).into());
    }

    let tx_hash = match resp.header("payment-receipt") {
        Some(header) => {
            if let Err(source) = mpp::parse_receipt(header) {
                eprintln!(
                    "Warning: ignoring invalid Payment-Receipt on paid charge response: {source}"
                );
            }
            mpp::protocol::core::extract_tx_hash(header)
                .or_else(|| mpp::parse_receipt(header).ok().map(|r| r.reference))
        }
        None => {
            eprintln!("Warning: missing Payment-Receipt on successful paid charge response");
            None
        }
    };

    Ok(PaymentResult {
        tx_hash,
        channel_id: None,
        status_code: resp.status_code,
        response: Some(resp),
    })
}

/// Parse a non-200 response after payment submission into a descriptive error.
fn parse_payment_rejection(response: &HttpResponse) -> PaymentError {
    let raw_reason = if let Ok(body) = response.body_string() {
        if let Some(msg) = tempo_common::payment::extract_json_error(&body) {
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
    let reason = sanitize_for_terminal(&raw_reason);

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
