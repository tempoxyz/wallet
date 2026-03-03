//! Terminal utilities: sanitization, hyperlink support, and safe redaction.

use std::sync::OnceLock;

use crate::analytics;

/// Strip control characters from a string to prevent terminal escape injection.
///
/// Removes all C0 control characters (0x00–0x1F) and DEL (0x7F) except for
/// tab and newline. This prevents:
/// - ANSI escape sequence injection (CSI, OSC, etc.)
/// - OSC 8 breakout via BEL (\x07)
/// - Cursor manipulation and line erasure
pub(crate) fn sanitize_for_terminal(s: &str) -> String {
    s.chars()
        .filter(|c| {
            // Keep printable characters and safe whitespace (tab, newline)
            !c.is_control() || matches!(*c, '\t' | '\n')
        })
        .collect()
}

/// Sensitive header names whose values must be redacted in logs and diagnostics.
const SENSITIVE_HEADERS: &[&str] = &[
    "authorization",
    "proxy-authorization",
    "cookie",
    "set-cookie",
    "x-api-key",
];

/// Redact a header value for safe logging.
///
/// For sensitive headers (Authorization, Cookie, etc.) the credential portion
/// is replaced with `[REDACTED]`. For `Authorization` / `Proxy-Authorization`
/// the scheme (e.g. `Bearer`, `Basic`) is preserved so the log remains useful.
pub(crate) fn redact_header_value(name: &str, value: &str) -> String {
    let lower = name.to_lowercase();
    if !SENSITIVE_HEADERS.contains(&lower.as_str()) {
        return value.to_string();
    }

    if lower == "authorization" || lower == "proxy-authorization" {
        if let Some((scheme, _)) = value.split_once(' ') {
            return format!("{scheme} [REDACTED]");
        }
    }

    "[REDACTED]".to_string()
}

/// Strip query parameters and fragments from a URL for safe logging.
/// Delegates to analytics::sanitize_url so both analytics and logs match.
pub(crate) fn redact_url(raw: &str) -> String {
    analytics::sanitize_url(raw)
}

/// Format text as a clickable hyperlink using the OSC 8 protocol.
/// Both text and url are sanitized to strip control characters.
pub(crate) fn hyperlink(text: &str, url: &str) -> String {
    let clean_text = sanitize_for_terminal(text);
    if supports_hyperlinks() {
        let clean_url = sanitize_for_terminal(url);
        format!("\x1b]8;;{}\x07{}\x1b]8;;\x07", clean_url, clean_text)
    } else {
        clean_text
    }
}

/// Check if the current terminal supports OSC 8 hyperlinks.
pub(crate) fn supports_hyperlinks() -> bool {
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
    fn test_hyperlink_format() {
        let url = "https://etherscan.io/tx/0x123";
        let text = "View transaction";
        let expected = "\x1b]8;;https://etherscan.io/tx/0x123\x07View transaction\x1b]8;;\x07";
        assert_eq!(format!("\x1b]8;;{}\x07{}\x1b]8;;\x07", url, text), expected);
    }

    #[test]
    fn test_redact_bearer_token() {
        assert_eq!(
            redact_header_value("Authorization", "Bearer sk_live_abc123"),
            "Bearer [REDACTED]"
        );
    }

    #[test]
    fn test_redact_basic_auth() {
        assert_eq!(
            redact_header_value("authorization", "Basic dXNlcjpwYXNz"),
            "Basic [REDACTED]"
        );
    }

    #[test]
    fn test_redact_proxy_authorization() {
        assert_eq!(
            redact_header_value("Proxy-Authorization", "Bearer proxy_token"),
            "Bearer [REDACTED]"
        );
    }

    #[test]
    fn test_redact_cookie() {
        assert_eq!(
            redact_header_value("cookie", "session=abc123; token=xyz"),
            "[REDACTED]"
        );
    }

    #[test]
    fn test_redact_set_cookie() {
        assert_eq!(
            redact_header_value("Set-Cookie", "sid=secret; Path=/; HttpOnly"),
            "[REDACTED]"
        );
    }

    #[test]
    fn test_redact_x_api_key() {
        assert_eq!(
            redact_header_value("X-Api-Key", "[REDACTED:sk-secret]"),
            "[REDACTED]"
        );
    }

    #[test]
    fn test_redact_auth_no_scheme() {
        assert_eq!(
            redact_header_value("Authorization", "tokenonly"),
            "[REDACTED]"
        );
    }

    #[test]
    fn test_redact_safe_header_unchanged() {
        assert_eq!(
            redact_header_value("Content-Type", "application/json"),
            "application/json"
        );
    }

    #[test]
    fn test_redact_accept_unchanged() {
        assert_eq!(redact_header_value("accept", "text/html"), "text/html");
    }

    #[test]
    fn test_redact_url_strips_secrets() {
        assert_eq!(
            redact_url("https://api.example.com/v1?api_key=secret"),
            "https://api.example.com/v1"
        );
    }

    #[test]
    fn test_redact_url_preserves_path() {
        assert_eq!(
            redact_url("https://api.example.com/v1/chat"),
            "https://api.example.com/v1/chat"
        );
    }
}
