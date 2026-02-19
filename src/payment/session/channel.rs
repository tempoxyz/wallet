use alloy::primitives::{Address, B256};
use anyhow::{Context, Result};

use crate::http::HttpResponse;

use super::sse::stream_sse_response;
use super::types::{SessionContext, SessionResult, SessionState};
use super::voucher::build_voucher_credential;

/// Extract the origin (scheme://host\[:port\]) from a URL.
pub(super) fn extract_origin(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(parsed) => {
            let scheme = parsed.scheme();
            let host = parsed.host_str().unwrap_or("unknown");
            match parsed.port() {
                Some(port) => format!("{scheme}://{host}:{port}"),
                None => format!("{scheme}://{host}"),
            }
        }
        Err(_) => url.to_string(),
    }
}

/// Build the escrow open calls: approve + open.
///
/// Delegates to `mpp::client::tempo::build_open_calls`.
pub(super) fn build_open_calls(
    currency: Address,
    escrow_contract: Address,
    deposit: u128,
    payee: Address,
    salt: B256,
    authorized_signer: Address,
) -> Vec<tempo_primitives::transaction::Call> {
    mpp::client::tempo::build_open_calls(
        currency,
        escrow_contract,
        deposit,
        payee,
        salt,
        authorized_signer,
    )
}

/// Send the actual request with a voucher and handle the response.
pub(super) async fn send_session_request(
    ctx: &SessionContext<'_>,
    state: &mut SessionState,
) -> Result<SessionResult> {
    if ctx.request_ctx.cli.is_verbose() && ctx.request_ctx.cli.should_show_output() {
        eprintln!("Sending request with session voucher...");
    }

    let voucher_credential = build_voucher_credential(ctx.signer, ctx.echo, ctx.did, state).await?;

    let voucher_auth = mpp::format_authorization(&voucher_credential)
        .context("Failed to format voucher credential")?;

    let data_request = ctx
        .request_ctx
        .build_reqwest_request(ctx.url, None)?
        .header("Authorization", &voucher_auth);

    let response = data_request
        .send()
        .await
        .context("Failed to send session request")?;

    let status = response.status();
    if status.as_u16() == 402 || status.is_client_error() || status.is_server_error() {
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!(
            "Session request failed: HTTP {} — {}",
            status,
            body.chars().take(500).collect::<String>()
        );
    }

    let is_sse = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.contains("text/event-stream"));

    if is_sse {
        stream_sse_response(ctx, state, response).await?;
        Ok(SessionResult::Streamed)
    } else {
        let status_code = status.as_u16() as u32;
        let mut headers = std::collections::HashMap::new();
        for (key, value) in response.headers() {
            if let Ok(value_str) = value.to_str() {
                headers.insert(key.as_str().to_lowercase(), value_str.to_string());
            }
        }
        let body = response.bytes().await?.to_vec();

        Ok(SessionResult::Response(HttpResponse {
            status_code,
            headers,
            body,
        }))
    }
}
