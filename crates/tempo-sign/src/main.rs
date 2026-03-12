//! Generate a signed release manifest for tempo CLI auto-install.
//!
//! Usage:
//!
//! ```text
//! tempo-sign sign \
//!     --key-file release.key \
//!     --artifacts-dir artifacts/ \
//!     --version 0.1.0 \
//!     --base-url https://cli.tempo.xyz/tempo-wallet \
//!     --description "Manage your Tempo Wallet" \
//!     --skill https://cli.tempo.xyz/tempo-wallet/v0.1.0/SKILL.md \
//!     --skill-sha256 <SHA256> \
//!     --skill-file crates/tempo-wallet/SKILL.md \
//!     --output manifest.json
//! ```
//!
//! The key file contains a minisign secret key box (unencrypted).
//! Generate one with: tempo-sign generate-key <path>

use clap::{Parser, Subcommand};
use minisign::{KeyPair, PublicKey, SecretKeyBox};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Cursor, Read};
use std::path::Path;
use std::process;

const SKIP_EXTENSIONS: &[&str] = &[".json", ".md", ".sh", ".txt", ".py"];

/// Generate signed release manifests for Tempo CLI extensions.
#[derive(Parser)]
#[command(name = "tempo-sign")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate a new minisign keypair
    GenerateKey {
        /// Path to write the secret key file
        path: String,
    },
    /// Print the public key from a secret key file
    PrintPublicKey {
        /// Path to the secret key file
        path: String,
    },
    /// Sign release artifacts and generate a manifest
    Sign {
        /// Path to the minisign secret key file
        #[arg(long)]
        key_file: String,
        /// Directory containing release artifacts to sign
        #[arg(long)]
        artifacts_dir: String,
        /// Release version (e.g., "0.1.0")
        #[arg(long)]
        version: String,
        /// Base URL for download links
        #[arg(long, default_value = "https://cli.tempo.xyz/tempo-wallet")]
        base_url: String,
        /// Extension description
        #[arg(long)]
        description: Option<String>,
        /// URL for the SKILL.md file
        #[arg(long)]
        skill: Option<String>,
        /// SHA256 hash of the SKILL.md file
        #[arg(long)]
        skill_sha256: Option<String>,
        /// Local path to the SKILL.md file for signing
        #[arg(long)]
        skill_file: Option<String>,
        /// Output path for the manifest JSON
        #[arg(long, default_value = "manifest.json")]
        output: String,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::GenerateKey { path } => generate_key(&path),
        Commands::PrintPublicKey { path } => print_public_key(&path),
        Commands::Sign {
            key_file,
            artifacts_dir,
            version,
            base_url,
            description,
            skill,
            skill_sha256,
            skill_file,
            output,
        } => {
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
                description.as_deref(),
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
    }
}

fn generate_key(path: &str) {
    let KeyPair { pk, sk } = KeyPair::generate_unencrypted_keypair().unwrap_or_else(|err| {
        eprintln!("error: failed to generate keypair: {err}");
        process::exit(1);
    });

    let sk_box_str = sk
        .to_box(None)
        .unwrap_or_else(|err| {
            eprintln!("error: failed to box secret key: {err}");
            process::exit(1);
        })
        .to_string();

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
    println!("Bake this public key into the verifying application's PUBLIC_KEY constant.");
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
    sk_box.into_unencrypted_secret_key().unwrap_or_else(|err| {
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

fn sign_file(path: &Path, trusted_comment: Option<&str>, sk: &minisign::SecretKey) -> String {
    let data = fs::read(path).unwrap_or_else(|err| {
        eprintln!("error: failed to read {}: {err}", path.display());
        process::exit(1);
    });
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
    let pk = PublicKey::from_secret_key(sk).unwrap();
    let sig_box = minisign::sign(
        Some(&pk),
        sk,
        Cursor::new(&data),
        Some(comment),
        Some("tempo release signature"),
    )
    .unwrap_or_else(|err| {
        eprintln!("error: failed to sign {}: {err}", path.display());
        process::exit(1);
    });
    sig_box.into_string()
}

#[allow(clippy::too_many_arguments)]
fn build_manifest(
    artifacts_dir: &str,
    version: &str,
    base_url: &str,
    description: Option<&str>,
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
        let binary_comment = format!("file:{filename}\tversion:{version_prefix}");
        let signature = sign_file(&path, Some(&binary_comment), sk);

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
    if let Some(desc) = description {
        manifest["description"] = json!(desc);
    }
    if let Some(skill_url) = skill {
        manifest["skill"] = json!(skill_url);
    }
    if let Some(sha256) = skill_sha256 {
        manifest["skill_sha256"] = json!(sha256);
    }
    if let Some(path) = skill_file {
        let skill_path = Path::new(path);
        // The trusted comment must match what the verifier expects:
        // "skill:<package-name>" where package-name is the last segment
        // of the base URL (e.g. "tempo-wallet").
        let pkg_name = base_url.rsplit('/').next().unwrap_or("unknown");
        let skill_comment = format!("skill:{pkg_name}\tversion:{version_prefix}");
        let signature = sign_file(skill_path, Some(&skill_comment), sk);
        manifest["skill_signature"] = json!(signature);
        println!("  signed SKILL.md");
    }
    manifest
}
