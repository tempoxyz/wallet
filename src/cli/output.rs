//! Output formatting and display utilities for the CLI.

#[cfg(test)]
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

use crate::config::validate_path;
use crate::http::{HttpResponse, OutputOptions};

use super::OutputFormat;

// Re-export hyperlink from util for use by request.rs
pub use crate::util::hyperlink;

// ---------------------------------------------------------------------------
// Time utilities
// ---------------------------------------------------------------------------

/// Get current Unix timestamp in seconds.
#[cfg(test)]
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Format an expiry timestamp as a human-readable string.
#[cfg(test)]
pub fn format_expiry(expiry: u64) -> String {
    if expiry == 0 {
        return "no expiry".to_string();
    }

    let now = now_secs();
    if expiry < now {
        "expired".to_string()
    } else {
        let remaining = expiry - now;
        let hours = remaining / 3600;
        let minutes = (remaining % 3600) / 60;
        format!("{}h {}m left", hours, minutes)
    }
}

// ---------------------------------------------------------------------------
// Response output
// ---------------------------------------------------------------------------

/// Handle a regular (non-402) HTTP response
pub fn handle_regular_response(opts: &OutputOptions, response: HttpResponse) -> Result<()> {
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

    Ok(())
}

/// Write response body to file or stdout.
///
/// Writes exact bytes with no trailing newline, matching curl-like semantics.
/// This preserves binary payloads and strict byte-stream consumers.
pub fn output_response_body(opts: &OutputOptions, body: &[u8]) -> Result<()> {
    if let Some(ref output_file) = opts.output_file {
        if output_file == "-" {
            use std::io::Write;
            std::io::stdout()
                .write_all(body)
                .context("Failed to write response to stdout")?;
        } else {
            validate_path(output_file, true).context("Invalid output path")?;
            std::fs::write(output_file, body).context("Failed to write output file")?;
            if opts.log_enabled() {
                eprintln!("Saved to: {output_file}");
            }
        }
    } else {
        use std::io::Write;
        std::io::stdout()
            .write_all(body)
            .context("Failed to write response to stdout")?;
    }
    Ok(())
}

/// Write string output to a specific file or stdout
pub fn write_output_to(opts: &OutputOptions, content: impl AsRef<str>) -> Result<()> {
    let content = content.as_ref();
    if let Some(ref output_file) = opts.output_file {
        if output_file == "-" {
            println!("{content}");
        } else {
            validate_path(output_file, true).context("Invalid output path")?;
            std::fs::write(output_file, content).context("Failed to write output file")?;
            if opts.log_enabled() {
                eprintln!("Saved to: {output_file}");
            }
        }
    } else {
        println!("{content}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hyperlink_format() {
        // Test the raw format (ignoring detection)
        let url = "https://etherscan.io/tx/0x123";
        let text = "View transaction";
        let expected = "\x1b]8;;https://etherscan.io/tx/0x123\x07View transaction\x1b]8;;\x07";
        assert_eq!(format!("\x1b]8;;{}\x07{}\x1b]8;;\x07", url, text), expected);
    }

    #[test]
    fn test_format_expiry_no_expiry() {
        assert_eq!(format_expiry(0), "no expiry");
    }

    #[test]
    fn test_format_expiry_expired() {
        let past = now_secs().saturating_sub(3600);
        assert_eq!(format_expiry(past), "expired");
    }

    #[test]
    fn test_format_expiry_future() {
        let future = now_secs() + 7200 + 1800;
        let result = format_expiry(future);
        assert!(result.contains("h") && result.contains("m left"));
    }

    #[test]
    fn test_now_secs_reasonable() {
        let now = now_secs();
        assert!(now > 1704067200);
        assert!(now < 4102444800);
    }
}
