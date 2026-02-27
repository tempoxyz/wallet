//! Service directory: fetch and deserialize the MPP service registry.

use std::collections::HashMap;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const SERVICES_API_URL: &str =
    "https://mpp.sh/api/services?x-vercel-protection-bypass=iGDnLnmF0nK6LWloAotUbTo3urEsaIkB";

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct ServiceRegistry {
    pub version: u32,
    pub services: Vec<Service>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct Service {
    pub id: String,
    pub name: String,
    pub url: String,
    #[serde(default, rename = "serviceUrl")]
    pub service_url: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub integration: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub docs: Option<ServiceDocs>,
    #[serde(default)]
    pub methods: HashMap<String, PaymentMethod>,
    #[serde(default)]
    pub realm: Option<String>,
    #[serde(default)]
    pub endpoints: Vec<Endpoint>,
    #[serde(default)]
    pub provider: Option<Provider>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct ServiceDocs {
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default, rename = "llmsTxt")]
    pub llms_txt: Option<String>,
    #[serde(default)]
    pub openapi: Option<String>,
    #[serde(default, rename = "apiReference")]
    pub api_reference: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct PaymentMethod {
    #[serde(default)]
    pub intents: Vec<String>,
    #[serde(default)]
    pub assets: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct Endpoint {
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub payment: Option<EndpointPayment>,
    #[serde(default)]
    pub docs: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct EndpointPayment {
    pub intent: String,
    pub method: String,
    #[serde(default)]
    pub amount: Option<String>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub decimals: Option<u32>,
    #[serde(default)]
    pub recipient: Option<String>,
    #[serde(default, rename = "unitType")]
    pub unit_type: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub dynamic: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct Provider {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
}

// ---------------------------------------------------------------------------
// Fetch
// ---------------------------------------------------------------------------

pub(crate) async fn fetch_services() -> Result<ServiceRegistry> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .context("failed to build HTTP client")?;
    let resp = client
        .get(SERVICES_API_URL)
        .send()
        .await
        .context("failed to fetch service directory")?;

    let status = resp.status();
    if !status.is_success() {
        anyhow::bail!("service directory returned HTTP {status}");
    }

    resp.json::<ServiceRegistry>()
        .await
        .context("failed to parse service directory response")
}
