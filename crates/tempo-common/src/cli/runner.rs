//! CLI lifecycle: command execution with shared setup and teardown.

use super::{
    args::GlobalArgs,
    context, exit_codes,
    output::{self, OutputFormat},
    runtime,
    terminal::sanitize_for_terminal,
    tracking,
};
use crate::{
    analytics::{events, KeystoreLoadDegradedPayload, SessionStoreDegradedPayload},
    error::{PaymentError, TempoError},
    keys,
    network::NetworkId,
    payment::session,
};

/// Run a CLI command with shared setup and teardown.
///
/// Handles the boilerplate that every extension binary repeats:
/// 1. Initialize tracing and color support
/// 2. Build the shared `Context`
/// 3. Run the handler
/// 4. Track success/failure (with duration) and flush analytics
///
/// The handler receives the `Context` and returns a `(&str, Result)` tuple
/// where the `&str` is the analytics command name.
///
/// ```ignore
/// cli::run_cli(&global, &["tempo_wallet"], "tempo-wallet", |ctx| async move {
///     let cmd_name = command_name(&command);
///     (cmd_name, do_work(&ctx, command).await)
/// }).await
/// ```
///
/// # Errors
///
/// Returns an error when context construction fails or the provided handler
/// returns an error.
pub async fn run_cli<F, Fut, E>(
    global: &GlobalArgs,
    target_crates: &[&str],
    app_id: &'static str,
    handler: F,
) -> Result<(), TempoError>
where
    F: FnOnce(context::Context) -> Fut,
    Fut: std::future::Future<Output = (&'static str, Result<(), E>)>,
    E: std::fmt::Display + Into<TempoError>,
{
    runtime::init_tracing(global.silent, global.verbose, target_crates);
    runtime::init_color_support(global.color);
    global.warn_argv_private_key();

    let ctx = global.build_context(app_id).await?;
    let analytics = ctx.analytics.clone();
    let ctx_network = ctx.network;
    let ctx_config = ctx.config.clone();
    let ctx_keys = ctx.keys.clone();

    let start = std::time::Instant::now();
    let (cmd_name, result) = handler(ctx).await;
    let duration = start.elapsed();

    let session_store_diagnostics = session::take_channel_store_diagnostics();
    let keystore_diagnostics = keys::take_keystore_load_summary();
    if let Some(ref a) = analytics {
        if session_store_diagnostics.malformed_load_drops > 0
            || session_store_diagnostics.malformed_list_drops > 0
        {
            a.track(
                events::SESSION_STORE_DEGRADED,
                SessionStoreDegradedPayload {
                    malformed_load_drops: session_store_diagnostics.malformed_load_drops,
                    malformed_list_drops: session_store_diagnostics.malformed_list_drops,
                },
            );
        }

        if keystore_diagnostics.strict_parse_failures > 0
            || keystore_diagnostics.salvage_malformed_entries > 0
            || keystore_diagnostics.filtered_invalid_entries > 0
        {
            a.track(
                events::KEYSTORE_LOAD_DEGRADED,
                KeystoreLoadDegradedPayload {
                    strict_parse_failures: keystore_diagnostics.strict_parse_failures,
                    salvage_malformed_entries: keystore_diagnostics.salvage_malformed_entries,
                    filtered_invalid_entries: keystore_diagnostics.filtered_invalid_entries,
                },
            );
        }
    }

    tracking::track_result(&analytics, cmd_name, &result, duration);
    if let Some(ref a) = analytics {
        a.flush().await;
    }

    let final_result = result.map_err(Into::into);
    let final_result = maybe_map_inactive_access_key_rejection(final_result, || async {
        is_access_key_inactive_on_chain(&ctx_config, &ctx_keys, ctx_network).await
    })
    .await;

    // Auto-invalidate revoked access keys so the next `login` re-authorizes
    if matches!(
        &final_result,
        Err(TempoError::Payment(
            crate::error::PaymentError::AccessKeyRevoked
        ))
    ) {
        if let Ok(mut ks) = keys::Keystore::load(None) {
            if let Some(entry) = ks.key_for_network(ctx_network) {
                if let Some(wallet) = entry.wallet_address_parsed() {
                    let _ = ks.delete_passkey_wallet_address(wallet);
                    let _ = ks.save();
                    tracing::info!(
                        "revoked access key removed — run 'tempo wallet login' to re-authorize"
                    );
                }
            }
        }
    }

    final_result
}

async fn maybe_map_inactive_access_key_rejection<F, Fut>(
    result: Result<(), TempoError>,
    check_on_chain_inactive: F,
) -> Result<(), TempoError>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    match result {
        Err(err) => {
            let is_inactive_candidate = matches!(
                &err,
                TempoError::Payment(PaymentError::PaymentRejected { reason, .. })
                    if crate::payment::is_inactive_access_key_error(reason)
            );

            if is_inactive_candidate && check_on_chain_inactive().await {
                Err(TempoError::Payment(PaymentError::AccessKeyRevoked))
            } else {
                Err(err)
            }
        }
        Ok(()) => Ok(()),
    }
}

async fn is_access_key_inactive_on_chain(
    config: &crate::config::Config,
    keys: &keys::Keystore,
    network: NetworkId,
) -> bool {
    let Some(key_entry) = keys.key_for_network(network) else {
        return false;
    };
    if key_entry.is_direct_eoa_key() {
        return false;
    }
    let Some(wallet_address) = key_entry.wallet_address_parsed() else {
        return false;
    };
    let Some(key_address) = key_entry.key_address_parsed() else {
        return false;
    };

    let provider = alloy::providers::ProviderBuilder::new().connect_http(config.rpc_url(network));
    let token = network.token();

    match mpp::client::tempo::signing::keychain::query_key_spending_limit(
        &provider,
        wallet_address,
        key_address,
        token.address,
    )
    .await
    {
        Ok(_) => false,
        Err(err) => {
            let msg = err.to_string().to_ascii_lowercase();
            msg.contains("revoked") || msg.contains("expired")
        }
    }
}

/// Run a CLI binary with shared error handling.
///
/// Handles structured error output for JSON/TOON formats and sets the process
/// exit code based on the error type.
pub fn run_main(output_format: OutputFormat, result: Result<(), TempoError>) {
    let Err(e) = result else { return };

    match output_format {
        OutputFormat::Json | OutputFormat::Toon => {
            let code = exit_codes::ExitCode::from(&e).label();
            let payload = runtime::render_error_payload(&e, code);
            output::emit_formatted_or_fallback(
                || output::format_structured(output_format, &payload),
                || runtime::render_error_fallback(code),
            );
        }
        OutputFormat::Text => {
            let rendered = format!("{e:#}");
            let safe_error = sanitize_for_terminal(&rendered);
            eprintln!("Error: {safe_error}");
        }
    }
    exit_codes::ExitCode::from(&e).exit();
}

#[cfg(test)]
mod tests {
    use super::maybe_map_inactive_access_key_rejection;
    use crate::error::{PaymentError, TempoError};

    #[tokio::test]
    async fn maps_inactive_shape_to_access_key_revoked_when_on_chain_check_confirms() {
        let result = Err(TempoError::Payment(PaymentError::PaymentRejected {
            reason: "Payment verification failed: Missing or invalid parameters. eth_sendRawTransactionSync"
                .to_string(),
            status_code: 402,
        }));

        let mapped = maybe_map_inactive_access_key_rejection(result, || async { true }).await;
        assert!(matches!(
            mapped,
            Err(TempoError::Payment(PaymentError::AccessKeyRevoked))
        ));
    }

    #[tokio::test]
    async fn keeps_payment_rejected_when_on_chain_check_is_negative() {
        let result = Err(TempoError::Payment(PaymentError::PaymentRejected {
            reason: "Payment verification failed: Missing or invalid parameters. eth_sendRawTransactionSync"
                .to_string(),
            status_code: 402,
        }));

        let mapped = maybe_map_inactive_access_key_rejection(result, || async { false }).await;
        assert!(matches!(
            mapped,
            Err(TempoError::Payment(PaymentError::PaymentRejected { .. }))
        ));
    }

    #[tokio::test]
    async fn keeps_other_payment_rejections_unchanged() {
        let result = Err(TempoError::Payment(PaymentError::PaymentRejected {
            reason: "provider internal error".to_string(),
            status_code: 500,
        }));

        let mapped = maybe_map_inactive_access_key_rejection(result, || async { true }).await;
        assert!(matches!(
            mapped,
            Err(TempoError::Payment(PaymentError::PaymentRejected { .. }))
        ));
    }
}
