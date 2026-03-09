//! Generate a signed release manifest for tempo CLI auto-install.
//!
//! Usage:
//!
//! ```text
//! tempo-sign \
//!     --key-file release.key \
//!     --artifacts-dir artifacts/ \
//!     --version 0.1.0 \
//!     --base-url https://cli.tempo.xyz/tempo-wallet \
//!     --skill https://cli.tempo.xyz/tempo-wallet/v0.1.0/SKILL.md \
//!     --skill-sha256 <SHA256> \
//!     --skill-file crates/tempo-wallet/SKILL.md \
//!     --output manifest.json
//! ```
//!
//! The key file contains a minisign secret key box (unencrypted).
//! Generate one with: tempo-sign --generate-key release.key

use minisign::{KeyPair, PublicKey, SecretKeyBox};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Cursor, Read};
use std::path::Path;
use std::process;

const DEFAULT_BASE_URL: &str = "https://cli.tempo.xyz/tempo-wallet";
const SKIP_EXTENSIONS: &[&str] = &[".json", ".md", ".sh", ".txt", ".py"];

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if let Some(path) = find_flag_value(&args, "--generate-key") {
        generate_key(&path);
        return;
    }

    if let Some(path) = find_flag_value(&args, "--print-public-key") {
        print_public_key(&path);
        return;
    }

    let key_file = find_flag_value(&args, "--key-file");
    let artifacts_dir = find_flag_value(&args, "--artifacts-dir");
    let version = find_flag_value(&args, "--version");
    let base_url =
        find_flag_value(&args, "--base-url").unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
    let skill = find_flag_value(&args, "--skill");
    let skill_sha256 = find_flag_value(&args, "--skill-sha256");
    let skill_file = find_flag_value(&args, "--skill-file");
    let output = find_flag_value(&args, "--output").unwrap_or_else(|| "manifest.json".to_string());

    let (Some(key_file), Some(artifacts_dir), Some(version)) = (key_file, artifacts_dir, version)
    else {
        eprintln!("error: --key-file, --artifacts-dir, and --version are required");
        process::exit(2);
    };

    let sk = load_secret_key(&key_file);
    let pk = PublicKey::from_secret_key(&sk).unwrap();
    let pk_base64 = pk.to_base64();

    println!("Signing release {version}");
    println!("  Public key: {pk_base64}");
    println!("  Artifacts: {artifacts_dir}");
    println!();

    let manifest = build_manifest(
        &artifacts_dir,
        &version,
        &base_url,
        skill.as_deref(),
        skill_sha256.as_deref(),
        skill_file.as_deref(),
        &sk,
    );

    let json = serde_json::to_string_pretty(&manifest).unwrap_or_else(|err| {
        eprintln!("error: failed to serialize manifest: {err}");
        process::exit(1);
    });
    fs::write(&output, format!("{json}\n")).unwrap_or_else(|err| {
        eprintln!("error: failed to write {output}: {err}");
        process::exit(1);
    });

    let count = manifest["binaries"]
        .as_object()
        .map(|m| m.len())
        .unwrap_or(0);
    println!();
    println!("Wrote {output} ({count} binaries)");
}

fn generate_key(path: &str) {
    let KeyPair { pk, sk } = KeyPair::generate_unencrypted_keypair().unwrap_or_else(|err| {
        eprintln!("error: failed to generate keypair: {err}");
        process::exit(1);
    });

    let sk_box_str = sk.to_box(None).unwrap_or_else(|err| {
        eprintln!("error: failed to box secret key: {err}");
        process::exit(1);
    }).to_string();

    fs::write(path, &sk_box_str).unwrap_or_else(|err| {
        eprintln!("error: failed to write key file: {err}");
        process::exit(1);
    });

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        fs::set_permissions(path, perms).unwrap_or_else(|err| {
            eprintln!("warning: failed to set key file permissions: {err}");
        });
    }

    let pk_base64 = pk.to_base64();
    println!("Generated minisign keypair");
    println!("  Secret key box: {path}");
    println!("  Public key (base64): {pk_base64}");
    println!();
    println!("Bake this public key into the Tempo CLI (src/launcher.rs PUBLIC_KEY constant).");
    println!("Keep {path} secret — it signs release binaries.");
}

fn print_public_key(path: &str) {
    let sk = load_secret_key(path);
    println!("{}", PublicKey::from_secret_key(&sk).unwrap().to_base64());
}

fn load_secret_key(path: &str) -> minisign::SecretKey {
    let sk_box_str = fs::read_to_string(path).unwrap_or_else(|err| {
        eprintln!("error: failed to read key file {path}: {err}");
        process::exit(1);
    });
    let sk_box = SecretKeyBox::from_string(&sk_box_str).unwrap_or_else(|err| {
        eprintln!("error: invalid minisign secret key box in {path}: {err}");
        process::exit(1);
    });
    sk_box.into_secret_key(None).unwrap_or_else(|err| {
        eprintln!("error: failed to decode secret key from {path}: {err}");
        process::exit(1);
    })
}

fn sha256_file(path: &Path) -> String {
    let mut file = fs::File::open(path).unwrap_or_else(|err| {
        eprintln!("error: failed to open {}: {err}", path.display());
        process::exit(1);
    });
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file.read(&mut buf).unwrap_or_else(|err| {
            eprintln!("error: failed to read {}: {err}", path.display());
            process::exit(1);
        });
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    format!("{:x}", hasher.finalize())
}

fn sign_file(path: &Path, sk: &minisign::SecretKey) -> String {
    let data = fs::read(path).unwrap_or_else(|err| {
        eprintln!("error: failed to read {}: {err}", path.display());
        process::exit(1);
    });
    let filename = path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();
    let pk = PublicKey::from_secret_key(sk).unwrap();
    let sig_box = minisign::sign(
        Some(&pk),
        sk,
        Cursor::new(&data),
        Some(&format!("file:{filename}")),
        Some("tempo release signature"),
    )
    .unwrap_or_else(|err| {
        eprintln!("error: failed to sign {}: {err}", path.display());
        process::exit(1);
    });
    sig_box.into_string()
}

fn build_manifest(
    artifacts_dir: &str,
    version: &str,
    base_url: &str,
    skill: Option<&str>,
    skill_sha256: Option<&str>,
    skill_file: Option<&str>,
    sk: &minisign::SecretKey,
) -> serde_json::Value {
    let base_url = base_url.trim_end_matches('/');
    let version_prefix = if version.starts_with('v') {
        version.to_string()
    } else {
        format!("v{version}")
    };

    let mut binaries = BTreeMap::new();

    let mut entries: Vec<_> = fs::read_dir(artifacts_dir)
        .unwrap_or_else(|err| {
            eprintln!("error: failed to read artifacts directory {artifacts_dir}: {err}");
            process::exit(1);
        })
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let filename = entry.file_name();
        let filename = filename.to_string_lossy();
        if SKIP_EXTENSIONS.iter().any(|ext| filename.ends_with(ext)) {
            continue;
        }

        let checksum = sha256_file(&path);
        let signature = sign_file(&path, sk);

        println!("  signed {filename} (sha256: {}...)", &checksum[..16]);

        binaries.insert(
            filename.to_string(),
            json!({
                "url": format!("{base_url}/{version_prefix}/{filename}"),
                "sha256": checksum,
                "signature": signature,
            }),
        );
    }

    let mut manifest = json!({
        "version": version_prefix,
        "binaries": binaries,
    });
    if let Some(skill_url) = skill {
        manifest["skill"] = json!(skill_url);
    }
    if let Some(sha256) = skill_sha256 {
        manifest["skill_sha256"] = json!(sha256);
    }
    if let Some(path) = skill_file {
        let skill_path = Path::new(path);
        let signature = sign_file(skill_path, sk);
        manifest["skill_signature"] = json!(signature);
        println!("  signed SKILL.md");
    }
    manifest
}

fn find_flag_value(args: &[String], flag: &str) -> Option<String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == flag {
            return iter.next().cloned();
        }
    }
    None
}
