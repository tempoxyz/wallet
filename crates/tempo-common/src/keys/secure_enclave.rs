//! Secure Enclave key operations via the `tempo-se` Swift shim.
//!
//! Only available on macOS with Apple Silicon or T2 chip.
//! Falls back to an error on unsupported platforms.

use alloy::primitives::B256;
use tempo_primitives::transaction::{
    tt_signature::{normalize_p256_s, P256SignatureWithPreHash},
    PrimitiveSignature,
};

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

/// Sign a Tempo transaction hash with a Secure Enclave key and return a
/// `PrimitiveSignature::P256`.
///
/// The SE always signs `SHA-256(input)`, so we pre-hash the `hash_to_sign`
/// with SHA-256 before passing to the shim, and set `pre_hash: true` so the
/// on-chain verifier applies the same transform to the sig_hash.
///
/// The shim returns a DER-encoded ECDSA signature which is parsed into (r, s)
/// with mandatory low-s normalization.
pub fn sign_hash(
    label: &str,
    hash_to_sign: &B256,
    pub_key_x: B256,
    pub_key_y: B256,
) -> Result<PrimitiveSignature, TempoError> {
    use sha2::{Digest, Sha256};

    // Pre-hash: the SE signs SHA-256(input), and we set pre_hash=true so the
    // chain also hashes the sig_hash before verifying.
    let pre_hashed = Sha256::digest(hash_to_sign);
    let pre_hashed_hex = hex::encode(pre_hashed);

    let der_hex = sign(label, &pre_hashed_hex)?;
    let der_bytes = hex::decode(&der_hex)
        .map_err(|_| KeyError::SecureEnclave("invalid DER signature hex from SE".to_string()))?;

    let (r, s) = parse_der_signature(&der_bytes)?;

    Ok(PrimitiveSignature::P256(P256SignatureWithPreHash {
        r: B256::from_slice(&r),
        s: normalize_p256_s(&s),
        pub_key_x,
        pub_key_y,
        pre_hash: true,
    }))
}

/// Parse a DER-encoded ECDSA signature into (r, s) as 32-byte big-endian arrays.
///
/// DER format: `30 <len> 02 <r_len> <r_bytes> 02 <s_len> <s_bytes>`
fn parse_der_signature(der: &[u8]) -> Result<([u8; 32], [u8; 32]), TempoError> {
    if der.len() < 8 || der[0] != 0x30 {
        return Err(
            KeyError::SecureEnclave("invalid DER signature: bad header".to_string()).into(),
        );
    }

    let mut pos = 2; // skip SEQUENCE tag + length

    // Parse r
    if der[pos] != 0x02 {
        return Err(KeyError::SecureEnclave(
            "invalid DER signature: expected INTEGER tag for r".to_string(),
        )
        .into());
    }
    pos += 1;
    let r_len = der[pos] as usize;
    pos += 1;
    let r_bytes = &der[pos..pos + r_len];
    pos += r_len;

    // Parse s
    if der[pos] != 0x02 {
        return Err(KeyError::SecureEnclave(
            "invalid DER signature: expected INTEGER tag for s".to_string(),
        )
        .into());
    }
    pos += 1;
    let s_len = der[pos] as usize;
    pos += 1;
    let s_bytes = &der[pos..pos + s_len];

    Ok((der_integer_to_32(r_bytes), der_integer_to_32(s_bytes)))
}

/// Convert a DER INTEGER value to a 32-byte big-endian array.
///
/// DER INTEGERs may have a leading 0x00 for positive sign or be shorter
/// than 32 bytes.
fn der_integer_to_32(bytes: &[u8]) -> [u8; 32] {
    let mut result = [0u8; 32];
    // Strip leading zero padding (DER sign byte)
    let trimmed = if bytes.len() > 32 && bytes[0] == 0x00 {
        &bytes[1..]
    } else {
        bytes
    };
    // Right-align into 32 bytes
    let start = 32 - trimmed.len();
    result[start..].copy_from_slice(trimmed);
    result
}

/// Check if the Secure Enclave is available on this system.
pub fn is_available() -> bool {
    cfg!(target_os = "macos") && find_tempo_se().is_ok()
}
