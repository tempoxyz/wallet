#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::format_push_string,
    clippy::future_not_send,
    clippy::items_after_statements,
    clippy::map_unwrap_or,
    clippy::needless_pass_by_value,
    clippy::option_if_let_else,
    clippy::redundant_pub_crate,
    clippy::significant_drop_tightening,
    clippy::struct_excessive_bools,
    clippy::too_many_lines,
    clippy::useless_let_if_seq
)]

mod analytics;
mod app;
mod args;
mod http;
mod payment;
mod query;

use crate::args::Cli;

#[tokio::main]
async fn main() {
    let cli: Cli = tempo_common::cli::parse_cli();
    let output_format = cli.global.resolve_output_format();
    let result = app::run(cli).await;
    tempo_common::cli::run_main(output_format, result);
}
