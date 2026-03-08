//! Top-level command dispatch and analytics tracking.

use anyhow::Result;

use super::commands::query;
use super::Cli;
use tempo_common::cli::dispatch::{track_command, track_result};
use tempo_common::context::ContextArgs;
use tempo_common::runtime;

impl Cli {
    /// Application entry point: build context, dispatch command, flush analytics.
    pub(crate) async fn run(self) -> Result<()> {
        runtime::init_tracing(
            self.global.silent,
            self.global.verbose,
            &["tempo_request", "mpp"],
        );
        runtime::init_color_support(self.global.color);

        let ctx = tempo_common::context::Context::build(ContextArgs {
            config_path: self.global.config.clone(),
            rpc_url: self.global.rpc_url.clone(),
            requested_network: self.global.network.clone(),
            private_key: self.global.private_key.clone(),
            output_format: self.resolve_output_format(),
            verbosity: self.global.verbosity(),
        })
        .await?;
        track_command(&ctx.analytics, "request");

        let result = query::run(&ctx, self.query).await;

        track_result(&ctx.analytics, "request", &result);

        ctx.flush_analytics().await;

        result
    }
}
