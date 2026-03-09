//! CLI lifecycle: command execution with shared setup and teardown.

use super::args::GlobalArgs;
use super::context;
use super::exit_code;
use super::output::{self, OutputFormat};
use super::runtime;
use super::tracking;

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
pub async fn run_cli<F, Fut>(
    global: &GlobalArgs,
    target_crates: &[&str],
    handler: F,
) -> anyhow::Result<()>
where
    F: FnOnce(context::Context) -> Fut,
    Fut: std::future::Future<Output = (&'static str, anyhow::Result<()>)>,
{
    runtime::init_tracing(global.silent, global.verbose, target_crates);
    runtime::init_color_support(global.color);

    let ctx = global.build_context().await?;
    let analytics = ctx.analytics.clone();

    let (cmd_name, result) = handler(ctx).await;

    tracking::track_result(&analytics, cmd_name, &result);
    if let Some(ref a) = analytics {
        a.flush().await;
    }

    result
}

/// Run a CLI binary with shared error handling.
///
/// Handles structured error output for JSON/TOON formats and sets the process
/// exit code based on the error type.
pub fn run_main(output_format: OutputFormat, result: Result<(), anyhow::Error>) {
    let Err(e) = result else { return };

    match output_format {
        OutputFormat::Json | OutputFormat::Toon => {
            let code = exit_code::ExitCode::from(&e).label();
            let payload = runtime::render_error_payload(&e, code);
            output::emit_formatted_or_fallback(
                || output::format_structured(output_format, &payload),
                || runtime::render_error_fallback(code),
            );
        }
        _ => {
            eprintln!("Error: {e:#}");
        }
    }
    exit_code::ExitCode::from(&e).exit();
}
