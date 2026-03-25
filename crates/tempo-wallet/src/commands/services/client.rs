//! HTTP client and fetch logic for the service directory.

use tempo_common::error::{ConfigError, NetworkError, TempoError};

use super::model::{ServiceRegistry, SERVICES_API_URL};

/// Shared lightweight HTTP client for non-query commands (service directory, etc.).
pub(super) fn simple_client() -> Result<reqwest::Client, TempoError> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent(format!("tempo-wallet/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(NetworkError::Reqwest)
        .map_err(TempoError::from)
}

pub(super) async fn fetch_services(
    client: &reqwest::Client,
) -> Result<ServiceRegistry, TempoError> {
    let url = std::env::var("TEMPO_SERVICES_URL").unwrap_or_else(|_| SERVICES_API_URL.to_string());
    let parsed_url = reqwest::Url::parse(&url).map_err(|source| ConfigError::InvalidUrl {
        context: "service directory",
        source,
    })?;
    let resp = client
        .get(parsed_url)
        .send()
        .await
        .map_err(NetworkError::Reqwest)?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.map_err(NetworkError::Reqwest)?;
        return Err(NetworkError::HttpStatus {
            operation: "fetch service directory",
            status: status.as_u16(),
            body: Some(body),
        }
        .into());
    }

    let body = resp.text().await.map_err(NetworkError::Reqwest)?;
    serde_json::from_str::<ServiceRegistry>(&body)
        .map_err(|source| NetworkError::ResponseParse {
            context: "service directory response",
            source,
        })
        .map_err(TempoError::from)
}
