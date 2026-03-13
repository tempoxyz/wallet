//! CLI lifecycle: command execution with shared setup and teardown.

use super::{
    args::GlobalArgs,
    context, exit_codes,
    output::{self, OutputFormat},
    runtime, tracking,
};
use crate::{
    analytics::{events, KeystoreLoadDegradedPayload, SessionStoreDegradedPayload},
    error::TempoError,
    keys,
    payment::session,
};

/// Run a CLI command with shared setup and teardown.
///
/// Handles the boilerplate that every extension binary repeats:
/// 1. Initialize tracing and color support
/// 2. Build the shared `Context`
/// 3. Track the command run event
/// 4. Run the handler
/// 5. Track success/failure and flush analytics
///
/// The handler receives the `Context` and returns a `(&str, Result)` tuple
/// where the `&str` is the analytics command name.
///
/// ```ignore
/// cli::run_cli(&global, &["tempo_wallet"], |ctx| async move {
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
    handler: F,
) -> Result<(), TempoError>
where
    F: FnOnce(context::Context) -> Fut,
    Fut: std::future::Future<Output = (&'static str, Result<(), E>)>,
    E: std::fmt::Display + Into<TempoError>,
{
    runtime::init_tracing(global.silent, global.verbose, target_crates);
    runtime::init_color_support(global.color);

    let ctx = global.build_context().await?;
    let analytics = ctx.analytics.clone();

    let (cmd_name, result) = handler(ctx).await;

    let session_store_diagnostics = session::take_store_diagnostics();
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

    tracking::track_result(&analytics, cmd_name, &result);
    if let Some(ref a) = analytics {
        a.flush().await;
    }

    result.map_err(Into::into)
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
            eprintln!("Error: {e:#}");
        }
    }
    exit_codes::ExitCode::from(&e).exit();
}
