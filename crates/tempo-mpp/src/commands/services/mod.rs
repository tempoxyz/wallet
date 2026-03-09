//! Service directory command entrypoint.

mod client;
mod model;
mod render;

use anyhow::{bail, Result};

use crate::args::ServicesCommands;
use tempo_common::cli::context::Context;

use client::{fetch_services, simple_client};
use render::{render_service_detail, render_service_list};

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
