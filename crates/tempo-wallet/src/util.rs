//! Shared utility helpers.

use std::path::PathBuf;
use std::sync::OnceLock;

use crate::network::NetworkId;

/// Verbosity configuration shared across HTTP and CLI layers.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Verbosity {
    pub(crate) level: u8,
    pub(crate) show_output: bool,
}

impl Verbosity {
    /// Whether agent-level log messages should be printed (`-v`).
    pub(crate) fn log_enabled(&self) -> bool {
        self.level >= 1 && self.show_output
    }

    /// Whether debug-level log messages should be printed (`-vv`).
    pub(crate) fn debug_enabled(&self) -> bool {
        self.level >= 2 && self.show_output
    }
}

/// Get the tempo-wallet data directory (platform-specific).
///
/// - macOS: `~/Library/Application Support/tempo/wallet/`
/// - Linux: `~/.local/share/tempo/wallet/`
pub(crate) fn data_dir() -> Result<PathBuf, crate::error::TempoWalletError> {
    dirs::data_dir()
        .ok_or(crate::error::TempoWalletError::NoConfigDir)
        .map(|d| d.join("tempo").join("wallet"))
}

/// Format atomic token units as a human-readable string with trimmed trailing zeros.
pub(crate) fn format_token_amount(atomic: u128, network: NetworkId) -> String {
    let t = network.token();
    let formatted =
        alloy::primitives::utils::format_units(atomic, t.decimals).expect("decimals <= 77");
    if let Some(stripped) = formatted.strip_suffix(&format!(".{}", "0".repeat(t.decimals as usize)))
    {
        format!("{stripped} {}", t.symbol)
    } else {
        let trimmed = formatted.trim_end_matches('0');
        format!("{trimmed} {}", t.symbol)
    }
}

/// Strip control characters from a string to prevent terminal escape injection.
///
/// Removes all C0 control characters (0x00–0x1F) and DEL (0x7F) except for
/// tab and newline. This prevents:
/// - ANSI escape sequence injection (CSI, OSC, etc.)
/// - OSC 8 breakout via BEL (\x07)
/// - Cursor manipulation and line erasure
fn sanitize_for_terminal(s: &str) -> String {
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
///
/// Query strings often contain secrets (`?api_key=...`, `?token=...`), so we
/// only keep the scheme + host + path.
pub(crate) fn redact_url(raw: &str) -> String {
    match url::Url::parse(raw) {
        Ok(mut parsed) => {
            parsed.set_query(None);
            parsed.set_fragment(None);
            parsed.to_string()
        }
        Err(_) => raw.to_string(),
    }
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

/// Format an address as a clickable hyperlink for the given network.
pub(crate) fn address_link(network: NetworkId, address: &str) -> String {
    let url = network.address_url(address);
    hyperlink(address, &url)
}

/// Check if the current terminal supports OSC 8 hyperlinks.
fn supports_hyperlinks() -> bool {
    static SUPPORTS: OnceLock<bool> = OnceLock::new();
    *SUPPORTS.get_or_init(detect_hyperlink_support)
}

/// Current UTC time as an ISO-8601 string (e.g. `2024-01-15T12:00:00Z`).
pub(crate) fn now_utc() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format_utc_timestamp(now)
}

/// Format a Unix timestamp as an ISO-8601 UTC string (e.g. `2024-01-15T12:00:00Z`).
pub(crate) fn format_utc_timestamp(timestamp: u64) -> String {
    let secs = i64::try_from(timestamp).unwrap_or(i64::MAX);
    let dt =
        time::OffsetDateTime::from_unix_timestamp(secs).unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        dt.year(),
        dt.month() as u8,
        dt.day(),
        dt.hour(),
        dt.minute(),
        dt.second()
    )
}

/// Format seconds as a human-readable duration (e.g., "1h 30m", "2m 5s").
pub(crate) fn format_duration(secs: u64) -> String {
    if secs >= 86400 {
        let d = secs / 86400;
        let h = (secs % 86400) / 3600;
        if h == 0 {
            format!("{d}d")
        } else {
            format!("{d}d {h}h")
        }
    } else if secs >= 3600 {
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        if m == 0 {
            format!("{h}h")
        } else {
            format!("{h}h {m}m")
        }
    } else if secs >= 60 {
        let m = secs / 60;
        let s = secs % 60;
        if s == 0 {
            format!("{m}m")
        } else {
            format!("{m}m {s}s")
        }
    } else {
        format!("{secs}s")
    }
}

/// Format a Unix timestamp as a human-readable relative time (e.g., "5m ago", "2h ago", "3d ago").
pub(crate) fn format_relative_time(ts: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if ts == 0 || ts > now {
        return "just now".to_string();
    }
    let ago = now - ts;
    if ago < 60 {
        format!("{ago}s ago")
    } else if ago < 3600 {
        format!("{}m ago", ago / 60)
    } else if ago < 86400 {
        format!("{}h ago", ago / 3600)
    } else {
        format!("{}d ago", ago / 86400)
    }
}

/// Truncate an error message to avoid leaking sensitive server responses.
pub(crate) fn sanitize_error(err: &str) -> String {
    const MAX_LEN: usize = 200;
    if err.len() <= MAX_LEN {
        err.to_string()
    } else {
        format!("{}…", &err[..MAX_LEN])
    }
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

/// Prompt the user for confirmation. Returns `true` if confirmed.
///
/// In non-interactive mode (piped stdin), returns an error suggesting `--yes`.
/// When `yes` is `true`, skips the prompt and returns `true` immediately.
pub(crate) fn confirm(prompt: &str, yes: bool) -> anyhow::Result<bool> {
    if yes {
        return Ok(true);
    }

    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal() {
        anyhow::bail!("Use --yes for non-interactive mode");
    }

    use std::io::{self, Write};
    print!("{prompt} [y/N] ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().eq_ignore_ascii_case("y"))
}

/// Truncate a display string to `max` characters, appending `…` if truncated.
pub(crate) fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max - 1).collect();
        format!("{truncated}…")
    }
}

/// Print a right-aligned label/value field to stdout with a custom label width.
pub(crate) fn print_field_w(width: usize, label: &str, value: &str) {
    println!("{:>width$}: {value}", label);
}

/// Print a right-aligned label/value field to stdout (14-char label width).
pub(crate) fn print_field(label: &str, value: &str) {
    print_field_w(14, label, value);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_token_amount() {
        assert_eq!(format_token_amount(1_000_000, NetworkId::Tempo), "1 USDC");
        assert_eq!(format_token_amount(1_500_000, NetworkId::Tempo), "1.5 USDC");
        assert_eq!(format_token_amount(123, NetworkId::Tempo), "0.000123 USDC");
        assert_eq!(format_token_amount(0, NetworkId::Tempo), "0 USDC");
    }

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

    #[test]
    fn sanitize_error_short_unchanged() {
        let short = "connection refused";
        assert_eq!(sanitize_error(short), short);
    }

    #[test]
    fn sanitize_error_exactly_200_unchanged() {
        let msg = "x".repeat(200);
        assert_eq!(sanitize_error(&msg), msg);
    }

    #[test]
    fn sanitize_error_truncates_long_message() {
        let msg = "x".repeat(300);
        let result = sanitize_error(&msg);
        assert_eq!(result.len(), 200 + "…".len());
        assert!(result.ends_with('…'));
        assert!(result.starts_with("xxx"));
    }

    #[test]
    fn sanitize_error_prevents_secret_leakage_in_long_body() {
        let msg = format!("server error: {}secret_api_key_12345", "a]".repeat(100));
        let result = sanitize_error(&msg);
        assert!(!result.contains("secret_api_key_12345"));
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

    #[test]
    fn test_format_duration_zero() {
        assert_eq!(format_duration(0), "0s");
    }

    #[test]
    fn test_format_duration_seconds() {
        assert_eq!(format_duration(1), "1s");
        assert_eq!(format_duration(59), "59s");
    }

    #[test]
    fn test_format_duration_exact_minutes() {
        assert_eq!(format_duration(60), "1m");
        assert_eq!(format_duration(120), "2m");
        assert_eq!(format_duration(900), "15m");
    }

    #[test]
    fn test_format_duration_minutes_and_seconds() {
        assert_eq!(format_duration(61), "1m 1s");
        assert_eq!(format_duration(90), "1m 30s");
        assert_eq!(format_duration(125), "2m 5s");
    }

    #[test]
    fn test_format_duration_hours() {
        assert_eq!(format_duration(3600), "1h");
        assert_eq!(format_duration(3661), "1h 1m");
        assert_eq!(format_duration(7200), "2h");
        assert_eq!(format_duration(5400), "1h 30m");
    }

    #[test]
    fn test_format_duration_days() {
        assert_eq!(format_duration(86400), "1d");
        assert_eq!(format_duration(90000), "1d 1h");
        assert_eq!(format_duration(172800), "2d");
    }
}
