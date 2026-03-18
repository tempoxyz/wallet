//! Integration tests for tempo-sign.

use std::{fs, process::Command};

fn tempo_sign() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("tempo-sign"))
}

// ── Missing required flags ──────────────────────────────────────────────

#[test]
fn missing_subcommand_exits_2() {
    let output = tempo_sign().output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Usage") || stderr.contains("subcommand"),
        "should mention usage or subcommand: {stderr}"
    );
}

#[test]
fn missing_version_exits_2() {
    let tmp = tempfile::tempdir().unwrap();
    let key_path = tmp.path().join("test.key");
    let artifacts = tmp.path().join("artifacts");
    fs::create_dir(&artifacts).unwrap();

    // Generate a key first
    let gen = tempo_sign()
        .args(["generate-key", key_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(gen.status.success());

    let output = tempo_sign()
        .args([
            "sign",
            "--key-file",
            key_path.to_str().unwrap(),
            "--artifacts-dir",
            artifacts.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}

// ── Key generation ──────────────────────────────────────────────────────

#[test]
fn generate_key_creates_file() {
    let tmp = tempfile::tempdir().unwrap();
    let key_path = tmp.path().join("release.key");

    let output = tempo_sign()
        .args(["generate-key", key_path.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(key_path.exists(), "key file should be created");

    let content = fs::read_to_string(&key_path).unwrap();
    assert!(
        content.contains("secret key"),
        "key file should contain secret key box: {content}"
    );

    // Verify file permissions on unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&key_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "key file should be mode 0600, got {mode:o}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Public key"),
        "should print public key: {stdout}"
    );
}

// ── Print public key ────────────────────────────────────────────────────

#[test]
fn print_public_key_outputs_base64() {
    let tmp = tempfile::tempdir().unwrap();
    let key_path = tmp.path().join("test.key");

    // Generate key first
    let gen = tempo_sign()
        .args(["generate-key", key_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(gen.status.success());

    let output = tempo_sign()
        .args(["print-public-key", key_path.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let pk = stdout.trim();
    // minisign public keys are base64-encoded, typically ~56 chars
    assert!(
        pk.len() > 20
            && pk
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '='),
        "should be valid base64: {pk}"
    );
}

#[test]
fn print_public_key_invalid_file_exits_1() {
    let output = tempo_sign()
        .args(["print-public-key", "/nonexistent/key.file"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error"), "should show error: {stderr}");
}

// ── Full signing flow ───────────────────────────────────────────────────

fn setup_signing_env(tmp: &tempfile::TempDir) -> (String, String) {
    let key_path = tmp.path().join("release.key");
    let artifacts = tmp.path().join("artifacts");
    fs::create_dir(&artifacts).unwrap();

    // Generate key
    let gen = tempo_sign()
        .args(["generate-key", key_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(gen.status.success());

    (
        key_path.to_str().unwrap().to_string(),
        artifacts.to_str().unwrap().to_string(),
    )
}

#[test]
fn sign_artifacts_produces_manifest() {
    let tmp = tempfile::tempdir().unwrap();
    let (key_path, artifacts_dir) = setup_signing_env(&tmp);
    let output_path = tmp.path().join("manifest.json");

    // Create fake binary artifacts
    fs::write(
        tmp.path().join("artifacts/tempo-wallet-x86_64-linux"),
        b"fake-binary-linux",
    )
    .unwrap();
    fs::write(
        tmp.path().join("artifacts/tempo-wallet-aarch64-darwin"),
        b"fake-binary-darwin",
    )
    .unwrap();

    let output = tempo_sign()
        .args([
            "sign",
            "--key-file",
            &key_path,
            "--artifacts-dir",
            &artifacts_dir,
            "--version",
            "0.1.0",
            "--output",
            output_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "signing should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output_path.exists(), "manifest.json should be created");

    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&output_path).unwrap()).unwrap();

    assert_eq!(manifest["version"], "v0.1.0");
    let binaries = manifest["binaries"].as_object().unwrap();
    assert_eq!(binaries.len(), 2, "should have 2 binaries");
    assert!(binaries.contains_key("tempo-wallet-x86_64-linux"));
    assert!(binaries.contains_key("tempo-wallet-aarch64-darwin"));

    // Each binary should have url, sha256, signature
    for (name, entry) in binaries {
        assert!(entry["url"].is_string(), "missing url for {name}");
        assert!(entry["sha256"].is_string(), "missing sha256 for {name}");
        assert!(
            entry["signature"].is_string(),
            "missing signature for {name}"
        );
        let url = entry["url"].as_str().unwrap();
        assert!(
            url.starts_with("https://cli.tempo.xyz/extensions/tempo-wallet/v0.1.0/"),
            "url should use default base: {url}"
        );
    }
}

#[test]
fn sign_version_prefix_normalized() {
    let tmp = tempfile::tempdir().unwrap();
    let (key_path, artifacts_dir) = setup_signing_env(&tmp);
    let output_path = tmp.path().join("manifest.json");

    fs::write(tmp.path().join("artifacts/binary"), b"content").unwrap();

    // Pass version already prefixed with 'v'
    let output = tempo_sign()
        .args([
            "sign",
            "--key-file",
            &key_path,
            "--artifacts-dir",
            &artifacts_dir,
            "--version",
            "v2.0.0",
            "--output",
            output_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&output_path).unwrap()).unwrap();
    assert_eq!(
        manifest["version"], "v2.0.0",
        "should not double-prefix: {manifest}"
    );
}

#[test]
fn sign_skips_non_binary_extensions() {
    let tmp = tempfile::tempdir().unwrap();
    let (key_path, artifacts_dir) = setup_signing_env(&tmp);
    let output_path = tmp.path().join("manifest.json");

    // Create files that should be skipped
    fs::write(tmp.path().join("artifacts/notes.md"), b"# Notes").unwrap();
    fs::write(tmp.path().join("artifacts/manifest.json"), b"{}").unwrap();
    fs::write(tmp.path().join("artifacts/install.sh"), b"#!/bin/bash").unwrap();
    fs::write(tmp.path().join("artifacts/readme.txt"), b"readme").unwrap();
    fs::write(tmp.path().join("artifacts/script.py"), b"print('hi')").unwrap();
    // One real binary
    fs::write(
        tmp.path().join("artifacts/tempo-wallet-x86_64-linux"),
        b"binary",
    )
    .unwrap();

    let output = tempo_sign()
        .args([
            "sign",
            "--key-file",
            &key_path,
            "--artifacts-dir",
            &artifacts_dir,
            "--version",
            "1.0.0",
            "--output",
            output_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&output_path).unwrap()).unwrap();
    let binaries = manifest["binaries"].as_object().unwrap();
    assert_eq!(
        binaries.len(),
        1,
        "should only include the binary, not .md/.json/.sh/.txt/.py: {binaries:?}"
    );
    assert!(binaries.contains_key("tempo-wallet-x86_64-linux"));
}

#[test]
fn sign_empty_artifacts_produces_empty_binaries() {
    let tmp = tempfile::tempdir().unwrap();
    let (key_path, artifacts_dir) = setup_signing_env(&tmp);
    let output_path = tmp.path().join("manifest.json");

    let output = tempo_sign()
        .args([
            "sign",
            "--key-file",
            &key_path,
            "--artifacts-dir",
            &artifacts_dir,
            "--version",
            "1.0.0",
            "--output",
            output_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&output_path).unwrap()).unwrap();
    let binaries = manifest["binaries"].as_object().unwrap();
    assert!(binaries.is_empty(), "empty artifacts → empty binaries");
}

#[test]
fn sign_custom_base_url() {
    let tmp = tempfile::tempdir().unwrap();
    let (key_path, artifacts_dir) = setup_signing_env(&tmp);
    let output_path = tmp.path().join("manifest.json");

    fs::write(tmp.path().join("artifacts/binary"), b"content").unwrap();

    let output = tempo_sign()
        .args([
            "sign",
            "--key-file",
            &key_path,
            "--artifacts-dir",
            &artifacts_dir,
            "--version",
            "1.0.0",
            "--base-url",
            "https://cdn.example.com/releases",
            "--output",
            output_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&output_path).unwrap()).unwrap();
    let url = manifest["binaries"]["binary"]["url"].as_str().unwrap();
    assert!(
        url.starts_with("https://cdn.example.com/releases/v1.0.0/"),
        "should use custom base URL: {url}"
    );
}

// ── Optional metadata fields ────────────────────────────────────────────

#[test]
fn sign_with_description() {
    let tmp = tempfile::tempdir().unwrap();
    let (key_path, artifacts_dir) = setup_signing_env(&tmp);
    let output_path = tmp.path().join("manifest.json");

    fs::write(tmp.path().join("artifacts/binary"), b"content").unwrap();

    let output = tempo_sign()
        .args([
            "sign",
            "--key-file",
            &key_path,
            "--artifacts-dir",
            &artifacts_dir,
            "--version",
            "1.0.0",
            "--description",
            "Manage your Tempo Wallet",
            "--output",
            output_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&output_path).unwrap()).unwrap();
    assert_eq!(manifest["description"], "Manage your Tempo Wallet");
}

#[test]
fn sign_with_skill_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let (key_path, artifacts_dir) = setup_signing_env(&tmp);
    let output_path = tmp.path().join("manifest.json");

    fs::write(tmp.path().join("artifacts/binary"), b"content").unwrap();

    let output = tempo_sign()
        .args([
            "sign",
            "--key-file",
            &key_path,
            "--artifacts-dir",
            &artifacts_dir,
            "--version",
            "1.0.0",
            "--skill",
            "https://cli.tempo.xyz/extensions/tempo-wallet/v1.0.0/SKILL.md",
            "--skill-sha256",
            "abc123def456",
            "--output",
            output_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&output_path).unwrap()).unwrap();
    assert_eq!(
        manifest["skill"],
        "https://cli.tempo.xyz/extensions/tempo-wallet/v1.0.0/SKILL.md"
    );
    assert_eq!(manifest["skill_sha256"], "abc123def456");
}

#[test]
fn sign_with_skill_file_adds_signature() {
    let tmp = tempfile::tempdir().unwrap();
    let (key_path, artifacts_dir) = setup_signing_env(&tmp);
    let output_path = tmp.path().join("manifest.json");
    let skill_path = tmp.path().join("SKILL.md");

    fs::write(tmp.path().join("artifacts/binary"), b"content").unwrap();
    fs::write(&skill_path, b"# Skill\nSome skill content").unwrap();

    let output = tempo_sign()
        .args([
            "sign",
            "--key-file",
            &key_path,
            "--artifacts-dir",
            &artifacts_dir,
            "--version",
            "1.0.0",
            "--skill-file",
            skill_path.to_str().unwrap(),
            "--output",
            output_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&output_path).unwrap()).unwrap();
    assert!(
        manifest["skill_signature"].is_string(),
        "should have skill_signature: {manifest}"
    );
    let sig = manifest["skill_signature"].as_str().unwrap();
    assert!(
        sig.contains("untrusted comment:"),
        "skill_signature should be a minisign signature: {sig}"
    );
}

// ── Default output path ─────────────────────────────────────────────────

#[test]
fn sign_default_output_is_manifest_json() {
    let tmp = tempfile::tempdir().unwrap();
    let (key_path, artifacts_dir) = setup_signing_env(&tmp);

    fs::write(tmp.path().join("artifacts/binary"), b"content").unwrap();

    let output = tempo_sign()
        .current_dir(tmp.path())
        .args([
            "sign",
            "--key-file",
            &key_path,
            "--artifacts-dir",
            &artifacts_dir,
            "--version",
            "1.0.0",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(
        tmp.path().join("manifest.json").exists(),
        "should write manifest.json in cwd by default"
    );
}

// ── SHA256 correctness ──────────────────────────────────────────────────

#[test]
fn sign_sha256_matches_file_content() {
    use sha2::{Digest, Sha256};

    let tmp = tempfile::tempdir().unwrap();
    let (key_path, artifacts_dir) = setup_signing_env(&tmp);
    let output_path = tmp.path().join("manifest.json");

    let content = b"known content for hash verification";
    fs::write(tmp.path().join("artifacts/hashtest"), content).unwrap();

    let output = tempo_sign()
        .args([
            "sign",
            "--key-file",
            &key_path,
            "--artifacts-dir",
            &artifacts_dir,
            "--version",
            "1.0.0",
            "--output",
            output_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&output_path).unwrap()).unwrap();

    let expected_hash = format!("{:x}", Sha256::digest(content));
    let actual_hash = manifest["binaries"]["hashtest"]["sha256"].as_str().unwrap();
    assert_eq!(actual_hash, expected_hash, "SHA256 mismatch");
}
