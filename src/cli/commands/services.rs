//! Service directory commands: list and inspect MPP services.

use std::collections::HashMap;

use anyhow::{bail, Context as _, Result};
use serde::{Deserialize, Serialize};

use crate::cli::args::ServicesCommands;
use crate::cli::{Context, OutputFormat};

// ---------------------------------------------------------------------------
// Data model (service registry)
// ---------------------------------------------------------------------------

const SERVICES_API_URL: &str =
    "https://mpp.sh/api/services?x-vercel-protection-bypass=iGDnLnmF0nK6LWloAotUbTo3urEsaIkB";

#[derive(Debug, Deserialize, Serialize)]
struct ServiceRegistry {
    version: u32,
    services: Vec<Service>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Service {
    id: String,
    name: String,
    url: String,
    #[serde(default, rename = "serviceUrl")]
    service_url: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    categories: Vec<String>,
    #[serde(default)]
    integration: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    docs: Option<ServiceDocs>,
    #[serde(default)]
    methods: HashMap<String, PaymentMethod>,
    #[serde(default)]
    realm: Option<String>,
    #[serde(default)]
    endpoints: Vec<Endpoint>,
    #[serde(default)]
    provider: Option<Provider>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ServiceDocs {
    #[serde(default)]
    homepage: Option<String>,
    #[serde(default, rename = "llmsTxt")]
    llms_txt: Option<String>,
    #[serde(default)]
    openapi: Option<String>,
    #[serde(default, rename = "apiReference")]
    api_reference: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct PaymentMethod {
    #[serde(default)]
    intents: Vec<String>,
    #[serde(default)]
    assets: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Endpoint {
    method: String,
    path: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    payment: Option<EndpointPayment>,
    #[serde(default)]
    docs: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct EndpointPayment {
    intent: String,
    method: String,
    #[serde(default)]
    amount: Option<String>,
    #[serde(default)]
    currency: Option<String>,
    #[serde(default)]
    decimals: Option<u32>,
    #[serde(default)]
    recipient: Option<String>,
    #[serde(default, rename = "unitType")]
    unit_type: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    dynamic: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Provider {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    icon: Option<String>,
}

async fn fetch_services() -> Result<ServiceRegistry> {
    let url = std::env::var("PRESTO_SERVICES_URL").unwrap_or_else(|_| SERVICES_API_URL.to_string());
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
        anyhow::bail!("service directory returned HTTP {status}");
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
    let output_format = ctx.output_format;
    match command {
        Some(ServicesCommands::Info { service_id }) => {
            show_service_info(output_format, &service_id).await
        }
        Some(ServicesCommands::List) => {
            list_services(output_format, category.as_deref(), search.as_deref()).await
        }
        None if service_id.is_some() => {
            show_service_info(output_format, service_id.as_deref().unwrap()).await
        }
        None => list_services(output_format, category.as_deref(), search.as_deref()).await,
    }
}

// ---------------------------------------------------------------------------
// JSON output shapes (curated, not raw API dumps)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ServiceListEntry<'a> {
    id: &'a str,
    name: &'a str,
    categories: &'a [String],
    status: Option<&'a str>,
    integration: Option<&'a str>,
    payment: Vec<&'a str>,
    service_url: Option<&'a str>,
}

#[derive(Serialize)]
struct ServiceDetail<'a> {
    id: &'a str,
    name: &'a str,
    description: Option<&'a str>,
    categories: &'a [String],
    status: Option<&'a str>,
    integration: Option<&'a str>,
    service_url: Option<&'a str>,
    url: &'a str,
    tags: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<ProviderJson<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    docs: Option<DocsJson<'a>>,
    endpoints: Vec<EndpointJson<'a>>,
}

#[derive(Serialize)]
struct ProviderJson<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<&'a str>,
}

#[derive(Serialize)]
struct DocsJson<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    homepage: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    llms_txt: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    openapi: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    api_reference: Option<&'a str>,
}

#[derive(Serialize)]
struct EndpointJson<'a> {
    method: &'a str,
    path: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
    pricing: String,
    example: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    docs: Option<&'a str>,
}

fn collect_payment_intents(s: &Service) -> Vec<&str> {
    let mut intents: Vec<&str> = s
        .methods
        .values()
        .flat_map(|m| m.intents.iter().map(|i| i.as_str()))
        .collect();
    intents.sort();
    intents.dedup();
    intents
}

fn format_endpoint_pricing(ep: &Endpoint) -> String {
    match &ep.payment {
        None => "free".to_string(),
        Some(p) => {
            let mut parts = Vec::new();
            if p.dynamic == Some(true) {
                parts.push("dynamic".to_string());
            } else if let Some(ref amount) = p.amount {
                let formatted = match p.decimals {
                    Some(d) if d > 0 => match amount.parse::<u128>() {
                        Ok(v) => {
                            let s = v.to_string();
                            let d = d as usize;
                            let padded = format!("{:0>width$}", s, width = d + 1);
                            let (int, frac) = padded.split_at(padded.len() - d);
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

fn format_example_command(method: &str, base_url: &str, path: &str) -> String {
    let url = format!("{}{}", base_url.trim_end_matches('/'), path);
    match method.to_uppercase().as_str() {
        "GET" => format!("presto {url}"),
        m => format!("presto -X {m} --json '{{}}' {url}"),
    }
}

fn to_list_entry(s: &Service) -> ServiceListEntry<'_> {
    ServiceListEntry {
        id: &s.id,
        name: &s.name,
        categories: &s.categories,
        status: s.status.as_deref(),
        integration: s.integration.as_deref(),
        payment: collect_payment_intents(s),
        service_url: s.service_url.as_deref(),
    }
}

fn to_detail(s: &Service) -> ServiceDetail<'_> {
    let service_desc = s.description.as_deref();

    ServiceDetail {
        id: &s.id,
        name: &s.name,
        description: service_desc,
        categories: &s.categories,
        status: s.status.as_deref(),
        integration: s.integration.as_deref(),
        service_url: s.service_url.as_deref(),
        url: &s.url,
        tags: &s.tags,
        provider: s.provider.as_ref().map(|p| ProviderJson {
            name: p.name.as_deref(),
            url: p.url.as_deref(),
        }),
        docs: s.docs.as_ref().map(|d| DocsJson {
            homepage: d.homepage.as_deref(),
            llms_txt: d.llms_txt.as_deref(),
            openapi: d.openapi.as_deref(),
            api_reference: d.api_reference.as_deref(),
        }),
        endpoints: s
            .endpoints
            .iter()
            .map(|ep| {
                let ep_desc = ep
                    .description
                    .as_deref()
                    .or_else(|| ep.payment.as_ref().and_then(|p| p.description.as_deref()))
                    .or(service_desc);
                let base_url = s.service_url.as_deref().unwrap_or(&s.url);
                let example = format_example_command(&ep.method, base_url, &ep.path);
                EndpointJson {
                    method: &ep.method,
                    path: &ep.path,
                    description: ep_desc,
                    pricing: format_endpoint_pricing(ep),
                    example,
                    docs: ep.docs.as_deref(),
                }
            })
            .collect(),
    }
}

// ---------------------------------------------------------------------------
// List
// ---------------------------------------------------------------------------

async fn list_services(
    output_format: OutputFormat,
    category: Option<&str>,
    search: Option<&str>,
) -> Result<()> {
    let registry = fetch_services().await?;

    let filtered: Vec<&Service> = registry
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
                let matches_name = s.name.to_lowercase().contains(&q_lower);
                let matches_id = s.id.to_lowercase().contains(&q_lower);
                let matches_desc = s
                    .description
                    .as_ref()
                    .is_some_and(|d| d.to_lowercase().contains(&q_lower));
                let matches_tags = s.tags.iter().any(|t| t.to_lowercase().contains(&q_lower));
                if !(matches_name || matches_id || matches_desc || matches_tags) {
                    return false;
                }
            }
            true
        })
        .collect();

    match output_format {
        OutputFormat::Json | OutputFormat::Toon => {
            let entries: Vec<_> = filtered.iter().map(|s| to_list_entry(s)).collect();
            println!("{}", output_format.serialize(&entries)?);
        }
        OutputFormat::Text => {
            if filtered.is_empty() {
                println!("No services found.");
                return Ok(());
            }
            render_service_table(&filtered);
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Info
// ---------------------------------------------------------------------------

async fn show_service_info(output_format: OutputFormat, service_id: &str) -> Result<()> {
    let registry = fetch_services().await?;

    let service = registry
        .services
        .iter()
        .find(|s| s.id.eq_ignore_ascii_case(service_id));

    let Some(service) = service else {
        bail!("service '{service_id}' not found");
    };

    match output_format {
        OutputFormat::Json | OutputFormat::Toon => {
            println!("{}", output_format.serialize(&to_detail(service))?);
        }
        OutputFormat::Text => {
            render_service_detail(service);
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Text rendering — list table
// ---------------------------------------------------------------------------

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max - 1).collect();
        format!("{truncated}…")
    }
}

fn render_service_table(services: &[&Service]) {
    // Column width caps
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
        .map(|s| format_categories(s).len())
        .max()
        .unwrap_or(8)
        .clamp(8, MAX_CAT);
    let w_status = services
        .iter()
        .map(|s| s.status.as_deref().unwrap_or("—").len())
        .max()
        .unwrap_or(6)
        .clamp(6, MAX_STATUS);
    let w_integ = 3; // "1p" / "3p"
    let w_payment = services
        .iter()
        .map(|s| format_payment_intents(s).len())
        .max()
        .unwrap_or(7)
        .max(7);

    // Header
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
        let categories = truncate(&format_categories(s), MAX_CAT);
        let status = truncate(s.status.as_deref().unwrap_or("—"), MAX_STATUS);
        let integration = match s.integration.as_deref() {
            Some("first-party") => "1p",
            Some("third-party") => "3p",
            _ => "—",
        };
        let payment = format_payment_intents(s);
        let service_url = s.service_url.as_deref().unwrap_or("—");

        println!(
            "  {:<w_id$}  {:<w_name$}  {:<w_cat$}  {:<w_status$}  {:<w_integ$}  {:<w_payment$}  {}",
            id, name, categories, status, integration, payment, service_url
        );
    }

    println!("\n{} service(s).", services.len());
}

fn format_categories(s: &Service) -> String {
    if s.categories.is_empty() {
        "—".to_string()
    } else {
        s.categories.join(", ")
    }
}

fn format_payment_intents(s: &Service) -> String {
    let intents = collect_payment_intents(s);
    if intents.is_empty() {
        "—".to_string()
    } else {
        intents.join(", ")
    }
}

// ---------------------------------------------------------------------------
// Text rendering — detail view
// ---------------------------------------------------------------------------

fn render_service_detail(s: &Service) {
    println!("{}", s.name);
    println!("{}", "─".repeat(s.name.chars().count()));

    if let Some(ref desc) = s.description {
        println!("{desc}");
    }
    println!();

    print_field("ID", &s.id);
    print_field("Categories", &format_categories(s));
    print_field("Status", s.status.as_deref().unwrap_or("—"));
    print_field("Integration", s.integration.as_deref().unwrap_or("—"));
    print_field("Service URL", s.service_url.as_deref().unwrap_or("—"));
    print_field("Upstream URL", &s.url);

    if !s.tags.is_empty() {
        print_field("Tags", &s.tags.join(", "));
    }
    if let Some(ref icon) = s.icon {
        print_field("Icon", icon);
    }
    if let Some(ref realm) = s.realm {
        print_field("Realm", realm);
    }

    // Provider
    if let Some(ref p) = s.provider {
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

    // Docs
    if let Some(ref docs) = s.docs {
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

    // Endpoints
    if !s.endpoints.is_empty() {
        println!();
        println!("Endpoints:");
        let base_url = s.service_url.as_deref().unwrap_or(&s.url);
        for ep in &s.endpoints {
            render_endpoint(ep, base_url);
        }
    }
}

fn render_endpoint(ep: &Endpoint, base_url: &str) {
    let pricing = format_endpoint_pricing(ep);

    println!("  {:>6} {:<40} {}", ep.method, ep.path, pricing);

    // Show endpoint description, but skip if it duplicates the payment description
    let payment_desc = ep.payment.as_ref().and_then(|p| p.description.as_deref());
    if let Some(ref desc) = ep.description {
        println!("         {desc}");
    } else if let Some(desc) = payment_desc {
        println!("         {desc}");
    }

    if let Some(ref p) = ep.payment {
        if let Some(ref unit_type) = p.unit_type {
            println!("         per {unit_type}");
        }
    }

    println!(
        "         example: {}",
        format_example_command(&ep.method, base_url, &ep.path)
    );

    if let Some(ref docs_url) = ep.docs {
        println!("         docs: {docs_url}");
    }
}

fn print_field(label: &str, value: &str) {
    println!("{:>14}: {value}", label);
}
