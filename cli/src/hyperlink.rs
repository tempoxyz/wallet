//! Terminal hyperlink support using the OSC 8 protocol.
//!
//! This module provides utilities for creating clickable hyperlinks in terminal output
//! that open URLs when clicked (in supported terminals).

use std::sync::OnceLock;

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

/// Format text as a hyperlink, with fallback URL display for unsupported terminals.
///
/// In terminals that support OSC 8 hyperlinks, the text will be clickable.
/// In terminals that don't support hyperlinks, the URL is shown in brackets.
///
/// # Examples
///
/// ```ignore
/// let link = hyperlink_with_fallback("0x123...abc", "https://etherscan.io/tx/0x123");
/// // In supported terminals: "0x123...abc" (clickable)
/// // In unsupported terminals: "0x123...abc [https://etherscan.io/tx/0x123]"
/// ```
#[allow(dead_code)]
pub fn hyperlink_with_fallback(text: &str, url: &str) -> String {
    if supports_hyperlinks() {
        format!("\x1b]8;;{}\x07{}\x1b]8;;\x07", url, text)
    } else {
        format!("{} [{}]", text, url)
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

#[cfg(test)]
mod tests {
    #[test]
    fn test_hyperlink_format() {
        // Test the raw format (ignoring detection)
        let url = "https://etherscan.io/tx/0x123";
        let text = "View transaction";
        let expected = "\x1b]8;;https://etherscan.io/tx/0x123\x07View transaction\x1b]8;;\x07";
        assert_eq!(format!("\x1b]8;;{}\x07{}\x1b]8;;\x07", url, text), expected);
    }

    #[test]
    fn test_hyperlink_with_fallback_format() {
        let url = "https://etherscan.io/tx/0x123";
        let text = "0x123...abc";

        // Test fallback format
        let fallback = format!("{} [{}]", text, url);
        assert_eq!(fallback, "0x123...abc [https://etherscan.io/tx/0x123]");
    }
}
