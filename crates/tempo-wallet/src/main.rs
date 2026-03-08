#![forbid(unsafe_code)]
#![deny(warnings)]
#![warn(unreachable_pub)]

pub(crate) mod account;
mod cli;
pub(crate) mod prompt;

use crate::cli::Cli;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let output_format = cli.resolve_output_format();
    let result = cli.run().await;
    tempo_common::cli::run_main(output_format, result);
}
