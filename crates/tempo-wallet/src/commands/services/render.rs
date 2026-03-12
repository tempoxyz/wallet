//! Rendering functions for the services command.

use anyhow::Result;
use serde::Serialize;

use tempo_common::cli::output;
use tempo_common::cli::output::OutputFormat;
use tempo_common::cli::terminal::{print_field, sanitize_for_terminal, truncate};

use super::model::{EndpointPayment, Service, ServiceDocs};

// ── List serialization structs ───────────────────────────────────────

#[derive(Serialize)]
struct ServiceListItem<'a> {
    id: &'a str,
    name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    service_url: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
    categories: Vec<&'a str>,
    tags: Vec<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    docs: Option<&'a ServiceDocs>,
    endpoints: Vec<EndpointListItem<'a>>,
}

#[derive(Serialize)]
struct EndpointListItem<'a> {
    method: &'a str,
    path: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    docs: Option<&'a str>,
}

// ── Detail serialization structs ─────────────────────────────────────

#[derive(Serialize)]
struct ServiceDetail<'a> {
    id: &'a str,
    name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    service_url: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
    categories: Vec<&'a str>,
    tags: Vec<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    docs: Option<&'a ServiceDocs>,
    endpoints: Vec<EndpointDetailItem<'a>>,
}

#[derive(Serialize)]
struct EndpointDetailItem<'a> {
    method: &'a str,
    path: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    payment: Option<&'a EndpointPayment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    docs: Option<&'a str>,
}

// ── Public rendering entry points ────────────────────────────────────

pub(super) fn render_service_list(
    services: &[Service],
    output_format: OutputFormat,
    search: Option<&str>,
) -> Result<()> {
    let filtered: Vec<&Service> = services
        .iter()
        .filter(|s| {
            if let Some(q) = search {
                let q_lower = q.to_lowercase();
                let matches = s.name.to_lowercase().contains(&q_lower)
                    || s.id.to_lowercase().contains(&q_lower)
                    || s.description
                        .as_ref()
                        .is_some_and(|d| d.to_lowercase().contains(&q_lower))
                    || s.tags.iter().any(|t| t.to_lowercase().contains(&q_lower))
                    || s.categories
                        .iter()
                        .any(|c| c.to_lowercase().contains(&q_lower));
                if !matches {
                    return false;
                }
            }
            true
        })
        .collect();

    let list_items: Vec<ServiceListItem> = filtered
        .iter()
        .map(|s| ServiceListItem {
            id: &s.id,
            name: &s.name,
            url: Some(&s.url),
            service_url: s.service_url.as_deref(),
            description: s.description.as_deref(),
            categories: s.categories.iter().map(|c| c.as_str()).collect(),
            tags: s.tags.iter().map(|t| t.as_str()).collect(),
            docs: s.docs.as_ref(),
            endpoints: s
                .endpoints
                .iter()
                .map(|ep| EndpointListItem {
                    method: &ep.method,
                    path: &ep.path,
                    description: ep.description.as_deref(),
                    docs: ep.docs.as_deref(),
                })
                .collect(),
        })
        .collect();

    output::emit_by_format(output_format, &list_items, || {
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
    let detail = ServiceDetail {
        id: &service.id,
        name: &service.name,
        url: Some(&service.url),
        service_url: service.service_url.as_deref(),
        description: service.description.as_deref(),
        categories: service.categories.iter().map(|c| c.as_str()).collect(),
        tags: service.tags.iter().map(|t| t.as_str()).collect(),
        docs: service.docs.as_ref(),
        endpoints: service
            .endpoints
            .iter()
            .map(|ep| EndpointDetailItem {
                method: &ep.method,
                path: &ep.path,
                description: ep.description.as_deref(),
                payment: ep.payment.as_ref(),
                docs: ep.docs.as_deref(),
            })
            .collect(),
    };

    output::emit_by_format(output_format, &detail, || {
        render_detail(service);
        Ok(())
    })?;
    Ok(())
}

// ── Private rendering helpers ────────────────────────────────────────

fn render_table(services: &[&Service]) {
    const MAX_ID: usize = 20;
    const MAX_NAME: usize = 24;
    const MAX_CAT: usize = 16;

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

    println!(
        "  {:<w_id$}  {:<w_name$}  {:<w_cat$}  Service URL",
        "ID", "Name", "Category"
    );
    let total_w = 2 + w_id + 2 + w_name + 2 + w_cat + 2 + 30;
    println!("  {}", "─".repeat(total_w));

    for s in services {
        let id = truncate(&s.id, MAX_ID);
        let name = truncate(&s.name, MAX_NAME);
        let categories = truncate(&s.format_categories(), MAX_CAT);
        let service_url = sanitize_for_terminal(s.service_url.as_deref().unwrap_or("—"));

        println!(
            "  {:<w_id$}  {:<w_name$}  {:<w_cat$}  {}",
            id, name, categories, service_url
        );
    }

    println!("\n{} service(s).", services.len());
}

fn render_detail(s: &Service) {
    let safe_name = sanitize_for_terminal(&s.name);
    println!("{safe_name}");
    println!("{}", "─".repeat(safe_name.chars().count()));

    if let Some(desc) = &s.description {
        println!("{}", sanitize_for_terminal(desc));
    }
    println!();

    print_field("ID", &s.id);
    print_field("Categories", &s.format_categories());
    print_field("Service URL", s.service_url.as_deref().unwrap_or("—"));
    print_field("Upstream URL", &s.url);

    if !s.tags.is_empty() {
        print_field("Tags", &s.tags.join(", "));
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
            let pricing = sanitize_for_terminal(&ep.format_pricing());
            let method = sanitize_for_terminal(&ep.method);
            let path = sanitize_for_terminal(&ep.path);
            println!("  {:>6} {:<40} {}", method, path, pricing);

            let desc = ep
                .description
                .as_deref()
                .or_else(|| ep.payment.as_ref().and_then(|p| p.description.as_deref()));
            if let Some(desc) = desc {
                println!("         {}", sanitize_for_terminal(desc));
            }

            if let Some(unit_type) = ep.payment.as_ref().and_then(|p| p.unit_type.as_deref()) {
                println!("         per {}", sanitize_for_terminal(unit_type));
            }

            let full_url = format!("{}{}", base_url.trim_end_matches('/'), ep.path);
            let safe_url = sanitize_for_terminal(&full_url);
            let example = match ep.method.to_uppercase().as_str() {
                "GET" => format!("tempo request {safe_url}"),
                m => format!("tempo request -X {m} --json '{{}}' {safe_url}"),
            };
            println!("         example: {example}");

            if let Some(docs_url) = &ep.docs {
                println!("         docs: {}", sanitize_for_terminal(docs_url));
            }
        }
    }
}
