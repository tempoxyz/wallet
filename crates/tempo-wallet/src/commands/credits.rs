//! Credits balance lookup.

use std::io::Write;

use alloy::primitives::Address;
use serde::Serialize;

use crate::commands::fund;
use tempo_common::{
    cli::{context::Context, output, output::OutputFormat},
    error::{ConfigError, TempoError},
};

#[derive(Debug, Serialize)]
struct CreditsResponse {
    wallet: String,
    balance: String,
    raw_balance: String,
}

pub(crate) async fn run(ctx: &Context, address: Option<String>) -> Result<(), TempoError> {
    let auth_server_url =
        std::env::var("TEMPO_AUTH_URL").unwrap_or_else(|_| ctx.network.auth_url().to_string());
    let wallet = fund::resolve_address(address, &ctx.keys)?;
    let wallet_address: Address = wallet.parse().map_err(|_| ConfigError::InvalidAddress {
        context: "wallet address",
        value: wallet.clone(),
    })?;
    let credit_seed = fund::fetch_credit_seed(&auth_server_url).await?;
    let raw_balance = fund::query_credit_balance(wallet_address, &credit_seed).await?;
    let response = CreditsResponse {
        wallet,
        balance: fund::format_credit_balance(raw_balance),
        raw_balance: raw_balance.to_string(),
    };

    response.render(ctx.output_format)
}

impl CreditsResponse {
    fn render(&self, format: OutputFormat) -> Result<(), TempoError> {
        output::emit_by_format(format, self, || {
            let w = &mut std::io::stdout();
            writeln!(w, "{:>10}: {}", "Wallet", self.wallet)?;
            writeln!(w, "{:>10}: {}", "Credits", self.balance)?;
            Ok(())
        })
    }
}
