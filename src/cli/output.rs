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
    pub write_meta: Option<String>,
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
        OutputFormat::Json | OutputFormat::Toon => {
            if let Ok(json_value) = serde_json::from_slice::<serde_json::Value>(&response.body) {
                let output = opts.output_format.serialize_pretty(&json_value)?;
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

/// Write response metadata (JSON) if requested.
pub(crate) fn write_meta_if_requested(
    opts: &OutputOptions,
    response: &HttpResponse,
    elapsed_ms: u128,
    bytes: usize,
    effective_url: &str,
) -> Result<()> {
    if let Some(ref path) = opts.write_meta {
        let obj = serde_json::json!({
            "status": response.status_code,
            "url": effective_url,
            "elapsed_ms": elapsed_ms,
            "bytes": bytes,
            "headers": response.headers,
        });
        let s = serde_json::to_string_pretty(&obj)?;
        write_to_file(opts, path, s.as_bytes())?;
    }
    Ok(())
}

/// Render a structured error object for agent consumption.
///
/// Schema: { code, message, cause? }
pub(crate) fn render_error_structured(err: &anyhow::Error, format: OutputFormat) -> String {
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

    format.serialize(&obj).unwrap_or_else(|_| {
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

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== render_error_structured stability ====================

    #[test]
    fn test_render_error_json_payment_rejected() {
        let err: anyhow::Error = crate::error::PrestoError::PaymentRejected {
            reason: "insufficient funds".into(),
            status_code: 402,
        }
        .into();
        let json_str = render_error_structured(&err, OutputFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed["code"], "E_PAYMENT");
        assert!(parsed["message"]
            .as_str()
            .unwrap()
            .contains("insufficient funds"));
    }

    #[test]
    fn test_render_error_json_missing_header() {
        let err: anyhow::Error =
            crate::error::PrestoError::MissingHeader("WWW-Authenticate".into()).into();
        let json_str = render_error_structured(&err, OutputFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed["code"], "E_PAYMENT");
        assert!(parsed["message"]
            .as_str()
            .unwrap()
            .contains("WWW-Authenticate"));
    }

    #[test]
    fn test_render_error_json_unsupported_payment_method() {
        let err: anyhow::Error =
            crate::error::PrestoError::UnsupportedPaymentMethod("bitcoin".into()).into();
        let json_str = render_error_structured(&err, OutputFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed["code"], "E_PAYMENT");
        assert!(parsed["message"].as_str().unwrap().contains("bitcoin"));
    }

    #[test]
    fn test_render_error_json_config_missing() {
        let err: anyhow::Error =
            crate::error::PrestoError::ConfigMissing("no wallet".into()).into();
        let json_str = render_error_structured(&err, OutputFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed["code"], "E_CONFIG");
    }

    #[test]
    fn test_render_error_json_http_error() {
        let err: anyhow::Error =
            crate::error::PrestoError::Http("500 Internal Server Error".into()).into();
        let json_str = render_error_structured(&err, OutputFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed["code"], "E_NETWORK");
    }

    #[test]
    fn test_render_error_json_has_cause() {
        let inner: anyhow::Error =
            crate::error::PrestoError::Http("connection refused".into()).into();
        let err = inner.context("failed to reach server");
        let json_str = render_error_structured(&err, OutputFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        // The outermost message is the context
        assert!(parsed["message"]
            .as_str()
            .unwrap()
            .contains("failed to reach server"));
        // The cause is the inner error
        assert!(parsed["cause"]
            .as_str()
            .unwrap()
            .contains("connection refused"));
    }

    #[test]
    fn test_render_error_json_no_cause() {
        let err: anyhow::Error = crate::error::PrestoError::InvalidUrl("bad scheme".into()).into();
        let json_str = render_error_structured(&err, OutputFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert!(parsed.get("cause").is_none());
    }

    #[test]
    fn test_render_error_json_schema_fields() {
        // Verify the JSON always has exactly "code" and "message" (and optionally "cause")
        let err: anyhow::Error = crate::error::PrestoError::UnknownNetwork("custom".into()).into();
        let json_str = render_error_structured(&err, OutputFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        let obj = parsed.as_object().unwrap();
        assert!(obj.contains_key("code"));
        assert!(obj.contains_key("message"));
        // No extra fields
        for key in obj.keys() {
            assert!(
                key == "code" || key == "message" || key == "cause",
                "unexpected field: {key}"
            );
        }
    }

    // ==================== render_error_structured TOON format ====================

    #[test]
    fn test_render_error_toon_contains_code_and_message() {
        let err: anyhow::Error = crate::error::PrestoError::Http("timeout".into()).into();
        let toon_str = render_error_structured(&err, OutputFormat::Toon);
        assert!(
            toon_str.contains("code"),
            "TOON output should contain 'code'"
        );
        assert!(
            toon_str.contains("timeout"),
            "TOON output should contain 'timeout'"
        );
        assert!(
            toon_str.contains("E_NETWORK"),
            "TOON output should contain 'E_NETWORK'"
        );

        // Round-trip: decode TOON back to serde_json::Value
        let parsed: serde_json::Value = toon_format::decode_default(&toon_str).unwrap();
        assert_eq!(parsed["code"], "E_NETWORK");
        assert!(parsed["message"].as_str().unwrap().contains("timeout"));
    }

    #[test]
    fn test_render_error_toon_with_cause() {
        let inner: anyhow::Error = crate::error::PrestoError::Http("refused".into()).into();
        let err = inner.context("server down");
        let toon_str = render_error_structured(&err, OutputFormat::Toon);
        assert!(
            toon_str.contains("server down"),
            "TOON output should contain context message"
        );
        assert!(
            toon_str.contains("refused"),
            "TOON output should contain cause message"
        );
    }

    #[test]
    fn test_render_error_toon_is_not_json() {
        let err: anyhow::Error = crate::error::PrestoError::Http("fail".into()).into();
        let toon_str = render_error_structured(&err, OutputFormat::Toon);
        assert!(
            !toon_str.starts_with('{'),
            "TOON output should not start with '{{' (not JSON)"
        );
    }
}
