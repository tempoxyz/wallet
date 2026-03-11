//! Terminal output helpers (hyperlinks, field formatting, truncation).

use std::sync::OnceLock;

use crate::network::NetworkId;

/// Strip control characters from a string to prevent terminal escape injection.
///
/// Removes all C0 control characters (0x00–0x1F) and DEL (0x7F) except for
/// tab and newline. This prevents:
/// - ANSI escape sequence injection (CSI, OSC, etc.)
/// - OSC 8 breakout via BEL (\x07)
/// - Cursor manipulation and line erasure
pub fn sanitize_for_terminal(s: &str) -> String {
    s.chars()
        .filter(|c| {
            // Keep printable characters and safe whitespace (tab, newline)
            !c.is_control() || matches!(*c, '\t' | '\n')
        })
        .collect()
}

/// Format text as a clickable hyperlink using the OSC 8 protocol.
/// Both text and url are sanitized to strip control characters.
pub fn hyperlink(text: &str, url: &str) -> String {
    let clean_text = sanitize_for_terminal(text);
    if supports_hyperlinks() {
        let clean_url = sanitize_for_terminal(url);
        format!("\x1b]8;;{}\x07{}\x1b]8;;\x07", clean_url, clean_text)
    } else {
        clean_text
    }
}

/// Format an address as a clickable hyperlink for the given network.
pub fn address_link(network: NetworkId, address: &str) -> String {
    let url = network.address_url(address);
    hyperlink(address, &url)
}

/// Check if the current terminal supports OSC 8 hyperlinks.
fn supports_hyperlinks() -> bool {
    static SUPPORTS: OnceLock<bool> = OnceLock::new();
    *SUPPORTS.get_or_init(detect_hyperlink_support)
}

fn detect_hyperlink_support() -> bool {
    use std::env;

    if env::var("FORCE_HYPERLINKS").is_ok_and(|v| v == "1") {
        return true;
    }
    if env::var("CI").is_ok() {
        return false;
    }
    if !std::io::IsTerminal::is_terminal(&std::io::stderr()) {
        return false;
    }

    const SUPPORTED_TERMINAL_VARS: &[&str] = &[
        "ITERM_SESSION_ID",
        "WT_SESSION",
        "WEZTERM_PANE",
        "GHOSTTY_RESOURCES_DIR",
        "KITTY_WINDOW_ID",
        "ALACRITTY_SOCKET",
        "KONSOLE_VERSION",
    ];

    if SUPPORTED_TERMINAL_VARS
        .iter()
        .any(|var| env::var(var).is_ok())
    {
        return true;
    }

    const SUPPORTED_TERM_PROGRAMS: &[&str] = &["vscode", "Hyper"];

    if let Ok(term_program) = env::var("TERM_PROGRAM") {
        if SUPPORTED_TERM_PROGRAMS.contains(&term_program.as_str()) {
            return true;
        }
    }

    if let Ok(vte_version) = env::var("VTE_VERSION") {
        if vte_version
            .parse::<u32>()
            .map(|v| v >= 5000)
            .unwrap_or(false)
        {
            return true;
        }
    }

    false
}

/// Truncate a display string to `max` characters, appending `…` if truncated.
pub fn truncate(s: &str, max: usize) -> String {
    let safe = sanitize_for_terminal(s);
    if safe.chars().count() <= max {
        safe
    } else {
        let truncated: String = safe.chars().take(max - 1).collect();
        format!("{truncated}…")
    }
}

/// Print a right-aligned label/value field to stdout with a custom label width.
pub fn print_field_w(width: usize, label: &str, value: &str) {
    let safe_value = sanitize_for_terminal(value);
    println!("{:>width$}: {safe_value}", label);
}

/// Print a right-aligned label/value field to stdout (14-char label width).
pub fn print_field(label: &str, value: &str) {
    print_field_w(14, label, value);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hyperlink_sanitizes_escape_sequences_in_text() {
        let malicious_text = "0xabc\x1b[31mPHISHING\x1b[0m";
        let url = "https://explorer.tempo.xyz/tx/0xabc";

        let result = hyperlink(malicious_text, url);

        assert!(
            !result.contains('\x1b'),
            "hyperlink() must strip escape sequences from text, got: {:?}",
            result
        );
    }

    #[test]
    fn test_hyperlink_sanitizes_bel_in_text() {
        let malicious_text = "0xabc\x07\x1b]8;;https://evil.com\x07click here\x1b]8;;\x07";
        let url = "https://explorer.tempo.xyz/tx/0xabc";

        let result = hyperlink(malicious_text, url);

        assert!(
            !result.contains('\x07') && !result.contains('\x1b'),
            "hyperlink() must strip BEL/ESC from text to prevent OSC 8 breakout, got: {:?}",
            result
        );
    }

    #[test]
    fn test_hyperlink_sanitizes_escape_sequences_in_url() {
        let malicious_url =
            "https://explorer.tempo.xyz/tx/0xabc\x07\x1b]8;;https://evil.com\x07fake";

        let clean_url = sanitize_for_terminal(malicious_url);
        assert!(
            !clean_url.contains('\x07') && !clean_url.contains('\x1b'),
            "sanitized URL must not contain BEL/ESC control characters: {:?}",
            clean_url
        );
    }

    #[test]
    fn test_hyperlink_strips_cursor_manipulation() {
        let malicious_text = "0xabc\x1b[A\x1b[2KPayment successful: 0.00 USDC";
        let url = "https://explorer.tempo.xyz/tx/0xabc";

        let result = hyperlink(malicious_text, url);
        assert!(!result.contains("\x1b[A"));
        assert!(!result.contains("\x1b[2K"));
    }

    #[test]
    fn test_hyperlink_plain_text_when_unsupported() {
        // When hyperlinks are not supported (typical in test/CI), hyperlink()
        // returns just the sanitized text.
        let result = hyperlink("View tx", "https://etherscan.io/tx/0x123");
        assert!(
            result.contains("View tx"),
            "output should contain the display text, got: {result:?}"
        );
    }

    #[test]
    fn truncate_short_unchanged() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_exact_length_unchanged() {
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn truncate_long_adds_ellipsis() {
        assert_eq!(truncate("hello world", 5), "hell…");
    }
}
