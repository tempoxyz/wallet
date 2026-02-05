//! Display formatting for service directory.

use crate::services::{Directory, Service};

pub fn print_service_list(directory: &Directory, json: bool) {
    if json {
        if let Ok(json_str) = serde_json::to_string_pretty(&directory.services) {
            println!("{}", json_str);
        }
        return;
    }

    let name_width = directory
        .services
        .iter()
        .map(|s| s.slug.len())
        .max()
        .unwrap_or(4)
        .max(4);

    let aliases_width = directory
        .services
        .iter()
        .map(|s| s.aliases_string().len())
        .max()
        .unwrap_or(7)
        .max(7);

    println!(
        "{:<name_width$}  {:<aliases_width$}  URL",
        "NAME", "ALIASES"
    );

    for service in &directory.services {
        let url_display = service.url.strip_prefix("https://").unwrap_or(&service.url);

        println!(
            "{:<name_width$}  {:<aliases_width$}  {}",
            service.slug,
            service.aliases_string(),
            url_display
        );
    }
}

pub fn print_service_info(service: &Service, json: bool) {
    if json {
        if let Ok(json_str) = serde_json::to_string_pretty(service) {
            println!("{}", json_str);
        }
        return;
    }

    println!("{} ({})", service.name, service.slug);

    if !service.aliases.is_empty() {
        println!("Aliases: {}", service.aliases_string());
    }

    println!("URL: {}", service.url);
    println!();

    if service.pricing.endpoints.is_empty() {
        println!("No endpoints defined");
        return;
    }

    let path_width = service
        .pricing
        .endpoints
        .iter()
        .map(|e| e.path.len())
        .max()
        .unwrap_or(8)
        .max(8);

    let method_width = service
        .pricing
        .endpoints
        .iter()
        .map(|e| e.methods.join(",").len())
        .max()
        .unwrap_or(6)
        .max(6);

    let price_width = service
        .pricing
        .endpoints
        .iter()
        .map(|e| e.price_display().len())
        .max()
        .unwrap_or(5)
        .max(5);

    println!(
        "{:<path_width$}  {:<method_width$}  {:<price_width$}  DESCRIPTION",
        "ENDPOINT", "METHOD", "PRICE"
    );

    for endpoint in &service.pricing.endpoints {
        let methods = endpoint.methods.join(",");
        println!(
            "{:<path_width$}  {:<method_width$}  {:<price_width$}  {}",
            endpoint.path,
            methods,
            endpoint.price_display(),
            endpoint.description
        );
    }
}
