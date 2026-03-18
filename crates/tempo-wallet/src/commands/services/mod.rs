//! Service directory command entrypoint.

mod client;
mod model;
mod render;

use tempo_common::{
    cli::context::Context,
    error::{ConfigError, TempoError},
};

use client::{fetch_services, simple_client};
use render::{render_service_detail, render_service_list};

/// CLI arguments for the `services` command, unpacked from the enum variant.
pub(crate) struct ServicesArgs {
    pub(crate) service_id: Option<String>,
    pub(crate) search: Option<String>,
}

pub(crate) async fn run(ctx: &Context, args: ServicesArgs) -> Result<(), TempoError> {
    let client = simple_client()?;
    let registry = fetch_services(&client).await?;
    let output_format = ctx.output_format;

    // `services <ID>` → detail view
    let info_id = args.service_id.as_deref();

    if let Some(id) = info_id {
        let Some(service) = registry.find(id) else {
            // Intentional business-rule message for CLI compatibility; this is a
            // lookup miss rather than a lower-level source failure.
            return Err(ConfigError::Missing(format!("service '{id}' not found")).into());
        };
        return render_service_detail(service, output_format);
    }

    // `services list` or `services` (with optional --search)
    render_service_list(&registry.services, output_format, args.search.as_deref())
}
