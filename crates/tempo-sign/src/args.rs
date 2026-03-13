use clap::{Parser, Subcommand};

/// Generate signed release manifests for Tempo CLI extensions.
#[derive(Parser)]
#[command(name = "tempo-sign")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
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
        #[arg(long, default_value = "https://cli.tempo.xyz/extensions/tempo-wallet")]
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
