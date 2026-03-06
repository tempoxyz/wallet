//! Service directory commands: list and inspect MPP services.

use std::collections::HashMap;

use anyhow::{bail, Context as _, Result};
use serde::{Deserialize, Serialize};

use crate::cli::args::ServicesCommands;
use crate::cli::{Context, OutputFormat};

// ---------------------------------------------------------------------------
// Data model (service registry)
// ---------------------------------------------------------------------------

// Protection-bypass token is a public API key for unauthenticated access.
const SERVICES_API_URL: &str =
    "https://mpp.sh/api/services?x-vercel-protection-bypass=iGDnLnmF0nK6LWloAotUbTo3urEsaIkB";

#[derive(Deserialize)]
struct ServiceRegistry {
    services: Vec<Service>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Service {
    id: String,
    name: String,
    url: String,
    #[serde(
        default,
        rename = "serviceUrl",
        skip_serializing_if = "Option::is_none"
    )]
    service_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    icon: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    categories: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    integration: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    docs: Option<ServiceDocs>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    methods: HashMap<String, PaymentMethod>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    realm: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    endpoints: Vec<Endpoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    provider: Option<Provider>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ServiceDocs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    homepage: Option<String>,
    #[serde(default, rename = "llmsTxt", skip_serializing_if = "Option::is_none")]
    llms_txt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    openapi: Option<String>,
    #[serde(
        default,
        rename = "apiReference",
        skip_serializing_if = "Option::is_none"
    )]
    api_reference: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct PaymentMethod {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    intents: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Endpoint {
    method: String,
    path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    payment: Option<EndpointPayment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    docs: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct EndpointPayment {
    intent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    amount: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    decimals: Option<u32>,
    #[serde(default, rename = "unitType", skip_serializing_if = "Option::is_none")]
    unit_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dynamic: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Provider {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    icon: Option<String>,
}

impl ServiceRegistry {
    fn find(&self, id: &str) -> Option<&Service> {
        self.services.iter().find(|s| s.id.eq_ignore_ascii_case(id))
    }

    fn print_list(
        &self,
        output_format: OutputFormat,
        category: Option<&str>,
        search: Option<&str>,
    ) -> Result<()> {
        let filtered: Vec<&Service> = self
            .services
            .iter()
            .filter(|s| {
                if let Some(cat) = category {
                    if !s.categories.iter().any(|c| c.eq_ignore_ascii_case(cat)) {
                        return false;
                    }
                }
                if let Some(q) = search {
                    let q_lower = q.to_lowercase();
                    let matches = s.name.to_lowercase().contains(&q_lower)
                        || s.id.to_lowercase().contains(&q_lower)
                        || s.description
                            .as_ref()
                            .is_some_and(|d| d.to_lowercase().contains(&q_lower))
                        || s.tags.iter().any(|t| t.to_lowercase().contains(&q_lower));
                    if !matches {
                        return false;
                    }
                }
                true
            })
            .collect();

        match output_format {
            OutputFormat::Json | OutputFormat::Toon => {
                println!("{}", output_format.serialize(&filtered)?);
            }
            OutputFormat::Text => {
                if filtered.is_empty() {
                    eprintln!("No services found.");
                    return Ok(());
                }
                self.render_table(&filtered);
            }
        }

        Ok(())
    }

    fn render_table(&self, services: &[&Service]) {
        const MAX_ID: usize = 20;
        const MAX_NAME: usize = 24;
        const MAX_CAT: usize = 16;
        const MAX_STATUS: usize = 10;

        let w_id = services
            .iter()
            .map(|s| s.id.len())
            .max()
            .unwrap_or(2)
            .clamp(2, MAX_ID);
        let w_name = services
            .iter()
            .map(|s| s.name.len())
            .max()
            .unwrap_or(4)
            .clamp(4, MAX_NAME);
        let w_cat = services
            .iter()
            .map(|s| s.format_categories().len())
            .max()
            .unwrap_or(8)
            .clamp(8, MAX_CAT);
        let w_status = services
            .iter()
            .map(|s| s.status.as_deref().unwrap_or("—").len())
            .max()
            .unwrap_or(6)
            .clamp(6, MAX_STATUS);
        let w_integ = 3;
        let w_payment = services
            .iter()
            .map(|s| s.format_payment_intents().len())
            .max()
            .unwrap_or(7)
            .max(7);

        println!(
            "  {:<w_id$}  {:<w_name$}  {:<w_cat$}  {:<w_status$}  {:<w_integ$}  {:<w_payment$}  Service URL",
            "ID", "Name", "Category", "Status", "Int", "Payment"
        );
        let total_w =
            2 + w_id + 2 + w_name + 2 + w_cat + 2 + w_status + 2 + w_integ + 2 + w_payment + 2 + 30;
        println!("  {}", "─".repeat(total_w));

        for s in services {
            let id = truncate(&s.id, MAX_ID);
            let name = truncate(&s.name, MAX_NAME);
            let categories = truncate(&s.format_categories(), MAX_CAT);
            let status = truncate(s.status.as_deref().unwrap_or("—"), MAX_STATUS);
            let integration = match s.integration.as_deref() {
                Some("first-party") => "1p",
                Some("third-party") => "3p",
                _ => "—",
            };
            let payment = s.format_payment_intents();
            let service_url = s.service_url.as_deref().unwrap_or("—");

            println!(
                "  {:<w_id$}  {:<w_name$}  {:<w_cat$}  {:<w_status$}  {:<w_integ$}  {:<w_payment$}  {}",
                id, name, categories, status, integration, payment, service_url
            );
        }

        println!("\n{} service(s).", services.len());
    }
}

impl Service {
    fn print(&self, output_format: OutputFormat) -> Result<()> {
        match output_format {
            OutputFormat::Json | OutputFormat::Toon => {
                println!("{}", output_format.serialize(self)?);
            }
            OutputFormat::Text => self.render_detail(),
        }
        Ok(())
    }

    fn payment_intents(&self) -> Vec<&str> {
        let mut intents: Vec<&str> = self
            .methods
            .values()
            .flat_map(|m| m.intents.iter().map(|i| i.as_str()))
            .collect();
        intents.sort();
        intents.dedup();
        intents
    }

    fn format_categories(&self) -> String {
        if self.categories.is_empty() {
            "—".to_string()
        } else {
            self.categories.join(", ")
        }
    }

    fn format_payment_intents(&self) -> String {
        let intents = self.payment_intents();
        if intents.is_empty() {
            "—".to_string()
        } else {
            intents.join(", ")
        }
    }

    fn render_detail(&self) {
        println!("{}", self.name);
        println!("{}", "─".repeat(self.name.chars().count()));

        if let Some(ref desc) = self.description {
            println!("{desc}");
        }
        println!();

        print_field("ID", &self.id);
        print_field("Categories", &self.format_categories());
        print_field("Status", self.status.as_deref().unwrap_or("—"));
        print_field("Integration", self.integration.as_deref().unwrap_or("—"));
        print_field("Service URL", self.service_url.as_deref().unwrap_or("—"));
        print_field("Upstream URL", &self.url);

        if !self.tags.is_empty() {
            print_field("Tags", &self.tags.join(", "));
        }
        if let Some(ref icon) = self.icon {
            print_field("Icon", icon);
        }
        if let Some(ref realm) = self.realm {
            print_field("Realm", realm);
        }

        if let Some(ref p) = self.provider {
            println!();
            println!("Provider:");
            if let Some(ref name) = p.name {
                print_field("  Name", name);
            }
            if let Some(ref url) = p.url {
                print_field("  URL", url);
            }
            if let Some(ref icon) = p.icon {
                print_field("  Icon", icon);
            }
        }

        if let Some(ref docs) = self.docs {
            let has_any = docs.homepage.is_some()
                || docs.llms_txt.is_some()
                || docs.openapi.is_some()
                || docs.api_reference.is_some();
            if has_any {
                println!();
                println!("Docs:");
                if let Some(ref v) = docs.homepage {
                    print_field("  Homepage", v);
                }
                if let Some(ref v) = docs.llms_txt {
                    print_field("  LLMs.txt", v);
                }
                if let Some(ref v) = docs.openapi {
                    print_field("  OpenAPI", v);
                }
                if let Some(ref v) = docs.api_reference {
                    print_field("  API Reference", v);
                }
            }
        }

        if !self.endpoints.is_empty() {
            println!();
            println!("Endpoints:");
            let base_url = self.service_url.as_deref().unwrap_or(&self.url);
            for ep in &self.endpoints {
                let pricing = ep.format_pricing();
                println!("  {:>6} {:<40} {}", ep.method, ep.path, pricing);

                let desc = ep
                    .description
                    .as_deref()
                    .or_else(|| ep.payment.as_ref().and_then(|p| p.description.as_deref()));
                if let Some(desc) = desc {
                    println!("         {desc}");
                }

                if let Some(ref unit_type) =
                    ep.payment.as_ref().and_then(|p| p.unit_type.as_deref())
                {
                    println!("         per {unit_type}");
                }

                let full_url = format!("{}{}", base_url.trim_end_matches('/'), ep.path);
                let example = match ep.method.to_uppercase().as_str() {
                    "GET" => format!("tempo-wallet {full_url}"),
                    m => format!("tempo-wallet -X {m} --json '{{}}' {full_url}"),
                };
                println!("         example: {example}");

                if let Some(ref docs_url) = ep.docs {
                    println!("         docs: {docs_url}");
                }
            }
        }
    }
}

impl Endpoint {
    fn format_pricing(&self) -> String {
        match &self.payment {
            None => "free".to_string(),
            Some(p) => {
                let mut parts = Vec::new();
                if p.dynamic == Some(true) {
                    parts.push("dynamic".to_string());
                } else if let Some(ref amount) = p.amount {
                    let formatted = match p.decimals {
                        Some(dec) if dec > 0 => match amount.parse::<u128>() {
                            Ok(v) => {
                                let s = v.to_string();
                                let dec = dec as usize;
                                let padded = format!("{:0>width$}", s, width = dec + 1);
                                let (int, frac) = padded.split_at(padded.len() - dec);
                                format!("${int}.{frac}")
                            }
                            Err(_) => amount.clone(),
                        },
                        _ => amount.clone(),
                    };
                    parts.push(formatted);
                }
                parts.push(p.intent.clone());
                parts.join(" ")
            }
        }
    }
}

async fn fetch_services() -> Result<ServiceRegistry> {
    let url = std::env::var("TEMPO_SERVICES_URL").unwrap_or_else(|_| SERVICES_API_URL.to_string());
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .context("failed to build HTTP client")?;
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

pub(crate) async fn run(
    ctx: &Context,
    command: Option<ServicesCommands>,
    service_id: Option<String>,
    category: Option<String>,
    search: Option<String>,
) -> Result<()> {
    let registry = fetch_services().await?;
    let output_format = ctx.output_format;

    // `services info <ID>` or `services <ID>` → detail view
    let info_id = match &command {
        Some(ServicesCommands::Info { service_id }) => Some(service_id.as_str()),
        None if service_id.is_some() => service_id.as_deref(),
        _ => None,
    };

    if let Some(id) = info_id {
        let Some(service) = registry.find(id) else {
            bail!("service '{id}' not found");
        };
        return service.print(output_format);
    }

    // `services list` or `services` (with optional --category / --search)
    registry.print_list(output_format, category.as_deref(), search.as_deref())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max - 1).collect();
        format!("{truncated}…")
    }
}

fn print_field(label: &str, value: &str) {
    println!("{:>14}: {value}", label);
}
