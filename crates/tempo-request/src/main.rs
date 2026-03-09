#![forbid(unsafe_code)]
#![deny(warnings)]
#![warn(unreachable_pub)]

pub(crate) mod analytics;
mod args;
mod commands;
mod dispatch;
mod http;
pub(crate) mod output;
mod payment;

use crate::args::Cli;

#[tokio::main]
async fn main() {
    let cli: Cli = tempo_common::cli::parse_cli();
    let output_format = cli.global.resolve_output_format();
    let result = cli.run().await;
    tempo_common::cli::run_main(output_format, result);
}
