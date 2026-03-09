//! Session-open transaction building and retry logic.
//!
//! Constructs the session-open transaction and retries submission
//! when the server hasn't indexed the channel yet. Low-level signing
//! and broadcast helpers remain in `tempo_common::payment::session::tx`.

use alloy::primitives::Address;
use anyhow::Result;

use crate::http::{HttpClient, HttpResponse};
use tempo_common::error::{ConfigError, PaymentError};
use tempo_common::keys::Signer;
use tempo_common::payment::session::tx as common_tx;

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
) -> Result<TempoPaymentResult> {
    let rpc_url: url::Url = rpc_url_str
        .parse()
        .map_err(|e| ConfigError::Invalid(format!("invalid RPC URL: {}", e)))?;
    let provider = alloy::providers::RootProvider::<mpp::client::TempoNetwork>::new_http(rpc_url);

    let from = signing.from;
    let tx_bytes =
        common_tx::resolve_and_sign_tx(&provider, signing, chain_id, fee_token, from, calls)
            .await?;

    Ok(TempoPaymentResult { tx_bytes })
}

/// Send the Open credential to the server and retry on HTTP 410 while the node indexes.
pub(super) async fn send_open_with_retry(
    http: &HttpClient,
    url: &str,
    auth_header: &str,
    delays_ms: &[u64],
) -> Result<HttpResponse> {
    let truncate = |s: String| -> String { s.chars().take(500).collect() };

    let headers = vec![("Authorization".to_string(), auth_header.to_string())];
    let resp = http.execute(url, &headers).await?;

    if resp.status_code < 400 {
        return Ok(resp);
    }

    if resp.status_code == 410 {
        let body = resp.body_string().unwrap_or_default();
        if body.contains("channel not funded") || body.contains("Channel Not Found") {
            if http.log_enabled() {
                eprintln!("Server hasn't indexed channel yet, retrying...");
            }
            for delay in delays_ms {
                tokio::time::sleep(std::time::Duration::from_millis(*delay)).await;
                let next = http.execute(url, &headers).await?;
                if next.status_code < 400 {
                    return Ok(next);
                }
                if next.status_code != 410 {
                    let nb = next.body_string().unwrap_or_default();
                    let reason = tempo_common::payment::error::extract_json_error(&nb)
                        .unwrap_or_else(|| truncate(nb));
                    return Err(PaymentError::PaymentRejected {
                        reason,
                        status_code: next.status_code,
                    }
                    .into());
                }
            }
            return Err(PaymentError::PaymentRejected {
                reason: "Server could not find channel after retries".to_string(),
                status_code: 410,
            }
            .into());
        } else {
            return Err(PaymentError::PaymentRejected {
                reason: truncate(body),
                status_code: 410,
            }
            .into());
        }
    }

    let body = resp.body_string().unwrap_or_default();
    let reason =
        tempo_common::payment::error::extract_json_error(&body).unwrap_or_else(|| truncate(body));
    Err(PaymentError::PaymentRejected {
        reason,
        status_code: resp.status_code,
    }
    .into())
}
