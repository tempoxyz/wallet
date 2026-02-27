//! Service directory commands: list and inspect MPP services.

use anyhow::{bail, Result};

use super::OutputFormat;
use crate::services::{self, Endpoint, Service};

// ---------------------------------------------------------------------------
// List
// ---------------------------------------------------------------------------

pub(crate) async fn list_services(
    output_format: OutputFormat,
    category: Option<&str>,
    search: Option<&str>,
) -> Result<()> {
    let registry = services::fetch_services().await?;

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
        OutputFormat::Json => {
            let json: Vec<&Service> = filtered;
            println!("{}", serde_json::to_string(&json)?);
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

pub(crate) async fn show_service_info(output_format: OutputFormat, service_id: &str) -> Result<()> {
    let registry = services::fetch_services().await?;

    let service = registry
        .services
        .iter()
        .find(|s| s.id.eq_ignore_ascii_case(service_id));

    let Some(service) = service else {
        bail!("service '{service_id}' not found");
    };

    match output_format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string(service)?);
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
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max - 1])
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
    let intents: Vec<&str> = s
        .methods
        .values()
        .flat_map(|m| m.intents.iter().map(|i| i.as_str()))
        .collect();
    if intents.is_empty() {
        "—".to_string()
    } else {
        let mut unique: Vec<&str> = intents;
        unique.sort();
        unique.dedup();
        unique.join(", ")
    }
}

// ---------------------------------------------------------------------------
// Text rendering — detail view
// ---------------------------------------------------------------------------

fn render_service_detail(s: &Service) {
    println!("{}", s.name);
    println!("{}", "─".repeat(s.name.len()));

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
        for ep in &s.endpoints {
            render_endpoint(ep);
        }
    }
}

fn render_endpoint(ep: &Endpoint) {
    let pricing = match &ep.payment {
        None => "free".to_string(),
        Some(p) => {
            let mut parts = Vec::new();

            if p.dynamic == Some(true) {
                parts.push("dynamic".to_string());
            } else if let Some(ref amount) = p.amount {
                let formatted = match p.decimals {
                    Some(d) if d > 0 => {
                        let divisor = 10u128.pow(d);
                        match amount.parse::<u128>() {
                            Ok(v) => {
                                format!("${:.prec$}", v as f64 / divisor as f64, prec = d as usize)
                            }
                            Err(_) => amount.clone(),
                        }
                    }
                    _ => amount.clone(),
                };
                parts.push(formatted);
            }

            parts.push(p.intent.clone());

            parts.join(" ")
        }
    };

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

    if let Some(ref docs_url) = ep.docs {
        println!("         docs: {docs_url}");
    }
}

fn print_field(label: &str, value: &str) {
    println!("{:>14}: {value}", label);
}
