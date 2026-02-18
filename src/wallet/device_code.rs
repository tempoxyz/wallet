use anyhow::Result;
use serde::Deserialize;

use crate::error::PrestoError;

#[derive(Debug, Deserialize)]
pub struct DeviceCodeResponse {
    pub code: String,
    #[allow(dead_code)]
    pub expires_in: u64,
}

#[derive(Debug, Deserialize)]
pub struct PollResponse {
    pub status: String,
    pub account_address: Option<String>,
    pub key_authorization: Option<String>,
    pub error: Option<String>,
}

pub async fn create_device_code(
    client: &reqwest::Client,
    base_url: &str,
    pub_key: &str,
    key_type: &str,
    code_challenge: &str,
) -> Result<DeviceCodeResponse> {
    if std::env::var("PRESTO_MOCK_DEVICE_CODE").is_ok() {
        return Ok(DeviceCodeResponse {
            code: "TESTCODE".to_string(),
            expires_in: 600,
        });
    }

    let url = format!("{}/cli-auth/device-code", base_url);
    let resp = client
        .post(&url)
        .json(&serde_json::json!({
            "pub_key": pub_key,
            "key_type": key_type,
            "code_challenge": code_challenge,
        }))
        .send()
        .await
        .map_err(|e| PrestoError::Http(format!("Failed to create device code: {}", e)))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(PrestoError::Http(format!(
            "Device code request failed ({}): {}",
            status, body
        ))
        .into());
    }

    resp.json::<DeviceCodeResponse>().await.map_err(|e| {
        PrestoError::Http(format!("Failed to parse device code response: {}", e)).into()
    })
}

pub async fn poll_device_code(
    client: &reqwest::Client,
    base_url: &str,
    code: &str,
    code_verifier: &str,
) -> Result<PollResponse> {
    if std::env::var("PRESTO_MOCK_DEVICE_CODE").is_ok() {
        return Ok(PollResponse {
            status: "authorized".to_string(),
            account_address: Some("0x0000000000000000000000000000000000000001".to_string()),
            key_authorization: None,
            error: None,
        });
    }

    let url = format!("{}/cli-auth/poll/{}", base_url, code);
    let resp = client
        .post(&url)
        .json(&serde_json::json!({
            "code_verifier": code_verifier,
        }))
        .send()
        .await
        .map_err(|e| PrestoError::Http(format!("Failed to poll device code: {}", e)))?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(PrestoError::Http("Device code expired or not found".to_string()).into());
    }

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(
            PrestoError::Http(format!("Poll request failed ({}): {}", status, body)).into(),
        );
    }

    resp.json::<PollResponse>()
        .await
        .map_err(|e| PrestoError::Http(format!("Failed to parse poll response: {}", e)).into())
}
