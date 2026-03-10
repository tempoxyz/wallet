//! Wallet management commands — create, list, fund.

mod create;
mod fund;
mod keychain;
mod list;

use anyhow::Result;

use crate::analytics;
use crate::analytics::{WalletCreatedPayload, WalletFundFailurePayload, WalletFundPayload};
use tempo_common::cli::context::Context;
use tempo_common::network::NetworkId;
use tempo_common::security::sanitize_error;

pub(crate) fn list(ctx: &Context) -> Result<()> {
    list::run(ctx)
}

pub(crate) async fn create(ctx: &Context) -> Result<()> {
    let result = create::create_local_wallet(&ctx.network, &ctx.keys);
    if result.is_ok() {
        ctx.track(
            analytics::WALLET_CREATED,
            WalletCreatedPayload {
                wallet_type: "local".to_string(),
            },
        );
    }
    let wallet_addr = result?;
    let fresh_keys = ctx.keys.reload()?;
    super::whoami::show_whoami(ctx, Some(&fresh_keys), Some(&wallet_addr)).await
}

pub(crate) async fn fund(ctx: &Context, address: Option<String>, no_wait: bool, dry_run: bool) -> Result<()> {
    let method = match ctx.network {
        NetworkId::TempoModerato => "faucet",
        NetworkId::Tempo => "bridge",
    };
    if !dry_run {
        track_fund_start(ctx, method);
    }
    let result = fund::run(ctx, address, no_wait, dry_run).await;
    if !dry_run {
        track_fund_result(ctx, method, &result);
    }
    result
}

fn track_fund_start(ctx: &Context, method: &str) {
    ctx.track(
        analytics::WALLET_FUND_STARTED,
        WalletFundPayload {
            network: ctx.network.as_str().to_string(),
            method: method.to_string(),
        },
    );
}

fn track_fund_result(ctx: &Context, method: &str, result: &Result<()>) {
    match result {
        Ok(()) => {
            ctx.track(
                analytics::WALLET_FUND_SUCCESS,
                WalletFundPayload {
                    network: ctx.network.as_str().to_string(),
                    method: method.to_string(),
                },
            );
        }
        Err(e) => {
            ctx.track(
                analytics::WALLET_FUND_FAILURE,
                WalletFundFailurePayload {
                    network: ctx.network.as_str().to_string(),
                    method: method.to_string(),
                    error: sanitize_error(&e.to_string()),
                },
            );
        }
    }
}

// Re-export for keys command
pub(crate) use create::create_access_key;
