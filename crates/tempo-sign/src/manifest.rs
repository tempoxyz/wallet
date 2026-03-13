use std::collections::BTreeMap;
use std::path::Path;

use serde_json::json;

use crate::error::SignError;
use crate::sign::{sha256_file, sign_file, SKIP_EXTENSIONS};

#[allow(clippy::too_many_arguments)]
pub fn build_manifest(
    artifacts_dir: &str,
    version: &str,
    base_url: &str,
    description: Option<&str>,
    skill: Option<&str>,
    skill_sha256: Option<&str>,
    skill_file: Option<&str>,
    sk: &minisign::SecretKey,
) -> Result<serde_json::Value, SignError> {
    let base_url = base_url.trim_end_matches('/');
    let version_prefix = if version.starts_with('v') {
        version.to_string()
    } else {
        format!("v{version}")
    };

    let mut binaries = BTreeMap::new();

    let mut entries: Vec<_> = std::fs::read_dir(artifacts_dir)
        .map_err(|source| SignError::IoWithPath {
            operation: "read artifacts directory",
            path: artifacts_dir.to_string(),
            source,
        })?
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

        let checksum = sha256_file(&path)?;
        let binary_comment = format!("file:{filename}\tversion:{version_prefix}");
        let signature = sign_file(&path, Some(&binary_comment), sk)?;

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
        let signature = sign_file(skill_path, Some(&skill_comment), sk)?;
        manifest["skill_signature"] = json!(signature);
        println!("  signed SKILL.md");
    }
    Ok(manifest)
}
