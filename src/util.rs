//! Utility functions and constants.

use std::fs;
use std::io::Write;
use std::path::Path;

use crate::error::PrestoError;

// ── Atomic file writes ──────────────────────────────────────────────

pub fn atomic_write(
    path: &Path,
    contents: &str,
    #[allow(unused_variables)] unix_mode: u32,
) -> Result<(), PrestoError> {
    let parent = path.parent().ok_or_else(|| {
        PrestoError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("path has no parent directory: {}", path.display()),
        ))
    })?;

    fs::create_dir_all(parent)?;

    // Create temp file in the same directory (ensures same filesystem for rename)
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temp.as_file()
            .set_permissions(fs::Permissions::from_mode(unix_mode))?;
    }

    temp.write_all(contents.as_bytes())?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| PrestoError::Io(e.error))?;

    Ok(())
}

// ── Terminal output sanitization ─────────────────────────────────────

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

// ── Terminal hyperlinks (OSC 8) ──────────────────────────────────────

/// Format text as a clickable hyperlink using the OSC 8 protocol.
///
/// In terminals that support OSC 8 hyperlinks (iTerm2, WezTerm, VSCode, Ghostty, etc.),
/// the text will be clickable and open the URL when clicked.
/// In terminals that don't support hyperlinks, the text is returned unchanged.
///
/// Both `text` and `url` are sanitized to strip control characters, preventing
/// terminal escape injection from server-controlled data.
pub fn hyperlink(text: &str, url: &str) -> String {
    let clean_text = sanitize_for_terminal(text);
    if supports_hyperlinks() {
        let clean_url = sanitize_for_terminal(url);
        format!("\x1b]8;;{}\x07{}\x1b]8;;\x07", clean_url, clean_text)
    } else {
        clean_text
    }
}

/// Check if the current terminal supports OSC 8 hyperlinks.
pub fn supports_hyperlinks() -> bool {
    use std::sync::OnceLock;
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

// ── U256 formatting ─────────────────────────────────────────────────

/// Format a U256 value with the given number of decimal places.
///
/// Converts atomic units to a human-readable decimal string.
/// For example, `1000000` with 6 decimals becomes `"1.000000"`.
pub fn format_u256_with_decimals(value: alloy::primitives::U256, decimals: u8) -> String {
    use alloy::primitives::U256;

    if decimals == 0 {
        return value.to_string();
    }

    let divisor = U256::from(10u64).pow(U256::from(decimals));
    let whole = value / divisor;
    let remainder = value % divisor;

    let remainder_str = remainder.to_string();
    let padded = format!("{:0>width$}", remainder_str, width = decimals as usize);

    format!("{}.{}", whole, padded)
}

/// Format atomic token units as a human-readable string with trimmed trailing zeros.
pub fn format_token_amount(atomic: u128, symbol: &str, decimals: u8) -> String {
    let divisor = 10u128.pow(decimals as u32);
    let whole = atomic / divisor;
    let remainder = atomic % divisor;

    if remainder == 0 {
        format!("{whole} {symbol}")
    } else {
        let frac_str = format!("{:0width$}", remainder, width = decimals as usize);
        let trimmed = frac_str.trim_end_matches('0');
        format!("{whole}.{trimmed} {symbol}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::tempdir;

    // ── Atomic write tests ──────────────────────────────────────────

    #[test]
    fn test_atomic_write_creates_file() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("test.txt");

        atomic_write(&path, "hello world", 0o644).expect("write");

        assert_eq!(fs::read_to_string(&path).expect("read"), "hello world");
    }

    #[test]
    fn test_atomic_write_creates_parent_dirs() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("a").join("b").join("c").join("test.txt");

        atomic_write(&path, "nested content", 0o644).expect("write");

        assert_eq!(fs::read_to_string(&path).expect("read"), "nested content");
    }

    #[test]
    fn test_atomic_write_overwrites_existing() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("test.txt");

        atomic_write(&path, "first", 0o644).expect("first write");
        atomic_write(&path, "second", 0o644).expect("second write");

        assert_eq!(fs::read_to_string(&path).expect("read"), "second");
    }

    #[test]
    fn test_atomic_write_no_temp_left_on_success() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("test.txt");

        atomic_write(&path, "content", 0o644).expect("write");

        let tmp_files: Vec<_> = fs::read_dir(dir.path())
            .expect("read_dir")
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();

        assert!(tmp_files.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn test_atomic_write_sets_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("test.txt");

        atomic_write(&path, "secret", 0o600).expect("write");

        let metadata = fs::metadata(&path).expect("metadata");
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn test_atomic_write_empty_content() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("empty.txt");

        atomic_write(&path, "", 0o644).expect("write");

        let content = fs::read_to_string(&path).expect("read");
        assert!(content.is_empty());
    }

    #[test]
    fn test_atomic_write_temp_cleaned_up_on_rename_failure() {
        let dir = tempdir().expect("tempdir");
        let nested = dir.path().join("subdir");
        fs::create_dir(&nested).expect("mkdir");

        let target = nested.join("target.txt");
        atomic_write(&target, "original", 0o644).expect("write");

        fs::remove_dir_all(&nested).expect("remove");

        let result = atomic_write(&target, "new content", 0o644);
        assert!(result.is_ok());
    }

    #[test]
    fn test_atomic_write_no_temp_left_on_failure() {
        let dir = tempdir().expect("tempdir");
        let blocker = dir.path().join("subdir");
        fs::write(&blocker, "i am a file").expect("write blocker");

        let path = blocker.join("test.txt");
        let result = atomic_write(&path, "content", 0o644);
        assert!(result.is_err());

        let tmp_files: Vec<_> = fs::read_dir(dir.path())
            .expect("read_dir")
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(tmp_files.is_empty());
    }

    #[test]
    fn test_atomic_write_parent_is_file_not_dir() {
        let dir = tempdir().expect("tempdir");
        let blocker = dir.path().join("not_a_dir");
        fs::write(&blocker, "i am a file").expect("write blocker");

        let path = blocker.join("test.txt");
        let result = atomic_write(&path, "content", 0o644);
        assert!(result.is_err());
    }

    #[test]
    fn test_atomic_write_preserves_old_content_on_dir_failure() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("test.txt");

        atomic_write(&path, "original", 0o644).expect("write");

        let bad_path = Path::new("/dev/null/impossible/test.txt");
        let result = atomic_write(bad_path, "new content", 0o644);
        assert!(result.is_err());

        assert_eq!(fs::read_to_string(&path).expect("read"), "original");
    }

    #[cfg(unix)]
    #[test]
    fn test_atomic_write_does_not_follow_symlink_for_temp() {
        let dir = tempdir().expect("tempdir");
        let target_dir = tempdir().expect("target tempdir");
        let decoy = target_dir.path().join("decoy.txt");
        fs::write(&decoy, "original decoy").expect("write decoy");

        let link_path = dir.path().join("config.txt");
        std::os::unix::fs::symlink(&decoy, &link_path).expect("symlink");

        atomic_write(&link_path, "overwritten", 0o644).expect("write");

        let link_content = fs::read_to_string(&link_path).expect("read");
        assert_eq!(link_content, "overwritten");
    }

    // ── format_u256_with_decimals tests ────────────────────────────────

    #[test]
    fn test_format_u256_zero() {
        use alloy::primitives::U256;
        assert_eq!(format_u256_with_decimals(U256::from(0), 6), "0.000000");
    }

    #[test]
    fn test_format_u256_zero_decimals() {
        use alloy::primitives::U256;
        assert_eq!(format_u256_with_decimals(U256::from(12345), 0), "12345");
    }

    #[test]
    fn test_format_u256_small_value() {
        use alloy::primitives::U256;
        assert_eq!(format_u256_with_decimals(U256::from(1), 6), "0.000001");
    }

    #[test]
    fn test_format_u256_exact_divisor() {
        use alloy::primitives::U256;
        assert_eq!(
            format_u256_with_decimals(U256::from(1_000_000u64), 6),
            "1.000000"
        );
    }

    #[test]
    fn test_format_u256_large_value() {
        use alloy::primitives::U256;
        assert_eq!(
            format_u256_with_decimals(U256::from(123_456_789u64), 6),
            "123.456789"
        );
    }

    #[test]
    fn test_format_u256_max() {
        use alloy::primitives::U256;
        let result = format_u256_with_decimals(U256::MAX, 18);
        assert!(result.contains('.'));
        assert!(!result.is_empty());
    }

    // ── Terminal escape injection tests ────────────────────────────────

    #[test]
    fn test_hyperlink_sanitizes_escape_sequences_in_text() {
        // A malicious server could return a payment-receipt header with ANSI escape
        // sequences in the tx hash / reference field. These get passed to hyperlink()
        // as the text parameter. The output must not contain raw escape sequences,
        // even in the non-hyperlink (plain text) fallback path.
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
        // BEL (\x07) is the OSC 8 terminator. If a malicious reference contains it,
        // the attacker can break out of the OSC 8 sequence and inject a phishing URL.
        // After sanitization, the output must contain no control characters — the
        // literal "evil.com" text is harmless without ESC/BEL to form an OSC 8 link.
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
        // The URL parameter is derived from server-controlled data (via tx_url()).
        // In a real terminal, hyperlink() produces OSC 8: \x1b]8;;{url}\x07{text}\x1b]8;;\x07
        // A malicious URL containing BEL (\x07) breaks out of the OSC 8 sequence
        // and lets the attacker inject a phishing hyperlink.
        //
        // Verify sanitize_for_terminal strips the control characters that enable
        // the OSC 8 breakout, so the formatted output is safe.
        let malicious_url =
            "https://explorer.tempo.xyz/tx/0xabc\x07\x1b]8;;https://evil.com\x07fake";

        let clean_url = sanitize_for_terminal(malicious_url);

        // After sanitization, the URL must not contain BEL or ESC — those are what
        // enable the attacker to break OSC 8 framing and inject a phishing link.
        assert!(
            !clean_url.contains('\x07') && !clean_url.contains('\x1b'),
            "sanitized URL must not contain BEL/ESC control characters: {:?}",
            clean_url
        );
    }

    #[test]
    fn test_hyperlink_strips_cursor_manipulation() {
        // Escape sequences like "cursor up" + "erase line" can forge success messages.
        let malicious_text = "0xabc\x1b[A\x1b[2KPayment successful: 0.00 USDC";
        let url = "https://explorer.tempo.xyz/tx/0xabc";

        let result = hyperlink(malicious_text, url);

        assert!(
            !result.contains("\x1b[A"),
            "hyperlink() must strip cursor manipulation sequences, got: {:?}",
            result
        );
        assert!(
            !result.contains("\x1b[2K"),
            "hyperlink() must strip erase-line sequences, got: {:?}",
            result
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_atomic_write_permissions_on_overwrite() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("test.txt");

        fs::write(&path, "world readable").expect("write");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("set_permissions");

        atomic_write(&path, "now restricted", 0o600).expect("write");

        let mode = fs::metadata(&path).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(fs::read_to_string(&path).expect("read"), "now restricted");
    }

    // ── Hyperlink tests ─────────────────────────────────────────────

    #[test]
    fn test_hyperlink_format() {
        let url = "https://etherscan.io/tx/0x123";
        let text = "View transaction";
        let expected = "\x1b]8;;https://etherscan.io/tx/0x123\x07View transaction\x1b]8;;\x07";
        assert_eq!(format!("\x1b]8;;{}\x07{}\x1b]8;;\x07", url, text), expected);
    }
}
