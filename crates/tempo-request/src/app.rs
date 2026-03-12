//! Application entry point: build context, dispatch command, flush analytics.

use anyhow::Result;

use crate::args::Cli;
/// Run the tempo-request application.
pub(crate) async fn run(cli: Cli) -> Result<()> {
    let query = cli.query;
    tempo_common::cli::run_cli(&cli.global, &["tempo_request", "mpp"], |ctx| async move {
        tempo_common::cli::tracking::track_command(&ctx.analytics, "request");
        ("request", crate::query::run(&ctx, query).await)
    })
    .await
}
