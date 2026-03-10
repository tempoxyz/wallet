//! Response rendering, receipt display, and file output.

use std::fmt::Write as _;
use std::io::Write;
use std::path::{Component, Path};

use anyhow::{Context as _, Result};

use crate::args::QueryArgs;
use crate::http::{format_http_error, print_headers, HttpResponse};
use tempo_common::cli::output::{format_structured_pretty_json, OutputFormat};
use tempo_common::cli::terminal::hyperlink;
use tempo_common::cli::Verbosity;
use tempo_common::error::{InputError, NetworkError};
use tempo_common::network::NetworkId;

/// Output/display options extracted from CLI arguments.
///
/// Used by response formatting functions; kept separate from
/// `HttpClient` to avoid coupling HTTP/payment layers to
/// presentation concerns.
#[derive(Clone, Debug)]
pub(crate) struct OutputOptions {
    pub(crate) output_format: OutputFormat,
    pub(crate) include_headers: bool,
    pub(crate) output_file: Option<String>,
    pub(crate) verbosity: tempo_common::cli::Verbosity,
    pub(crate) dump_headers: Option<String>,
    pub(crate) write_meta: Option<String>,
}

impl OutputOptions {
    /// Whether agent-level log messages should be printed (`-v`).
    pub(crate) fn log_enabled(&self) -> bool {
        self.verbosity.log_enabled()
    }

    /// Whether payment summaries should be printed (always, unless `--quiet`).
    pub(crate) fn payment_log_enabled(&self) -> bool {
        self.verbosity.show_output
    }
}

/// Build `OutputOptions` from CLI arguments + config.
///
/// Accepts the already-parsed URL to avoid redundant parsing.
pub(crate) fn build_output_options(
    output_format: OutputFormat,
    verbosity: Verbosity,
    query: &QueryArgs,
    parsed_url: &url::Url,
) -> OutputOptions {
    OutputOptions {
        output_format,
        // -I (HEAD) implies showing headers, even if -i wasn't explicitly set
        include_headers: query.include_headers || query.head,
        output_file: if query.output.is_none() && query.remote_name {
            // Derive a filename from the URL's last path segment; fallback to 'index.html'
            let seg = parsed_url
                .path_segments()
                .and_then(|mut s| s.next_back())
                .filter(|v| !v.is_empty())
                .unwrap_or("index.html");
            Some(seg.to_string())
        } else {
            query.output.clone()
        },
        verbosity,
        dump_headers: query.dump_header.clone(),
        write_meta: query.write_meta.clone(),
    }
}

/// Handle a final response: render output, optionally save the payment receipt, and fail on HTTP errors.
pub(crate) fn handle_response(
    output_opts: &OutputOptions,
    response: HttpResponse,
    save_receipt_path: Option<&str>,
) -> Result<()> {
    let status = response.status_code;

    // Capture receipt header before consuming response for output
    let receipt_hdr =
        save_receipt_path.and_then(|_| response.header("payment-receipt").map(|s| s.to_string()));

    render_response(output_opts, response)?;

    // Optionally save receipt JSON if present
    if let (Some(path), Some(h)) = (save_receipt_path, receipt_hdr.as_ref()) {
        match mpp::parse_receipt(h) {
            Ok(receipt) => {
                let s = serde_json::to_string_pretty(&receipt)?;
                std::fs::write(path, s)?;
            }
            Err(e) => {
                tracing::warn!("failed to parse receipt for --save-receipt: {e}");
            }
        }
    }

    if status >= 400 {
        anyhow::bail!(NetworkError::Http(format_http_error(status)));
    }

    Ok(())
}

/// Render and output an HTTP response (headers, body, dump-headers).
///
/// Note: `include_headers` only applies to `Text` format; structured formats
/// (JSON/TOON) omit the status line and headers from stdout to keep output
/// machine-parseable. Use `--dump-header` to capture headers separately.
fn render_response(opts: &OutputOptions, response: HttpResponse) -> Result<()> {
    match opts.output_format {
        OutputFormat::Json | OutputFormat::Toon => {
            if let Ok(json_value) = serde_json::from_slice::<serde_json::Value>(&response.body) {
                let output = format_structured_pretty_json(opts.output_format, &json_value)?;
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
        write_headers_file(
            path,
            response.status_code,
            &response.headers,
            opts.log_enabled(),
        )?;
    }

    Ok(())
}

/// Write raw response bytes to stdout or file (no trailing newline).
fn write_body(opts: &OutputOptions, body: &[u8]) -> Result<()> {
    let dest = opts.output_file.as_deref().unwrap_or("-");
    write_to_file(dest, body, opts.log_enabled())
}

/// Write response metadata (JSON) if requested via `--write-meta`.
pub(crate) fn write_meta_if_requested(
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
    path: &str,
    status_code: u16,
    headers: &[(String, String)],
    verbose: bool,
) -> Result<()> {
    let mut content = String::new();
    writeln!(content, "HTTP {status_code}").unwrap();
    for (name, value) in headers {
        writeln!(content, "{name}: {value}").unwrap();
    }
    content.push('\n');
    write_to_file(path, content.as_bytes(), verbose)
}

/// Display receipt information from response with optional clickable explorer links.
pub(crate) fn display_receipt(
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
    let receipt_header = response.header("payment-receipt");
    let parsed_receipt = receipt_header.and_then(|h| mpp::parse_receipt(h).ok());

    let link = receipt_header.and_then(|h| {
        let tx_ref = mpp::protocol::core::extract_tx_hash(h)
            .or_else(|| parsed_receipt.as_ref().map(|r| r.reference.clone()));
        tx_ref.map(|tx| {
            let url = network.tx_url(&tx);
            hyperlink(&tx, &url)
        })
    });

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
// File output helpers
// ---------------------------------------------------------------------------

/// Write bytes to a file, handling `-` as stdout and validating the path.
///
/// Absolute paths are intentionally allowed (matching curl behaviour).
/// `..` traversal components are rejected lexically. For relative paths,
/// symlinks in the parent directory are resolved to prevent escaping the
/// working directory. Absolute paths bypass the symlink check (also
/// matching curl behaviour — the caller explicitly chose the destination).
fn write_to_file(output_file: &str, data: &[u8], verbose: bool) -> Result<()> {
    if output_file == "-" {
        std::io::stdout()
            .write_all(data)
            .context("Failed to write to stdout")?;
    } else {
        let path = Path::new(output_file);
        if path.components().any(|c| matches!(c, Component::ParentDir)) {
            anyhow::bail!(InputError::InvalidOutputPath(
                "path traversal (..) not allowed".to_string()
            ));
        }
        // Resolve symlinks in the parent to prevent escaping the intended directory
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            if let Ok(canonical) = parent.canonicalize() {
                let cwd = std::env::current_dir().unwrap_or_default();
                if !path.is_absolute() && !canonical.starts_with(&cwd) {
                    anyhow::bail!(InputError::InvalidOutputPath(
                        "resolved path escapes working directory".to_string()
                    ));
                }
            }
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
            verbosity: tempo_common::cli::Verbosity {
                level: 0,
                show_output,
            },
            dump_headers: None,
            write_meta: None,
        }
    }

    // ---------------------------------------------------------------------------
    // handle_response
    // ---------------------------------------------------------------------------

    #[test]
    fn test_handle_response_success_status() {
        let opts = test_opts(false);
        let resp = HttpResponse::for_test(200, b"ok");
        assert!(handle_response(&opts, resp, None).is_ok());
    }

    #[test]
    fn test_handle_response_4xx_fails() {
        let opts = test_opts(false);
        let resp = HttpResponse::for_test(404, b"not found");
        let err = handle_response(&opts, resp, None).unwrap_err();
        assert!(err.to_string().contains("404"));
    }

    #[test]
    fn test_handle_response_5xx_fails() {
        let opts = test_opts(false);
        let resp = HttpResponse::for_test(500, b"internal error");
        let err = handle_response(&opts, resp, None).unwrap_err();
        assert!(err.to_string().contains("Internal Server Error"));
    }

    // ---------------------------------------------------------------------------
    // display_receipt
    // ---------------------------------------------------------------------------

    #[test]
    fn test_display_receipt_quiet_mode_suppresses_output() {
        let opts = test_opts(false);
        let resp = HttpResponse::for_test(200, b"ok");
        // Should not panic even with missing receipt header
        display_receipt(&opts, &resp, NetworkId::default(), "10000");
    }

    #[test]
    fn test_display_receipt_missing_header_shows_amount_only() {
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

    // ---------------------------------------------------------------------------
    // build_output_options
    // ---------------------------------------------------------------------------

    use clap::Parser;
    use url::Url;

    use crate::args::Cli;

    /// Parse a CLI invocation into both `Cli` and `QueryArgs` for testing.
    fn parse(args: &[&str]) -> (Cli, QueryArgs) {
        let all: Vec<&str> = std::iter::once("tempo-request")
            .chain(args.iter().copied())
            .collect();
        let cli = Cli::try_parse_from(all).unwrap();
        let query = cli.query;
        let cli = Cli::try_parse_from(
            std::iter::once("tempo-request")
                .chain(args.iter().copied())
                .collect::<Vec<&str>>(),
        )
        .unwrap();
        (cli, query)
    }

    #[test]
    fn remote_name_derives_filename_from_url() {
        let (c, q) = parse(&["-O", "https://example.com/path/file.txt"]);
        let url = Url::parse(&q.url).unwrap();

        let opts = build_output_options(
            c.global.resolve_output_format(),
            c.global.verbosity(),
            &q,
            &url,
        );
        assert_eq!(opts.output_file.as_deref(), Some("file.txt"));
    }

    #[test]
    fn remote_name_falls_back_to_index_html() {
        let (c, q) = parse(&["-O", "https://example.com/"]);
        let url = Url::parse(&q.url).unwrap();

        let opts = build_output_options(
            c.global.resolve_output_format(),
            c.global.verbosity(),
            &q,
            &url,
        );
        assert_eq!(opts.output_file.as_deref(), Some("index.html"));
    }

    #[test]
    fn head_implies_include_headers() {
        let (c, q) = parse(&["-I", "https://example.com"]);
        let url = Url::parse(&q.url).unwrap();

        let opts = build_output_options(
            c.global.resolve_output_format(),
            c.global.verbosity(),
            &q,
            &url,
        );
        assert!(opts.include_headers);
    }

    #[test]
    fn explicit_output_file_overrides_remote_name() {
        let (c, q) = parse(&["-o", "custom.txt", "https://example.com/path/file.txt"]);
        let url = Url::parse(&q.url).unwrap();

        let opts = build_output_options(
            c.global.resolve_output_format(),
            c.global.verbosity(),
            &q,
            &url,
        );
        assert_eq!(opts.output_file.as_deref(), Some("custom.txt"));
    }

    #[test]
    fn no_output_flags_means_no_file() {
        let (c, q) = parse(&["https://example.com/path/file.txt"]);
        let url = Url::parse(&q.url).unwrap();

        let opts = build_output_options(
            c.global.resolve_output_format(),
            c.global.verbosity(),
            &q,
            &url,
        );
        assert!(opts.output_file.is_none());
        assert!(!opts.include_headers);
        assert_eq!(opts.output_format, OutputFormat::Text);
    }

    #[test]
    fn json_output_flag() {
        let (c, q) = parse(&["-j", "https://example.com"]);
        let url = Url::parse(&q.url).unwrap();

        let opts = build_output_options(
            c.global.resolve_output_format(),
            c.global.verbosity(),
            &q,
            &url,
        );
        assert_eq!(opts.output_format, OutputFormat::Json);
    }
}
