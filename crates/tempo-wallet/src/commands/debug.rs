//! Debug info collection for support tickets.

use std::io::Write;

use serde::Serialize;

use crate::commands::whoami;
use tempo_common::{
    cli::{context::Context, output},
    error::TempoError,
};

/// Long version string for tempo-wallet (matches args.rs).
const WALLET_VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("TEMPO_GIT_SHA"),
    " ",
    env!("TEMPO_BUILD_DATE"),
    " ",
    env!("TEMPO_BUILD_PROFILE"),
    ")"
);

#[derive(Debug, Serialize)]
struct DebugInfo {
    wallet_version: String,
    request_version: String,
    os: String,
    arch: String,
    network: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    wallet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    wallet_type: Option<String>,
    logged_in: bool,
}

pub(crate) async fn run(ctx: &Context) -> Result<(), TempoError> {
    let info = build_debug_info(ctx);
    render(ctx, &info).await
}

fn build_debug_info(ctx: &Context) -> DebugInfo {
    let has_wallet = ctx.keys.has_wallet();

    let (wallet, wallet_type) = if has_wallet {
        let entry = ctx.keys.key_for_network(ctx.network);
        let addr = entry.and_then(|e| e.wallet_address_hex());
        let wtype = entry.map(|e| e.wallet_type.as_str().to_string());
        (addr, wtype)
    } else {
        (None, None)
    };

    DebugInfo {
        wallet_version: WALLET_VERSION.to_string(),
        request_version: WALLET_VERSION.to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        network: ctx.network.to_string(),
        wallet,
        wallet_type,
        logged_in: has_wallet,
    }
}

async fn render(ctx: &Context, info: &DebugInfo) -> Result<(), TempoError> {
    output::emit_by_format(ctx.output_format, info, || {
        let w = &mut std::io::stdout();

        writeln!(w, "tempo debug")?;
        writeln!(w, "===========")?;
        writeln!(w)?;
        writeln!(w, "  tempo wallet  : {}", info.wallet_version)?;
        writeln!(w, "  tempo request : {}", info.request_version)?;
        writeln!(w, "  os            : {} ({})", info.os, info.arch)?;
        writeln!(w, "  network       : {}", info.network)?;
        writeln!(w)?;

        if info.logged_in {
            writeln!(
                w,
                "  wallet        : {}",
                info.wallet.as_deref().unwrap_or("—")
            )?;
            writeln!(
                w,
                "  wallet type   : {}",
                info.wallet_type.as_deref().unwrap_or("—")
            )?;
        } else {
            writeln!(w, "  wallet        : not logged in")?;
        }

        Ok(())
    })?;

    // In text mode, also print full whoami output below the debug block
    if !ctx.output_format.is_structured() && info.logged_in {
        let w = &mut std::io::stdout();
        writeln!(w)?;
        writeln!(w, "wallet and access key")?;
        writeln!(w, "=====================")?;
        whoami::show_whoami(ctx, None, None).await?;
    }

    if !ctx.output_format.is_structured() {
        let w = &mut std::io::stdout();
        writeln!(w)?;
        writeln!(w, "Copy the above and share it with Tempo support.")?;
    }

    Ok(())
}
