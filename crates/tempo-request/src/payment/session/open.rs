//! Session-open transaction building and retry logic.
//!
//! Constructs the session-open transaction and retries submission
//! when the server hasn't indexed the channel yet. Low-level signing
//! and broadcast helpers remain in `tempo_common::session::tx`.

use alloy::primitives::Address;

use crate::http::{HttpClient, HttpResponse};
use tempo_common::{
    cli::terminal::sanitize_for_terminal,
    error::{ConfigError, PaymentError, TempoError},
    keys::Signer,
    payment::{
        classify::{parse_problem_details, SessionProblemType},
        session as common_tx,
    },
};

fn should_retry_open_response(status_code: u16, body: &str) -> bool {
    if status_code != 410 {
        return false;
    }

    parse_problem_details(body)
        .is_some_and(|problem| problem.classify() == SessionProblemType::ChannelNotFound)
}

fn rejected_reason_from_body(body: &str) -> String {
    let raw_reason = tempo_common::payment::extract_json_error(body)
        .unwrap_or_else(|| body.chars().take(500).collect::<String>());
    sanitize_for_terminal(&raw_reason)
}

/// Result of building a Tempo payment from calls.
pub(super) struct TempoPaymentResult {
    pub(super) tx_bytes: Vec<u8>,
}

/// Create a Tempo payment credential from pre-built calls.
///
/// Used by session payments where the calls (e.g., approve + escrow.open)
/// are built externally. Uses expiring nonces and parallelizes fee
/// resolution with gas estimation.
///
/// Returns both the credential (for sending to the server) and the raw
/// signed transaction bytes (for optional client-side pre-broadcast).
pub(super) async fn create_tempo_payment_from_calls(
    rpc_url_str: &str,
    signing: &Signer,
    calls: Vec<tempo_primitives::transaction::Call>,
    fee_token: Address,
    chain_id: u64,
    fee_payer: bool,
) -> Result<TempoPaymentResult, TempoError> {
    let rpc_url: url::Url = rpc_url_str
        .parse()
        .map_err(|source| ConfigError::InvalidUrl {
            context: "RPC",
            source,
        })?;
    let provider = alloy::providers::RootProvider::<mpp::client::TempoNetwork>::new_http(rpc_url);

    let from = signing.from;
    let tx_bytes = common_tx::resolve_and_sign_tx_with_fee_payer(
        &provider, signing, chain_id, fee_token, from, calls, fee_payer,
    )
    .await?;

    Ok(TempoPaymentResult { tx_bytes })
}

/// Send the Open credential to the server and retry on HTTP 410 while the node indexes.
pub(super) async fn send_open_with_retry(
    http: &HttpClient,
    url: &str,
    auth_header: &str,
    idempotency_key: &str,
    delays_ms: &[u64],
) -> Result<HttpResponse, TempoError> {
    let headers = vec![
        ("Authorization".to_string(), auth_header.to_string()),
        ("Idempotency-Key".to_string(), idempotency_key.to_string()),
    ];
    let resp = http.execute(url, &headers).await?;

    if resp.status_code < 400 {
        return Ok(resp);
    }

    if resp.status_code == 410 {
        let body = resp.body_string().unwrap_or_default();
        if should_retry_open_response(resp.status_code, &body) {
            if http.log_enabled() {
                eprintln!("Server hasn't indexed channel yet, retrying...");
            }
            for delay in delays_ms {
                tokio::time::sleep(std::time::Duration::from_millis(*delay)).await;
                let next = http.execute(url, &headers).await?;
                if next.status_code < 400 {
                    return Ok(next);
                }
                let next_body = next.body_string().unwrap_or_default();
                if !should_retry_open_response(next.status_code, &next_body) {
                    let reason = rejected_reason_from_body(&next_body);
                    return Err(PaymentError::PaymentRejected {
                        reason,
                        status_code: next.status_code,
                    }
                    .into());
                }
            }
            // Intentional operator-facing retry exhaustion message; this path has
            // no richer source error beyond repeated 410 channel-not-found responses.
            return Err(PaymentError::PaymentRejected {
                reason: "Server could not find channel after retries".to_string(),
                status_code: 410,
            }
            .into());
        }
        let reason = rejected_reason_from_body(&body);
        return Err(PaymentError::PaymentRejected {
            reason,
            status_code: 410,
        }
        .into());
    }

    let body = resp.body_string().unwrap_or_default();
    let reason = rejected_reason_from_body(&body);
    Err(PaymentError::PaymentRejected {
        reason,
        status_code: resp.status_code,
    }
    .into())
}

#[cfg(test)]
mod tests {
    use super::{rejected_reason_from_body, should_retry_open_response};

    #[test]
    fn retries_only_for_channel_not_found_problem_type() {
        let body = r#"{"type":"https://paymentauth.org/problems/session/channel-not-found","detail":"channel unknown"}"#;
        assert!(should_retry_open_response(410, body));
    }

    #[test]
    fn does_not_retry_for_non_matching_problem_type() {
        let body = r#"{"type":"https://paymentauth.org/problems/session/signer-mismatch","detail":"bad signer"}"#;
        assert!(!should_retry_open_response(410, body));
    }

    #[test]
    fn does_not_retry_when_status_is_not_410() {
        let body = r#"{"type":"https://paymentauth.org/problems/session/channel-not-found"}"#;
        assert!(!should_retry_open_response(402, body));
    }

    #[test]
    fn rejected_reason_from_body_sanitizes_control_sequences() {
        let body = r#"{"error":"bad\u001b[31m\u0007value"}"#;
        let reason = rejected_reason_from_body(body);
        assert_eq!(reason, "bad[31mvalue");
        assert!(!reason.chars().any(char::is_control));
    }

    #[test]
    fn rejected_reason_from_body_truncates_plaintext_fallback() {
        let body = "x".repeat(600);
        let reason = rejected_reason_from_body(&body);
        assert_eq!(reason.len(), 500);
    }
}
