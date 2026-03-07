//! Payment receipt display and response finalization.

use std::fmt::Write as _;
use std::io::Write;
use std::path::{Component, Path};

use anyhow::{Context as _, Result};

use crate::cli::output::{OutputFormat, OutputOptions};
use crate::error::TempoWalletError;
use crate::http::HttpResponse;
use crate::network::NetworkId;
use crate::util::hyperlink;

/// Finalize a regular response: display output and fail on HTTP errors.
pub(super) fn finalize_response(output_opts: &OutputOptions, response: HttpResponse) -> Result<()> {
    let status = response.status_code;
    handle_response(output_opts, response)?;
    if status >= 400 {
        anyhow::bail!(TempoWalletError::Http(format!(
            "{} {}",
            status,
            crate::http::http_status_text(status)
        )));
    }
    Ok(())
}

/// Render and output an HTTP response (headers, body, dump-headers).
///
/// Note: `include_headers` only applies to `Text` format; structured formats
/// (JSON/TOON) omit the status line and headers from stdout to keep output
/// machine-parseable. Use `--dump-header` to capture headers separately.
fn handle_response(opts: &OutputOptions, response: HttpResponse) -> Result<()> {
    match opts.output_format {
        OutputFormat::Json | OutputFormat::Toon => {
            if let Ok(json_value) = serde_json::from_slice::<serde_json::Value>(&response.body) {
                // Pretty-print JSON responses; TOON is always compact.
                let output = match opts.output_format {
                    OutputFormat::Json => serde_json::to_string_pretty(&json_value)?,
                    _ => opts.output_format.serialize(&json_value)?,
                };
                if let Some(ref output_file) = opts.output_file {
                    write_to_file(output_file, output.as_bytes(), opts.log_enabled())?;
                } else {
                    println!("{output}");
                }
            } else {
                write_body(opts, &response.body)?;
            }
        }
        OutputFormat::Text => {
            if opts.include_headers {
                print_headers(response.status_code, &response.headers);
            }

            write_body(opts, &response.body)?;
        }
    }

    if let Some(ref path) = opts.dump_headers {
        write_headers_file(opts, path, response.status_code, &response.headers)?;
    }

    Ok(())
}

/// Write raw response bytes to stdout or file (no trailing newline).
fn write_body(opts: &OutputOptions, body: &[u8]) -> Result<()> {
    let dest = opts.output_file.as_deref().unwrap_or("-");
    write_to_file(dest, body, opts.log_enabled())
}

/// Write response metadata (JSON) if requested via `--write-meta`.
pub(super) fn write_meta_if_requested(
    opts: &OutputOptions,
    status_code: u16,
    headers: &[(String, String)],
    elapsed_ms: u128,
    bytes: usize,
    effective_url: &str,
) -> Result<()> {
    if let Some(ref path) = opts.write_meta {
        let hdr_obj: serde_json::Value = headers
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
            .collect::<serde_json::Map<_, _>>()
            .into();
        let obj = serde_json::json!({
            "status": status_code,
            "url": effective_url,
            "elapsed_ms": elapsed_ms,
            "bytes": bytes,
            "headers": hdr_obj,
        });
        let s = serde_json::to_string_pretty(&obj)?;
        write_to_file(path, s.as_bytes(), opts.log_enabled())?;
    }
    Ok(())
}

/// Write response headers to a file (HTTP status line + headers + blank line).
fn write_headers_file(
    opts: &OutputOptions,
    path: &str,
    status_code: u16,
    headers: &[(String, String)],
) -> Result<()> {
    let mut content = String::new();
    writeln!(content, "HTTP {status_code}").unwrap();
    for (name, value) in headers {
        writeln!(content, "{name}: {value}").unwrap();
    }
    content.push('\n');
    write_to_file(path, content.as_bytes(), opts.log_enabled())
}

/// Display receipt information from response with optional clickable explorer links.
pub(super) fn display_receipt(
    output_opts: &OutputOptions,
    response: &HttpResponse,
    network: NetworkId,
    amount_display: &str,
) {
    // Always show payment summary when money moved (unless --quiet)
    if !output_opts.payment_log_enabled() {
        return;
    }

    // Try to extract a transaction reference/link if the server provided a receipt header
    let mut link: Option<String> = None;
    let mut parsed_receipt = None;
    if let Some(receipt_header) = response.header("payment-receipt") {
        parsed_receipt = mpp::parse_receipt(receipt_header).ok();
        let tx_ref = mpp::protocol::core::extract_tx_hash(receipt_header)
            .or_else(|| parsed_receipt.as_ref().map(|r| r.reference.clone()));

        if let Some(tx) = tx_ref {
            let url = network.tx_url(&tx);
            link = Some(hyperlink(&tx, &url));
        }
    }

    if let Some(l) = link {
        eprintln!("Paid {amount_display} · {l}");
    } else {
        eprintln!("Paid {amount_display}");
    }

    // Extended receipt details at -v
    if output_opts.log_enabled() {
        if let Some(receipt) = parsed_receipt {
            eprintln!("  Status: {}", receipt.status);
            eprintln!("  Method: {}", receipt.method);
            eprintln!("  Timestamp: {}", receipt.timestamp);
        }
    }
}

// ---------------------------------------------------------------------------
// Shared display helpers
// ---------------------------------------------------------------------------

/// Print HTTP status line and headers to stdout.
pub(super) fn print_headers(status: u16, headers: &[(String, String)]) {
    println!("HTTP {status}");
    for (name, value) in headers {
        println!("{name}: {value}");
    }
    println!();
}

// ---------------------------------------------------------------------------
// File output helpers
// ---------------------------------------------------------------------------

/// Write bytes to a file, handling `-` as stdout and validating the path.
///
/// Absolute paths are intentionally allowed (matching curl behaviour);
/// only `..` traversal components are rejected.
fn write_to_file(output_file: &str, data: &[u8], verbose: bool) -> Result<()> {
    if output_file == "-" {
        std::io::stdout()
            .write_all(data)
            .context("Failed to write to stdout")?;
    } else {
        let path = Path::new(output_file);
        if path.components().any(|c| matches!(c, Component::ParentDir)) {
            anyhow::bail!(TempoWalletError::InvalidConfig(
                "invalid output path: path traversal (..) not allowed".to_string()
            ));
        }
        std::fs::write(output_file, data).context("Failed to write output file")?;
        if verbose {
            eprintln!("Saved to: {output_file}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_opts(show_output: bool) -> OutputOptions {
        OutputOptions {
            output_format: OutputFormat::Text,
            include_headers: false,
            output_file: None,
            verbosity: crate::util::Verbosity {
                level: 0,
                show_output,
            },
            dump_headers: None,
            write_meta: None,
        }
    }

    // ==================== finalize_response ====================

    #[test]
    fn test_finalize_response_success_status() {
        let opts = test_opts(false);
        let resp = HttpResponse::for_test(200, b"ok");
        assert!(finalize_response(&opts, resp).is_ok());
    }

    #[test]
    fn test_finalize_response_4xx_fails() {
        let opts = test_opts(false);
        let resp = HttpResponse::for_test(404, b"not found");
        let err = finalize_response(&opts, resp).unwrap_err();
        assert!(err.to_string().contains("404"));
    }

    #[test]
    fn test_finalize_response_5xx_fails() {
        let opts = test_opts(false);
        let resp = HttpResponse::for_test(500, b"internal error");
        let err = finalize_response(&opts, resp).unwrap_err();
        assert!(err.to_string().contains("Internal Server Error"));
    }

    // ==================== display_receipt (silent mode) ====================

    #[test]
    fn test_display_receipt_silent_mode_no_panic() {
        let opts = test_opts(false);
        let resp = HttpResponse::for_test(200, b"ok");
        // Should not panic even with missing receipt header
        display_receipt(&opts, &resp, NetworkId::default(), "10000");
    }

    #[test]
    fn test_display_receipt_no_receipt_header_no_panic() {
        let opts = test_opts(true);
        let resp = HttpResponse::for_test(200, b"ok");
        // Should print amount without link, no panic
        display_receipt(&opts, &resp, NetworkId::default(), "10000");
    }

    #[test]
    fn test_display_receipt_malformed_receipt_header_no_panic() {
        let opts = test_opts(true);
        let mut resp = HttpResponse::for_test(200, b"ok");
        resp.headers.push((
            "payment-receipt".to_string(),
            "garbage-not-a-receipt".to_string(),
        ));
        // Should not panic on malformed receipt
        display_receipt(&opts, &resp, NetworkId::default(), "10000");
    }

    #[test]
    fn test_display_receipt_non_numeric_amount_no_panic() {
        let opts = test_opts(true);
        let resp = HttpResponse::for_test(200, b"ok");
        // Non-numeric amount should fall back to raw display
        display_receipt(&opts, &resp, NetworkId::default(), "not-a-number");
    }
}
