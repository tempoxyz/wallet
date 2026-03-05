//! Login command — sign up or log in to your Tempo wallet.

mod passkey;

use super::whoami::{show_whoami, show_whoami_stderr};
use crate::analytics::{self, Event};
use crate::cli::{Context, OutputFormat};
use crate::error::TempoWalletError;
use crate::util::sanitize_error;

use passkey::WalletManager;

pub(crate) async fn run(ctx: &Context) -> anyhow::Result<()> {
    let net_str = ctx.network.as_str().to_string();
    if let Some(ref a) = ctx.analytics {
        a.track(
            Event::LoginStarted,
            analytics::NetworkPayload {
                network: net_str.clone(),
            },
        );
    }

    let result = run_impl(ctx).await;

    if let Some(ref a) = ctx.analytics {
        match &result {
            Ok(()) => {
                a.track(
                    Event::LoginSuccess,
                    analytics::NetworkPayload { network: net_str },
                );
            }
            Err(e) => {
                let err_str = e.to_string();
                let is_timeout = err_str.contains("timed out")
                    || e.chain()
                        .find_map(|cause| cause.downcast_ref::<TempoWalletError>())
                        .is_some_and(|pe| matches!(pe, TempoWalletError::LoginExpired));

                if is_timeout {
                    a.track(
                        Event::LoginTimeout,
                        analytics::NetworkPayload { network: net_str },
                    );
                } else {
                    a.track(
                        Event::LoginFailure,
                        analytics::LoginFailurePayload {
                            network: net_str,
                            error: sanitize_error(&err_str),
                        },
                    );
                }
            }
        }
    }

    result
}

async fn run_impl(ctx: &Context) -> anyhow::Result<()> {
    // Skip login if a wallet is already connected with a key for the target network.
    if ctx.keys.has_wallet() {
        let has_key_for_network = ctx
            .keys
            .keys
            .iter()
            .any(|k| k.chain_id == ctx.network.chain_id());

        if has_key_for_network {
            if ctx.output_format == OutputFormat::Text {
                println!("Already logged in.\n");
            }

            show_whoami(&ctx.config, ctx.output_format, ctx.network, None, &ctx.keys).await?;
            return Ok(());
        }
    }

    let manager = WalletManager::new(ctx.network, ctx.analytics.clone());
    let wallet_address = manager.setup_wallet(&ctx.keys).await?;

    // Ensure a config file exists so the user has something to edit.
    let _ = ctx.config.save();

    if ctx.output_format == OutputFormat::Text {
        eprintln!("\nWallet connected!\n");
        let fresh_keys = ctx.keys.reload()?;
        show_whoami_stderr(&ctx.config, ctx.network, Some(&wallet_address), &fresh_keys).await?;
    } else {
        let fresh_keys = ctx.keys.reload()?;
        show_whoami(
            &ctx.config,
            ctx.output_format,
            ctx.network,
            Some(&wallet_address),
            &fresh_keys,
        )
        .await?;
    }

    Ok(())
}
