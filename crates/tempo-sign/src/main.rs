//! Generate a signed release manifest for tempo CLI auto-install.

mod args;
mod error;
mod key;
mod manifest;
mod sign;

use clap::Parser;
use minisign::PublicKey;

use args::{Cli, Commands};
use error::SignError;

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), SignError> {
    let cli = Cli::parse();

    match cli.command {
        Commands::GenerateKey { path } => key::generate_key(&path),
        Commands::PrintPublicKey { path } => key::print_public_key(&path),
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
            let sk = key::load_secret_key(&key_file)?;
            let pk = PublicKey::from_secret_key(&sk).map_err(|err| SignError::Crypto {
                operation: "derive public key",
                source: err,
            })?;
            let pk_base64 = pk.to_base64();

            println!("Signing release {version}");
            println!("  Public key: {pk_base64}");
            println!("  Artifacts: {artifacts_dir}");
            println!();

            let manifest = manifest::build_manifest(
                &artifacts_dir,
                &version,
                &base_url,
                description.as_deref(),
                skill.as_deref(),
                skill_sha256.as_deref(),
                skill_file.as_deref(),
                &sk,
            )?;

            let json = serde_json::to_string_pretty(&manifest).map_err(|source| {
                SignError::Serialization {
                    operation: "serialize manifest",
                    source,
                }
            })?;

            std::fs::write(&output, format!("{json}\n")).map_err(|source| {
                SignError::IoWithPath {
                    operation: "write manifest",
                    path: output.clone(),
                    source,
                }
            })?;

            let count = manifest["binaries"]
                .as_object()
                .map_or(0, serde_json::Map::len);
            println!();
            println!("Wrote {output} ({count} binaries)");
            Ok(())
        }
    }
}
