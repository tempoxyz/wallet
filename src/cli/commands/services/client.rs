//! HTTP client and fetch logic for the service directory.

use anyhow::{bail, Context as _, Result};

use super::model::{ServiceRegistry, SERVICES_API_URL};

/// Shared lightweight HTTP client for non-query commands (service directory, etc.).
pub(super) fn simple_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent(format!("tempo-wallet/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .context("failed to build HTTP client")
}

pub(super) async fn fetch_services(client: &reqwest::Client) -> Result<ServiceRegistry> {
    let url = std::env::var("TEMPO_SERVICES_URL").unwrap_or_else(|_| SERVICES_API_URL.to_string());
    let resp = client
        .get(&url)
        .send()
        .await
        .context("failed to fetch service directory")?;

    let status = resp.status();
    if !status.is_success() {
        bail!("service directory returned HTTP {status}");
    }

    resp.json::<ServiceRegistry>()
        .await
        .context("failed to parse service directory response")
}
