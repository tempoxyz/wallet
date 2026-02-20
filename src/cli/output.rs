//! Output formatting, terminal hyperlinks, and display utilities for the CLI.

use std::sync::OnceLock;
#[cfg(test)]
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

use crate::network::ExplorerConfig;
use crate::{config::validate_path, http::HttpResponse};

use super::{Cli, OutputFormat, QueryArgs};

// ---------------------------------------------------------------------------
// Terminal hyperlink support (OSC 8)
// ---------------------------------------------------------------------------

/// Format text as a clickable hyperlink using the OSC 8 protocol.
///
/// In terminals that support OSC 8 hyperlinks (iTerm2, WezTerm, VSCode, Ghostty, etc.),
/// the text will be clickable and open the URL when clicked.
///
/// In terminals that don't support hyperlinks, the text is returned unchanged.
///
/// # Examples
///
/// ```ignore
/// let link = hyperlink("View transaction", "https://etherscan.io/tx/0x123");
/// // In supported terminals: "View transaction" is clickable
/// // In unsupported terminals: "View transaction"
/// ```
pub fn hyperlink(text: &str, url: &str) -> String {
    if supports_hyperlinks() {
        format!("\x1b]8;;{}\x07{}\x1b]8;;\x07", url, text)
    } else {
        text.to_string()
    }
}

/// Check if the current terminal supports OSC 8 hyperlinks.
///
/// This function caches its result for performance, only checking once per process.
///
/// Detection is based on:
/// - FORCE_HYPERLINKS=1 environment variable (force enable)
/// - CI environment variable (disable in CI)
/// - Known terminal identifiers (TERM_PROGRAM, WT_SESSION, etc.)
pub fn supports_hyperlinks() -> bool {
    static SUPPORTS: OnceLock<bool> = OnceLock::new();
    *SUPPORTS.get_or_init(detect_hyperlink_support)
}

/// Detect hyperlink support based on environment variables and terminal type.
fn detect_hyperlink_support() -> bool {
    use std::env;

    // Force enable via environment variable
    if env::var("FORCE_HYPERLINKS").is_ok_and(|v| v == "1") {
        return true;
    }

    // Disable in CI environments (output is typically not interactive)
    if env::var("CI").is_ok() {
        return false;
    }

    // Check if stderr is a terminal
    if !std::io::IsTerminal::is_terminal(&std::io::stderr()) {
        return false;
    }

    // Environment variables that indicate a terminal with OSC 8 support (presence check)
    const SUPPORTED_TERMINAL_VARS: &[&str] = &[
        "ITERM_SESSION_ID",      // iTerm2
        "WT_SESSION",            // Windows Terminal
        "WEZTERM_PANE",          // WezTerm
        "GHOSTTY_RESOURCES_DIR", // Ghostty
        "KITTY_WINDOW_ID",       // Kitty
        "ALACRITTY_SOCKET",      // Alacritty (supports OSC 8 since v0.11)
        "KONSOLE_VERSION",       // Konsole
    ];

    // Check presence-based env vars
    if SUPPORTED_TERMINAL_VARS
        .iter()
        .any(|var| env::var(var).is_ok())
    {
        return true;
    }

    // TERM_PROGRAM values that indicate OSC 8 support
    const SUPPORTED_TERM_PROGRAMS: &[&str] = &["vscode", "Hyper"];

    if let Ok(term_program) = env::var("TERM_PROGRAM") {
        if SUPPORTED_TERM_PROGRAMS.contains(&term_program.as_str()) {
            return true;
        }
    }

    // GNOME Terminal (VTE-based, version 0.50+)
    if let Ok(vte_version) = env::var("VTE_VERSION") {
        if vte_version
            .parse::<u32>()
            .map(|v| v >= 5000)
            .unwrap_or(false)
        {
            return true;
        }
    }

    // Default to false for unknown terminals
    false
}

// ---------------------------------------------------------------------------
// Address formatting
// ---------------------------------------------------------------------------

/// Format an address as a clickable hyperlink if explorer is available.
pub fn format_address_link(address: &str, explorer: Option<&ExplorerConfig>) -> String {
    if let Some(exp) = explorer {
        let url = exp.address_url(address);
        hyperlink(address, &url)
    } else {
        address.to_string()
    }
}

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
pub fn handle_regular_response(cli: &Cli, query: &QueryArgs, response: HttpResponse) -> Result<()> {
    match cli.output_format {
        OutputFormat::Json => {
            if let Ok(json_value) = serde_json::from_slice::<serde_json::Value>(&response.body) {
                let output = serde_json::to_string_pretty(&json_value)?;
                write_output_to(query.output.as_deref(), cli, output)?;
            } else {
                output_response_body(query.output.as_deref(), cli, &response.body)?;
            }
        }
        OutputFormat::Text => {
            if query.include_headers {
                println!("HTTP {}", response.status_code);
                for (name, value) in &response.headers {
                    println!("{name}: {value}");
                }
                println!();
            }

            output_response_body(query.output.as_deref(), cli, &response.body)?;
        }
    }

    Ok(())
}

/// Write response body to file or stdout.
///
/// Writes exact bytes with no trailing newline, matching curl-like semantics.
/// This preserves binary payloads and strict byte-stream consumers.
pub fn output_response_body(output_file: Option<&str>, cli: &Cli, body: &[u8]) -> Result<()> {
    if let Some(output_file) = output_file {
        if output_file == "-" {
            use std::io::Write;
            std::io::stdout()
                .write_all(body)
                .context("Failed to write response to stdout")?;
        } else {
            validate_path(output_file, true).context("Invalid output path")?;
            std::fs::write(output_file, body).context("Failed to write output file")?;
            if cli.is_verbose() && cli.should_show_output() {
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
pub fn write_output_to(
    output_file: Option<&str>,
    cli: &Cli,
    content: impl AsRef<str>,
) -> Result<()> {
    let content = content.as_ref();
    if let Some(output_file) = output_file {
        if output_file == "-" {
            println!("{content}");
        } else {
            validate_path(output_file, true).context("Invalid output path")?;
            std::fs::write(output_file, content).context("Failed to write output file")?;
            if cli.is_verbose() && cli.should_show_output() {
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
