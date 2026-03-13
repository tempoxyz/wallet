//! Application entry point: build context, dispatch command, flush analytics.

use crate::args::Cli;
use tempo_common::error::TempoError;
/// Run the tempo-request application.
pub(crate) async fn run(cli: Cli) -> Result<(), TempoError> {
    let query = cli.query;
    tempo_common::cli::run_cli(
        &cli.global,
        &["tempo_request", "mpp"],
        "tempo-request",
        |ctx| async move { ("request", crate::query::run(&ctx, query).await) },
    )
    .await
}
