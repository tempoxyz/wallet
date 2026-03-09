//! Top-level command dispatch and analytics tracking.

use anyhow::Result;

use crate::args::Cli;
use crate::commands;

impl Cli {
    /// Application entry point: build context, dispatch command, flush analytics.
    pub(crate) async fn run(self) -> Result<()> {
        let query = self.query;
        tempo_common::cli::run_cli(&self.global, &["tempo_request", "mpp"], |ctx| async move {
            tempo_common::cli::tracking::track_command(&ctx.analytics, "request");
            ("request", commands::run(&ctx, query).await)
        })
        .await
    }
}
