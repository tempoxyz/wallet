//! Service directory commands.

use crate::cli::OutputFormat;
use crate::error::{PgetError, Result};
use crate::services::{print_service_info, print_service_list, Directory};

/// List all available services.
pub async fn list_services(output_format: OutputFormat, refresh: bool) -> Result<()> {
    let directory = Directory::load(refresh).await?;
    let json = matches!(output_format, OutputFormat::Json);
    print_service_list(&directory, json);
    Ok(())
}

/// Show info for a specific service.
pub async fn show_service(name: &str, output_format: OutputFormat) -> Result<()> {
    let directory = Directory::load(false).await?;

    match directory.find_service(name) {
        Some(service) => {
            let json = matches!(output_format, OutputFormat::Json);
            print_service_info(service, json);
            Ok(())
        }
        None => Err(PgetError::ConfigMissing(format!(
            "Service '{}' not found. Run 'pget services' to see available services.",
            name
        ))),
    }
}
