use anyhow::{Context, Result};

use mpp::protocol::methods::tempo::session::SessionCredentialPayload;
use mpp::protocol::methods::tempo::sign_voucher;
use mpp::ChallengeEcho;

use super::types::SessionState;

/// Build a voucher credential for an existing session.
pub(super) async fn build_voucher_credential(
    signer: &alloy::signers::local::PrivateKeySigner,
    echo: &ChallengeEcho,
    did: &str,
    state: &SessionState,
) -> Result<mpp::PaymentCredential> {
    let sig = sign_voucher(
        signer,
        state.channel_id,
        state.cumulative_amount,
        state.escrow_contract,
        state.chain_id,
    )
    .await
    .context("Failed to sign voucher")?;

    let payload = SessionCredentialPayload::Voucher {
        channel_id: format!("{}", state.channel_id),
        cumulative_amount: state.cumulative_amount.to_string(),
        signature: format!("0x{}", hex::encode(&sig)),
    };

    Ok(mpp::PaymentCredential::with_source(
        echo.clone(),
        did.to_string(),
        payload,
    ))
}

/// Post a voucher to the server in a background task.
///
/// We MUST NOT await the response inline because the server may respond
/// with a streaming body (treating the POST as a new chat request).
/// Awaiting would deadlock: the server waits for us to read the SSE
/// stream, and we wait for the POST response.
pub(super) fn post_voucher(client: &reqwest::Client, url: &str, auth: &str, verbose: bool) {
    let vc = client.clone();
    let url = url.to_string();
    let auth = auth.to_string();
    tokio::spawn(async move {
        match vc.post(&url).header("Authorization", &auth).send().await {
            Ok(resp) => {
                if verbose {
                    let status = resp.status();
                    let ct = resp
                        .headers()
                        .get("content-type")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("none")
                        .to_string();
                    eprintln!("[voucher POST: {} content-type={}]", status, ct);
                }
            }
            Err(e) => {
                eprintln!("[voucher POST failed: {}]", e);
            }
        }
    });
}
