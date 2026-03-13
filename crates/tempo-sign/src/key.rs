use minisign::{KeyPair, PublicKey, SecretKeyBox};

use crate::error::SignError;

/// Generate a new minisign keypair and write the secret key box.
pub fn generate_key(path: &str) -> Result<(), SignError> {
    let KeyPair { pk, sk } =
        KeyPair::generate_unencrypted_keypair().map_err(|err| SignError::Crypto {
            operation: "generate keypair",
            source: err,
        })?;

    let sk_box_str = sk
        .to_box(None)
        .map_err(|err| SignError::Crypto {
            operation: "box secret key",
            source: err,
        })?
        .to_string();

    std::fs::write(path, &sk_box_str).map_err(|err| SignError::Io {
        operation: "write key file",
        source: err,
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        if let Err(err) = std::fs::set_permissions(path, perms) {
            eprintln!("warning: failed to set key file permissions: {err}");
        }
    }

    let pk_base64 = pk.to_base64();
    println!("Generated minisign keypair");
    println!("  Secret key box: {path}");
    println!("  Public key (base64): {pk_base64}");
    println!();
    println!("Bake this public key into the verifying application's PUBLIC_KEY constant.");
    println!("Keep {path} secret — it signs release binaries.");
    Ok(())
}

/// Print a public key derived from a secret key box file.
pub fn print_public_key(path: &str) -> Result<(), SignError> {
    let sk = load_secret_key(path)?;
    let pk = PublicKey::from_secret_key(&sk).map_err(|err| SignError::Crypto {
        operation: "derive public key",
        source: err,
    })?;
    println!("{}", pk.to_base64());
    Ok(())
}

/// Load minisign secret key from a secret key box file.
pub fn load_secret_key(path: &str) -> Result<minisign::SecretKey, SignError> {
    let sk_box_str = std::fs::read_to_string(path).map_err(|err| SignError::IoWithPath {
        operation: "read key file",
        path: path.to_string(),
        source: err,
    })?;
    let sk_box =
        SecretKeyBox::from_string(&sk_box_str).map_err(|err| SignError::CryptoWithPath {
            operation: "parse secret key box",
            path: path.to_string(),
            source: err,
        })?;
    sk_box
        .into_unencrypted_secret_key()
        .map_err(|err| SignError::CryptoWithPath {
            operation: "decode secret key",
            path: path.to_string(),
            source: err,
        })
}
