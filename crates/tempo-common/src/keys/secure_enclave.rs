//! Secure Enclave key operations via the `tempo-se` Swift shim.
//!
//! Only available on macOS with Apple Silicon or T2 chip.
//! Falls back to an error on unsupported platforms.

use crate::error::{KeyError, TempoError};

/// Keychain tag prefix for SE-managed keys.
const TAG_PREFIX: &str = "xyz.tempo.wallet.se.";

/// Build the full Keychain tag from a key label.
fn keychain_tag(label: &str) -> String {
    format!("{TAG_PREFIX}{label}")
}

/// Locate the `tempo-se` binary.
///
/// Searches next to the current executable, then falls back to PATH.
fn find_tempo_se() -> Result<std::path::PathBuf, TempoError> {
    // First: check next to the running binary
    if let Ok(exe) = std::env::current_exe() {
        let candidate = exe.with_file_name("tempo-se");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    // Fallback: search PATH
    which::which("tempo-se").map_err(|_| {
        KeyError::SecureEnclave(
            "tempo-se binary not found. Install with: swift build -c release --package-path tools/tempo-se".to_string(),
        )
        .into()
    })
}

/// Run the `tempo-se` shim with the given arguments.
fn run_shim(args: &[&str]) -> Result<String, TempoError> {
    let bin = find_tempo_se()?;
    let output = std::process::Command::new(&bin)
        .args(args)
        .output()
        .map_err(|e| KeyError::SecureEnclave(format!("failed to run tempo-se: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(KeyError::SecureEnclave(format!("tempo-se failed: {}", stderr.trim())).into());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Generate a new Secure Enclave key and return the uncompressed public key hex.
pub fn generate(label: &str) -> Result<String, TempoError> {
    let tag = keychain_tag(label);
    run_shim(&["generate", "--tag", &tag])
}

/// Sign a 32-byte hash and return the DER signature hex.
pub fn sign(label: &str, hash_hex: &str) -> Result<String, TempoError> {
    let tag = keychain_tag(label);
    run_shim(&["sign", "--tag", &tag, "--hash", hash_hex])
}

/// Retrieve the public key hex for an existing SE key.
pub fn pubkey(label: &str) -> Result<String, TempoError> {
    let tag = keychain_tag(label);
    run_shim(&["pubkey", "--tag", &tag])
}

/// Delete an SE key from the Keychain.
pub fn delete(label: &str) -> Result<(), TempoError> {
    let tag = keychain_tag(label);
    run_shim(&["delete", "--tag", &tag]).map(|_| ())
}

/// Check if the Secure Enclave is available on this system.
pub fn is_available() -> bool {
    cfg!(target_os = "macos") && find_tempo_se().is_ok()
}
