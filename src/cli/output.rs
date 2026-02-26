//! Output formatting and display utilities for the CLI.

use anyhow::{Context, Result};

use crate::cli::exit_codes::ExitCode;
use crate::config::validate_path;
use crate::http::HttpResponse;

use super::OutputFormat;

/// Output/display options extracted from CLI arguments.
///
/// Used by response formatting functions; kept separate from
/// `RequestRuntime` to avoid coupling HTTP/payment layers to
/// presentation concerns.
#[derive(Clone, Debug)]
pub(crate) struct OutputOptions {
    pub output_format: OutputFormat,
    pub include_headers: bool,
    pub output_file: Option<String>,
    pub verbosity: u8,
    pub show_output: bool,
    pub fail_silently: bool,
    pub dump_headers: Option<String>,
}

impl OutputOptions {
    /// Whether agent-level log messages should be printed (`-v`).
    pub fn log_enabled(&self) -> bool {
        self.verbosity >= 1 && self.show_output
    }

    /// Whether payment summaries should be printed (always, unless `--quiet`).
    pub fn payment_log_enabled(&self) -> bool {
        self.show_output
    }
}

// ---------------------------------------------------------------------------
// Response output
// ---------------------------------------------------------------------------

/// Handle a regular (non-402) HTTP response
pub(crate) fn handle_regular_response(opts: &OutputOptions, response: HttpResponse) -> Result<()> {
    // If -f/--fail is set and the response is an error, suppress body output.
    // Still honor -D/--dump-header if requested.
    if opts.fail_silently && response.status_code >= 400 {
        if let Some(ref path) = opts.dump_headers {
            write_headers_file(opts, path, &response)?;
        }
        return Ok(());
    }
    match opts.output_format {
        OutputFormat::Json => {
            if let Ok(json_value) = serde_json::from_slice::<serde_json::Value>(&response.body) {
                let output = serde_json::to_string_pretty(&json_value)?;
                write_output_to(opts, output)?;
            } else {
                output_response_body(opts, &response.body)?;
            }
        }
        OutputFormat::Text => {
            if opts.include_headers {
                println!("HTTP {}", response.status_code);
                for (name, value) in &response.headers {
                    println!("{name}: {value}");
                }
                println!();
            }

            output_response_body(opts, &response.body)?;
        }
    }

    if let Some(ref path) = opts.dump_headers {
        write_headers_file(opts, path, &response)?;
    }

    Ok(())
}

/// Render a structured JSON error object for agent consumption.
///
/// Schema: { code, message, cause? }
pub(crate) fn render_error_json(err: &anyhow::Error) -> String {
    let code = ExitCode::from(err).label();

    // Root message and optional immediate cause
    let message = err.to_string();
    let cause = err.chain().nth(1).map(|c| c.to_string());

    let mut obj = serde_json::json!({
        "code": code,
        "message": message,
    });

    if let Some(c) = cause {
        if let serde_json::Value::Object(ref mut map) = obj {
            map.insert("cause".into(), serde_json::Value::String(c));
        }
    }

    serde_json::to_string(&obj).unwrap_or_else(|_| {
        // As a last resort, emit a minimal JSON
        format!("{{\"code\":\"{}\",\"message\":\"error\"}}", code)
    })
}

/// Write raw bytes to the configured output destination.
///
/// Writes exact bytes with no trailing newline, matching curl-like semantics.
/// This preserves binary payloads and strict byte-stream consumers.
fn output_response_body(opts: &OutputOptions, body: &[u8]) -> Result<()> {
    if let Some(ref output_file) = opts.output_file {
        write_to_file(opts, output_file, body)?;
    } else {
        use std::io::Write;
        std::io::stdout()
            .write_all(body)
            .context("Failed to write response to stdout")?;
    }
    Ok(())
}

/// Write string content to the configured output destination.
///
/// Adds a trailing newline for stdout (suitable for formatted/JSON output).
/// File output writes the content as-is without a trailing newline.
fn write_output_to(opts: &OutputOptions, content: impl AsRef<str>) -> Result<()> {
    let content = content.as_ref();
    if let Some(ref output_file) = opts.output_file {
        write_to_file(opts, output_file, content.as_bytes())?;
    } else {
        println!("{content}");
    }
    Ok(())
}

/// Write bytes to a file, handling `-` as stdout and validating the path.
fn write_to_file(opts: &OutputOptions, output_file: &str, data: &[u8]) -> Result<()> {
    if output_file == "-" {
        use std::io::Write;
        std::io::stdout()
            .write_all(data)
            .context("Failed to write to stdout")?;
    } else {
        validate_path(output_file, true).context("Invalid output path")?;
        std::fs::write(output_file, data).context("Failed to write output file")?;
        if opts.log_enabled() {
            eprintln!("Saved to: {output_file}");
        }
    }
    Ok(())
}

/// Write response headers to a file (HTTP status line followed by headers, blank line terminator)
fn write_headers_file(opts: &OutputOptions, path: &str, response: &HttpResponse) -> Result<()> {
    let mut content = String::new();
    content.push_str(&format!("HTTP {}\n", response.status_code));
    for (name, value) in &response.headers {
        content.push_str(&format!("{}: {}\n", name, value));
    }
    content.push('\n');
    write_to_file(opts, path, content.as_bytes())
}
