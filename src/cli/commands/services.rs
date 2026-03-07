//! Service directory commands: list and inspect MPP services.

use std::collections::{BTreeSet, HashMap};

use anyhow::{bail, Context as _, Result};
use serde::{Deserialize, Serialize};

use crate::cli::args::ServicesCommands;
use crate::cli::output;
use crate::cli::{Context, OutputFormat};
use crate::util::{print_field, truncate};

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

#[derive(Debug, Deserialize, Serialize)]
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

#[derive(Debug, Deserialize, Serialize)]
struct PaymentMethod {
    #[serde(default)]
    intents: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
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

#[derive(Debug, Deserialize, Serialize)]
struct EndpointPayment {
    intent: String,
    #[serde(default)]
    amount: Option<String>,
    #[serde(default)]
    decimals: Option<u32>,
    #[serde(default, rename = "unitType")]
    unit_type: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    dynamic: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Provider {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    icon: Option<String>,
}

impl ServiceRegistry {
    fn find(&self, id: &str) -> Option<&Service> {
        self.services.iter().find(|s| s.id.eq_ignore_ascii_case(id))
    }
}

impl Service {
    fn payment_intents(&self) -> BTreeSet<&str> {
        self.methods
            .values()
            .flat_map(|m| m.intents.iter().map(|i| i.as_str()))
            .collect()
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
            intents.into_iter().collect::<Vec<_>>().join(", ")
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
                } else if let Some(amount) = &p.amount {
                    parts.push(format_amount(amount, p.decimals));
                }
                parts.push(p.intent.clone());
                parts.join(" ")
            }
        }
    }
}

/// Format a token amount string with the given decimal places as a dollar value.
fn format_amount(amount: &str, decimals: Option<u32>) -> String {
    match decimals {
        Some(dec) if dec > 0 => match amount.parse::<u128>() {
            Ok(v) => {
                let dec = dec as usize;
                let padded = format!("{:0>width$}", v, width = dec + 1);
                let (int, frac) = padded.split_at(padded.len() - dec);
                format!("${int}.{frac}")
            }
            Err(_) => amount.to_string(),
        },
        _ => amount.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Rendering (free functions, separated from data model)
// ---------------------------------------------------------------------------

fn render_service_list(
    services: &[Service],
    output_format: OutputFormat,
    category: Option<&str>,
    search: Option<&str>,
) -> Result<()> {
    let filtered: Vec<&Service> = services
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

    output::emit_by_format(output_format, &filtered, || {
        if filtered.is_empty() {
            println!("No services found.");
            return Ok(());
        }
        render_table(&filtered);
        Ok(())
    })?;

    Ok(())
}

fn render_table(services: &[&Service]) {
    // Max column widths to keep the table readable on standard terminals.
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
    // Integration column: always "1p", "3p", or "—" (max 2 chars + padding).
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

fn render_service_detail(service: &Service, output_format: OutputFormat) -> Result<()> {
    output::emit_by_format(output_format, service, || {
        render_detail(service);
        Ok(())
    })?;
    Ok(())
}

fn render_detail(s: &Service) {
    println!("{}", s.name);
    println!("{}", "─".repeat(s.name.chars().count()));

    if let Some(desc) = &s.description {
        println!("{desc}");
    }
    println!();

    print_field("ID", &s.id);
    print_field("Categories", &s.format_categories());
    print_field("Status", s.status.as_deref().unwrap_or("—"));
    print_field("Integration", s.integration.as_deref().unwrap_or("—"));
    print_field("Service URL", s.service_url.as_deref().unwrap_or("—"));
    print_field("Upstream URL", &s.url);

    if !s.tags.is_empty() {
        print_field("Tags", &s.tags.join(", "));
    }
    if let Some(icon) = &s.icon {
        print_field("Icon", icon);
    }
    if let Some(realm) = &s.realm {
        print_field("Realm", realm);
    }

    if let Some(p) = &s.provider {
        println!();
        println!("Provider:");
        if let Some(name) = &p.name {
            print_field("  Name", name);
        }
        if let Some(url) = &p.url {
            print_field("  URL", url);
        }
        if let Some(icon) = &p.icon {
            print_field("  Icon", icon);
        }
    }

    if let Some(docs) = &s.docs {
        let has_any = docs.homepage.is_some()
            || docs.llms_txt.is_some()
            || docs.openapi.is_some()
            || docs.api_reference.is_some();
        if has_any {
            println!();
            println!("Docs:");
            if let Some(v) = &docs.homepage {
                print_field("  Homepage", v);
            }
            if let Some(v) = &docs.llms_txt {
                print_field("  LLMs.txt", v);
            }
            if let Some(v) = &docs.openapi {
                print_field("  OpenAPI", v);
            }
            if let Some(v) = &docs.api_reference {
                print_field("  API Reference", v);
            }
        }
    }

    if !s.endpoints.is_empty() {
        println!();
        println!("Endpoints:");
        let base_url = s.service_url.as_deref().unwrap_or(&s.url);
        for ep in &s.endpoints {
            let pricing = ep.format_pricing();
            println!("  {:>6} {:<40} {}", ep.method, ep.path, pricing);

            let desc = ep
                .description
                .as_deref()
                .or_else(|| ep.payment.as_ref().and_then(|p| p.description.as_deref()));
            if let Some(desc) = desc {
                println!("         {desc}");
            }

            if let Some(unit_type) = ep.payment.as_ref().and_then(|p| p.unit_type.as_deref()) {
                println!("         per {unit_type}");
            }

            let full_url = format!("{}{}", base_url.trim_end_matches('/'), ep.path);
            let example = match ep.method.to_uppercase().as_str() {
                "GET" => format!("tempo-wallet {full_url}"),
                m => format!("tempo-wallet -X {m} --json '{{}}' {full_url}"),
            };
            println!("         example: {example}");

            if let Some(docs_url) = &ep.docs {
                println!("         docs: {docs_url}");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Fetch + entry point
// ---------------------------------------------------------------------------

async fn fetch_services(client: &reqwest::Client) -> Result<ServiceRegistry> {
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

/// Shared lightweight HTTP client for non-query commands (service directory, etc.).
fn simple_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent(format!("tempo-wallet/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .context("failed to build HTTP client")
}

/// CLI arguments for the `services` command, unpacked from the enum variant.
pub(crate) struct ServicesArgs {
    pub(crate) command: Option<ServicesCommands>,
    pub(crate) service_id: Option<String>,
    pub(crate) category: Option<String>,
    pub(crate) search: Option<String>,
}

pub(crate) async fn run(ctx: &Context, args: ServicesArgs) -> Result<()> {
    let client = simple_client()?;
    let registry = fetch_services(&client).await?;
    let output_format = ctx.output_format;

    // `services info <ID>` or `services <ID>` → detail view
    let info_id = match &args.command {
        Some(ServicesCommands::Info { service_id }) => Some(service_id.as_str()),
        None if args.service_id.is_some() => args.service_id.as_deref(),
        _ => None,
    };

    if let Some(id) = info_id {
        let Some(service) = registry.find(id) else {
            bail!("service '{id}' not found");
        };
        return render_service_detail(service, output_format);
    }

    // `services list` or `services` (with optional --category / --search)
    render_service_list(
        &registry.services,
        output_format,
        args.category.as_deref(),
        args.search.as_deref(),
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_amount_with_decimals() {
        assert_eq!(format_amount("1000000", Some(6)), "$1.000000");
        assert_eq!(format_amount("500", Some(2)), "$5.00");
    }

    #[test]
    fn format_amount_zero_decimals() {
        assert_eq!(format_amount("42", Some(0)), "42");
        assert_eq!(format_amount("42", None), "42");
    }

    #[test]
    fn format_amount_unparseable() {
        assert_eq!(format_amount("not-a-number", Some(6)), "not-a-number");
    }

    #[test]
    fn format_amount_small_value_pads() {
        // 123 with 6 decimals → $0.000123
        assert_eq!(format_amount("123", Some(6)), "$0.000123");
    }

    #[test]
    fn format_pricing_free() {
        let ep = Endpoint {
            method: "GET".into(),
            path: "/v1/test".into(),
            description: None,
            payment: None,
            docs: None,
        };
        assert_eq!(ep.format_pricing(), "free");
    }

    #[test]
    fn format_pricing_dynamic() {
        let ep = Endpoint {
            method: "POST".into(),
            path: "/v1/test".into(),
            description: None,
            payment: Some(EndpointPayment {
                intent: "charge".into(),
                amount: None,
                decimals: None,
                unit_type: None,
                description: None,
                dynamic: Some(true),
            }),
            docs: None,
        };
        assert_eq!(ep.format_pricing(), "dynamic charge");
    }

    #[test]
    fn format_pricing_fixed_amount() {
        let ep = Endpoint {
            method: "POST".into(),
            path: "/v1/test".into(),
            description: None,
            payment: Some(EndpointPayment {
                intent: "session".into(),
                amount: Some("1000000".into()),
                decimals: Some(6),
                unit_type: None,
                description: None,
                dynamic: None,
            }),
            docs: None,
        };
        assert_eq!(ep.format_pricing(), "$1.000000 session");
    }

    #[test]
    fn payment_intents_deduplicates() {
        let mut methods = HashMap::new();
        methods.insert(
            "a".into(),
            PaymentMethod {
                intents: vec!["charge".into(), "session".into()],
            },
        );
        methods.insert(
            "b".into(),
            PaymentMethod {
                intents: vec!["session".into()],
            },
        );
        let s = Service {
            id: "test".into(),
            name: "Test".into(),
            url: "https://example.com".into(),
            service_url: None,
            description: None,
            icon: None,
            categories: vec![],
            integration: None,
            tags: vec![],
            status: None,
            docs: None,
            methods,
            realm: None,
            endpoints: vec![],
            provider: None,
        };
        let intents: Vec<&str> = s.payment_intents().into_iter().collect();
        assert_eq!(intents, vec!["charge", "session"]);
    }
}
