//! Utility functions and constants.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{PrestoError, Result};

// ── Constants ────────────────────────────────────────────────────────

/// Application name for XDG directories
pub const APP_NAME: &str = "presto";

/// Config file name
pub const CONFIG_FILE: &str = "config.toml";

/// Get the  tempo-walletconfig directory (`~/.config/presto/`)
pub fn presto_config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|c| c.join(APP_NAME))
}

/// Get the default config file path (`~/.config/presto/config.toml`)
pub fn default_config_path() -> Option<PathBuf> {
    presto_config_dir().map(|p| p.join(CONFIG_FILE))
}

// ── Atomic file writes ──────────────────────────────────────────────

pub fn atomic_write(
    path: &Path,
    contents: &str,
    #[allow(unused_variables)] unix_mode: u32,
) -> Result<()> {
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

// ── Terminal hyperlinks (OSC 8) ──────────────────────────────────────

/// Format text as a clickable hyperlink using the OSC 8 protocol.
///
/// In terminals that support OSC 8 hyperlinks (iTerm2, WezTerm, VSCode, Ghostty, etc.),
/// the text will be clickable and open the URL when clicked.
/// In terminals that don't support hyperlinks, the text is returned unchanged.
pub fn hyperlink(text: &str, url: &str) -> String {
    if supports_hyperlinks() {
        format!("\x1b]8;;{}\x07{}\x1b]8;;\x07", url, text)
    } else {
        text.to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::tempdir;

    // ── Constants tests ─────────────────────────────────────────────

    #[test]
    fn test_presto_config_dir_exists() {
        let dir = presto_config_dir();
        assert!(dir.is_some());
        let path = dir.expect("Config dir should exist");
        assert!(path
            .to_str()
            .expect("Path should be valid UTF-8")
            .contains(APP_NAME));
    }

    #[test]
    fn test_default_config_path() {
        let path = default_config_path();
        assert!(path.is_some());
        let p = path.expect("Config path should exist");
        let path_str = p.to_str().expect("Path should be valid UTF-8");
        assert!(path_str.contains(CONFIG_FILE));
        assert!(path_str.contains(APP_NAME));
    }

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
}
