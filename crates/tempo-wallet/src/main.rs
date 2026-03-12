#![forbid(unsafe_code)]
#![deny(warnings)]
#![warn(unreachable_pub)]

mod analytics;
mod app;
mod args;
mod commands;
mod prompt;
mod wallet;

use crate::args::Cli;

#[tokio::main]
async fn main() {
    let cli: Cli = tempo_common::cli::parse_cli();
    let output_format = cli.global.resolve_output_format();
    let result = app::run(cli).await;
    tempo_common::cli::run_main(output_format, result);
}
