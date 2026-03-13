use std::io::{Cursor, Read};
use std::path::Path;

use minisign::PublicKey;
use sha2::{Digest, Sha256};

use crate::error::SignError;

/// Skip non-binary artifacts when building a release manifest.
pub const SKIP_EXTENSIONS: &[&str] = &[".json", ".md", ".sh", ".txt", ".py"];

/// Compute SHA-256 for a file.
pub fn sha256_file(path: &Path) -> Result<String, SignError> {
    let mut file = std::fs::File::open(path).map_err(|err| SignError::IoWithPath {
        operation: "open artifact",
        path: path.display().to_string(),
        source: err,
    })?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file.read(&mut buf).map_err(|err| SignError::IoWithPath {
            operation: "read artifact",
            path: path.display().to_string(),
            source: err,
        })?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Produce a minisign signature for a file.
pub fn sign_file(
    path: &Path,
    trusted_comment: Option<&str>,
    sk: &minisign::SecretKey,
) -> Result<String, SignError> {
    let data = std::fs::read(path).map_err(|err| SignError::IoWithPath {
        operation: "read artifact",
        path: path.display().to_string(),
        source: err,
    })?;
    let default_comment;
    let comment = match trusted_comment {
        Some(c) => c,
        None => {
            let filename = path
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default();
            default_comment = format!("file:{filename}");
            &default_comment
        }
    };
    let pk = PublicKey::from_secret_key(sk).map_err(|err| SignError::Crypto {
        operation: "derive public key",
        source: err,
    })?;
    let sig_box = minisign::sign(
        Some(&pk),
        sk,
        Cursor::new(&data),
        Some(comment),
        Some("tempo release signature"),
    )
    .map_err(|err| SignError::CryptoWithPath {
        operation: "sign artifact",
        path: path.display().to_string(),
        source: err,
    })?;
    Ok(sig_box.into_string())
}
