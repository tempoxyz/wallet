//! Rendering functions for the services command.

use anyhow::Result;

use tempo_common::cli::output;
use tempo_common::cli::output::OutputFormat;
use tempo_common::display::terminal::{print_field, truncate};

use super::model::Service;

pub(super) fn render_service_list(
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

pub(super) fn render_service_detail(service: &Service, output_format: OutputFormat) -> Result<()> {
    output::emit_by_format(output_format, service, || {
        render_detail(service);
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
