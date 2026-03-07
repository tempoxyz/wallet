#![cfg(unix)]

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const TEST_SIGNING_KEY_SEED: [u8; 32] = [7u8; 32];

fn tempo_bin() -> String {
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_tempo") {
        return path;
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let target_root = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            // In a workspace, target is at the workspace root, not the crate dir
            let workspace_root = manifest_dir
                .parent()
                .and_then(|p| p.parent())
                .unwrap_or(&manifest_dir);
            workspace_root.join("target")
        });
    target_root
        .join("debug")
        .join("tempo")
        .display()
        .to_string()
}

fn write_exec_script(path: &Path, body: &str) {
    fs::write(path, format!("#!/usr/bin/env bash\nset -e\n{body}\n")).expect("write script");
    let mut perms = fs::metadata(path).expect("metadata").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).expect("chmod");
}

fn setup_source_dir(tmp: &TempDir, with_core: bool, with_wallet: bool) -> PathBuf {
    let source_dir = tmp.path().join("source");
    fs::create_dir_all(&source_dir).expect("mkdir source");

    fs::copy(tempo_bin(), source_dir.join("tempo")).expect("copy tempo");

    if with_core {
        write_exec_script(&source_dir.join("tempo-core"), r#"echo "core:$*""#);
    }

    if with_wallet {
        write_exec_script(&source_dir.join("tempo-wallet"), r#"echo "wallet:$*""#);
    }

    source_dir
}

fn sha256_file(path: &Path) -> String {
    let bytes = fs::read(path).expect("read file for sha");
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn sha256_str(data: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn platform_binary_suffix() -> &'static str {
    if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        "darwin-arm64"
    } else if cfg!(target_os = "macos") && cfg!(target_arch = "x86_64") {
        "darwin-amd64"
    } else if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
        "linux-amd64"
    } else if cfg!(target_os = "linux") && cfg!(target_arch = "aarch64") {
        "linux-arm64"
    } else {
        "unknown-unknown"
    }
}

fn test_signing_key() -> SigningKey {
    SigningKey::from_bytes(&TEST_SIGNING_KEY_SEED)
}

fn public_key(signing_key: &SigningKey) -> String {
    BASE64_STANDARD.encode(signing_key.verifying_key().as_bytes())
}

/// Write a signed release manifest for a single extension binary.
fn write_extension_manifest(
    tmp: &TempDir,
    extension: &str,
    binary_path: &Path,
    signing_key: &SigningKey,
) -> PathBuf {
    write_extension_manifest_with_skill(tmp, extension, binary_path, signing_key, None, None)
}

/// Write a signed release manifest for a single extension binary, with optional skill.
fn write_extension_manifest_with_skill(
    tmp: &TempDir,
    extension: &str,
    binary_path: &Path,
    signing_key: &SigningKey,
    skill_url: Option<&str>,
    skill_sha256: Option<&str>,
) -> PathBuf {
    let bytes = fs::read(binary_path).expect("read binary for manifest");
    let checksum = sha256_file(binary_path);
    let signature = BASE64_STANDARD.encode(signing_key.sign(&bytes).to_bytes());
    let platform_key = format!("tempo-{extension}-{}", platform_binary_suffix());

    let mut skill_fields = String::new();
    if let Some(url) = skill_url {
        skill_fields.push_str(&format!(
            r#",
  "skill": "{url}""#
        ));
    }
    if let Some(sha) = skill_sha256 {
        skill_fields.push_str(&format!(
            r#",
  "skill_sha256": "{sha}""#
        ));
    }

    let manifest = format!(
        r#"{{
  "version": "test",
  "binaries": {{
    "{platform_key}": {{ "url": "file://{}", "sha256": "{checksum}", "signature": "{signature}" }}
  }}{skill_fields}
}}"#,
        binary_path.display(),
    );
    let manifest_path = tmp.path().join(format!("{extension}-manifest.json"));
    fs::write(&manifest_path, manifest).expect("write release manifest");
    manifest_path
}

/// Write a manifest with a custom wallet checksum (for testing checksum mismatch).
fn write_manifest_with_signer(
    tmp: &TempDir,
    source_dir: &Path,
    wallet_checksum: &str,
    signing_key: &SigningKey,
) -> PathBuf {
    let wallet_path = source_dir.join("tempo-wallet");
    let wallet_bytes = fs::read(&wallet_path).expect("read wallet for signature");

    let wallet_signature = BASE64_STANDARD.encode(signing_key.sign(&wallet_bytes).to_bytes());

    let platform_key = format!("tempo-wallet-{}", platform_binary_suffix());

    let manifest = format!(
        r#"{{
  "version": "test",
  "binaries": {{
    "{platform_key}": {{ "url": "file://{}", "sha256": "{}", "signature": "{}" }}
  }}
}}"#,
        wallet_path.display(),
        wallet_checksum,
        wallet_signature,
    );
    let manifest_path = tmp.path().join("manifest.json");
    fs::write(&manifest_path, manifest).expect("write release manifest");
    manifest_path
}

fn install_legacy_tempoup_node(bin_dir: &Path) {
    fs::create_dir_all(bin_dir).expect("mkdir bin");
    write_exec_script(&bin_dir.join("tempo"), r#"echo "legacy-node:$*""#);
}

/// Install extension via signed manifest.
fn add_extension(tmp: &TempDir, home: &Path, bin_dir: &Path, extension: &str, binary_path: &Path) {
    let signing_key = test_signing_key();
    let manifest_path = write_extension_manifest(tmp, extension, binary_path, &signing_key);
    let pk = public_key(&signing_key);
    assert!(
        TempoRun::new(home, bin_dir)
            .run(&[
                "add",
                extension,
                "--release-manifest",
                manifest_path.to_str().unwrap(),
                "--release-public-key",
                &pk,
            ])
            .status
            .success(),
        "failed to add {extension}"
    );
}

fn add_core_and_wallet(tmp: &TempDir, home: &Path, bin_dir: &Path, source_dir: &Path) {
    install_tempo_binary(bin_dir);
    add_extension(tmp, home, bin_dir, "core", &source_dir.join("tempo-core"));
    add_extension(
        tmp,
        home,
        bin_dir,
        "wallet",
        &source_dir.join("tempo-wallet"),
    );
}

fn install_tempo_binary(bin_dir: &Path) {
    fs::create_dir_all(bin_dir).expect("mkdir bin");
    fs::copy(tempo_bin(), bin_dir.join("tempo")).expect("copy tempo");
}

struct TempoRun<'a> {
    home: &'a Path,
    bin_dir: &'a Path,
    use_installed: bool,
    path: Option<&'a str>,
    debug: bool,
}

impl<'a> TempoRun<'a> {
    fn new(home: &'a Path, bin_dir: &'a Path) -> Self {
        Self {
            home,
            bin_dir,
            use_installed: false,
            path: None,
            debug: false,
        }
    }

    /// Run the installed `bin_dir/tempo` instead of the cargo-built binary.
    fn installed(mut self) -> Self {
        self.use_installed = true;
        self
    }

    fn path(mut self, path: &'a str) -> Self {
        self.path = Some(path);
        self
    }

    fn debug(mut self) -> Self {
        self.debug = true;
        self
    }

    fn run(self, args: &[&str]) -> Output {
        let bin = if self.use_installed {
            self.bin_dir.join("tempo").display().to_string()
        } else {
            tempo_bin()
        };

        let mut cmd = Command::new(bin);
        cmd.env("TEMPO_HOME", self.home);
        cmd.env("HOME", self.home);

        if let Some(p) = self.path {
            cmd.env("PATH", p);
        }
        if self.debug {
            cmd.env("TEMPO_DEBUG", "1");
        }

        cmd.args(args).output().expect("run tempo")
    }
}

#[test]
fn existing_tempoup_node_user_adds_wallet_without_implicit_core_migration() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().join("home");
    let bin_dir = home.join("bin");
    let source_dir = setup_source_dir(&tmp, false, true);
    install_legacy_tempoup_node(&bin_dir);

    add_extension(
        &tmp,
        &home,
        &bin_dir,
        "wallet",
        &source_dir.join("tempo-wallet"),
    );

    assert!(bin_dir.join("tempo-wallet").exists());
    assert!(!bin_dir.join("tempo-core").exists());

    let legacy_node = TempoRun::new(&home, &bin_dir)
        .installed()
        .run(&["node", "--chain", "testnet"]);
    assert!(legacy_node.status.success());
    assert!(String::from_utf8(legacy_node.stdout)
        .unwrap()
        .contains("legacy-node:node --chain testnet"));

    install_tempo_binary(&bin_dir);

    let node = TempoRun::new(&home, &bin_dir)
        .installed()
        .run(&["node", "--chain", "testnet"]);
    assert!(!node.status.success());
    assert!(String::from_utf8(node.stderr)
        .unwrap()
        .contains("Run: tempoup"));

    let wallet = TempoRun::new(&home, &bin_dir)
        .installed()
        .run(&["wallet", "services"]);
    assert!(wallet.status.success());
    assert!(String::from_utf8(wallet.stdout)
        .unwrap()
        .contains("wallet:services"));
}

#[test]
fn does_not_treat_legacy_tempoup_node_as_core_after_wallet_install() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().join("home");
    let bin_dir = home.join("bin");
    let source_dir = setup_source_dir(&tmp, false, true);

    install_legacy_tempoup_node(&bin_dir);
    add_extension(
        &tmp,
        &home,
        &bin_dir,
        "wallet",
        &source_dir.join("tempo-wallet"),
    );

    install_tempo_binary(&bin_dir);

    let legacy_init = TempoRun::new(&home, &bin_dir)
        .installed()
        .run(&["init", "--chain", "testnet"]);
    assert!(!legacy_init.status.success());
    assert!(String::from_utf8(legacy_init.stderr)
        .unwrap()
        .contains("Run: tempoup"));

    let wallet = TempoRun::new(&home, &bin_dir)
        .installed()
        .run(&["wallet", "https://api.example.com"]);
    assert!(wallet.status.success());
}

#[test]
fn new_node_user_via_tempo_then_adds_wallet_via_tempo() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().join("home");
    let bin_dir = home.join("bin");
    let source_dir = setup_source_dir(&tmp, true, true);

    add_extension(
        &tmp,
        &home,
        &bin_dir,
        "core",
        &source_dir.join("tempo-core"),
    );
    add_extension(
        &tmp,
        &home,
        &bin_dir,
        "wallet",
        &source_dir.join("tempo-wallet"),
    );

    install_tempo_binary(&bin_dir);

    assert!(TempoRun::new(&home, &bin_dir)
        .installed()
        .run(&["db", "stats"])
        .status
        .success());
    assert!(TempoRun::new(&home, &bin_dir)
        .installed()
        .run(&["wallet", "services"])
        .status
        .success());
}

#[test]
fn new_wallet_user_via_tempo_then_adds_node_via_tempo() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().join("home");
    let bin_dir = home.join("bin");
    let source_dir = setup_source_dir(&tmp, true, true);

    add_extension(
        &tmp,
        &home,
        &bin_dir,
        "wallet",
        &source_dir.join("tempo-wallet"),
    );

    install_tempo_binary(&bin_dir);

    assert!(TempoRun::new(&home, &bin_dir)
        .installed()
        .run(&["wallet", "services"])
        .status
        .success());

    let missing_node = TempoRun::new(&home, &bin_dir).installed().run(&["node"]);
    assert!(!missing_node.status.success());
    assert!(String::from_utf8(missing_node.stderr)
        .unwrap()
        .contains("tempoup"));

    add_extension(
        &tmp,
        &home,
        &bin_dir,
        "core",
        &source_dir.join("tempo-core"),
    );
    assert!(TempoRun::new(&home, &bin_dir)
        .installed()
        .run(&["node", "--chain", "testnet"])
        .status
        .success());
}

#[test]
fn new_user_adds_core_and_wallet() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().join("home");
    let bin_dir = home.join("bin");
    let source_dir = setup_source_dir(&tmp, true, true);

    add_core_and_wallet(&tmp, &home, &bin_dir, &source_dir);
    assert!(bin_dir.join("tempo").exists());
    assert!(bin_dir.join("tempo-core").exists());
    assert!(bin_dir.join("tempo-wallet").exists());
}

#[test]
fn update_wallet_via_tempo() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().join("home");
    let bin_dir = home.join("bin");
    let source_dir = setup_source_dir(&tmp, true, true);

    add_core_and_wallet(&tmp, &home, &bin_dir, &source_dir);

    let signing_key = test_signing_key();
    let manifest_path = write_extension_manifest(
        &tmp,
        "wallet",
        &source_dir.join("tempo-wallet"),
        &signing_key,
    );
    assert!(TempoRun::new(&home, &bin_dir)
        .run(&[
            "update",
            "wallet",
            "--release-manifest",
            manifest_path.to_str().unwrap(),
            "--release-public-key",
            &public_key(&signing_key),
        ])
        .status
        .success());
    assert!(TempoRun::new(&home, &bin_dir)
        .installed()
        .run(&["wallet", "services"])
        .status
        .success());
}

#[test]
fn update_core_via_tempo() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().join("home");
    let bin_dir = home.join("bin");
    let source_dir = setup_source_dir(&tmp, true, true);

    add_core_and_wallet(&tmp, &home, &bin_dir, &source_dir);

    let signing_key = test_signing_key();
    let manifest_path =
        write_extension_manifest(&tmp, "core", &source_dir.join("tempo-core"), &signing_key);
    assert!(TempoRun::new(&home, &bin_dir)
        .run(&[
            "update",
            "core",
            "--release-manifest",
            manifest_path.to_str().unwrap(),
            "--release-public-key",
            &public_key(&signing_key),
        ])
        .status
        .success());
    assert!(TempoRun::new(&home, &bin_dir)
        .installed()
        .run(&["node", "--chain", "testnet"])
        .status
        .success());
}

#[test]
fn remove_wallet_keeps_core_working_via_tempo() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().join("home");
    let bin_dir = home.join("bin");
    let source_dir = setup_source_dir(&tmp, true, true);

    add_core_and_wallet(&tmp, &home, &bin_dir, &source_dir);
    assert!(TempoRun::new(&home, &bin_dir)
        .run(&["remove", "wallet"])
        .status
        .success());

    assert!(TempoRun::new(&home, &bin_dir)
        .installed()
        .run(&["db", "stats"])
        .status
        .success());
    let wallet = TempoRun::new(&home, &bin_dir)
        .installed()
        .run(&["wallet", "services"]);
    let out = String::from_utf8(wallet.stdout).unwrap();
    assert!(
        !out.contains("wallet:services"),
        "wallet extension should no longer be invoked"
    );
}

#[test]
fn remove_core_keeps_wallet_working_via_tempo() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().join("home");
    let bin_dir = home.join("bin");
    let source_dir = setup_source_dir(&tmp, true, true);

    add_core_and_wallet(&tmp, &home, &bin_dir, &source_dir);
    assert!(TempoRun::new(&home, &bin_dir)
        .run(&["remove", "core"])
        .status
        .success());

    assert!(TempoRun::new(&home, &bin_dir)
        .installed()
        .run(&["wallet", "services"])
        .status
        .success());
    let node = TempoRun::new(&home, &bin_dir).installed().run(&["node"]);
    assert!(!node.status.success());
}

#[test]
fn idempotent_add_for_same_extension() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().join("home");
    let bin_dir = home.join("bin");
    let source_dir = setup_source_dir(&tmp, true, true);

    add_extension(
        &tmp,
        &home,
        &bin_dir,
        "wallet",
        &source_dir.join("tempo-wallet"),
    );
    add_extension(
        &tmp,
        &home,
        &bin_dir,
        "wallet",
        &source_dir.join("tempo-wallet"),
    );
    add_extension(
        &tmp,
        &home,
        &bin_dir,
        "core",
        &source_dir.join("tempo-core"),
    );
    add_extension(
        &tmp,
        &home,
        &bin_dir,
        "core",
        &source_dir.join("tempo-core"),
    );
}

#[test]
fn management_parsing_allows_flag_before_extension() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().join("home");
    let bin_dir = home.join("bin");
    let source_dir = setup_source_dir(&tmp, true, true);

    let signing_key = test_signing_key();
    let manifest_path = write_extension_manifest(
        &tmp,
        "wallet",
        &source_dir.join("tempo-wallet"),
        &signing_key,
    );

    let dry_run = TempoRun::new(&home, &bin_dir).run(&[
        "add",
        "--dry-run",
        "wallet",
        "--release-manifest",
        manifest_path.to_str().unwrap(),
        "--release-public-key",
        &public_key(&signing_key),
    ]);

    assert!(dry_run.status.success());
}

#[test]
fn remove_then_readd_repairs_state() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().join("home");
    let bin_dir = home.join("bin");
    let source_dir = setup_source_dir(&tmp, true, true);

    add_core_and_wallet(&tmp, &home, &bin_dir, &source_dir);
    fs::remove_file(bin_dir.join("tempo-wallet")).expect("remove wallet to simulate drift");

    add_extension(
        &tmp,
        &home,
        &bin_dir,
        "wallet",
        &source_dir.join("tempo-wallet"),
    );
    assert!(TempoRun::new(&home, &bin_dir)
        .installed()
        .run(&["wallet", "services"])
        .status
        .success());
}

#[test]
fn manifest_verify_failure_keeps_legacy_tempoup_node_state_unchanged() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().join("home");
    let bin_dir = home.join("bin");
    install_legacy_tempoup_node(&bin_dir);

    // Manifest intentionally sets wrong wallet checksum to force verify failure.
    let source_dir = setup_source_dir(&tmp, false, true);
    let signing_key = test_signing_key();
    let manifest_path = write_manifest_with_signer(&tmp, &source_dir, "deadbeef", &signing_key);
    let install = TempoRun::new(&home, &bin_dir).run(&[
        "add",
        "wallet",
        "--release-manifest",
        manifest_path.to_str().unwrap(),
        "--release-public-key",
        &public_key(&signing_key),
    ]);
    assert!(!install.status.success());
    let stderr = String::from_utf8(install.stderr).unwrap();
    assert!(stderr.contains("checksum mismatch"));

    // Legacy state should remain usable with no partial state changes.
    assert!(bin_dir.join("tempo").exists());
    assert!(!bin_dir.join("tempo-core").exists());
    assert!(!bin_dir.join("tempo-wallet").exists());

    let legacy = TempoRun::new(&home, &bin_dir).installed().run(&["node"]);
    assert!(legacy.status.success());
    assert!(String::from_utf8(legacy.stdout)
        .unwrap()
        .contains("legacy-node:node"));
}

#[test]
fn manifest_based_add_with_checksum_verification_succeeds() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().join("home");
    let bin_dir = home.join("bin");
    let source_dir = setup_source_dir(&tmp, false, true);
    let signing_key = test_signing_key();
    let manifest_path = write_extension_manifest(
        &tmp,
        "wallet",
        &source_dir.join("tempo-wallet"),
        &signing_key,
    );

    let install = TempoRun::new(&home, &bin_dir).run(&[
        "add",
        "wallet",
        "--release-manifest",
        manifest_path.to_str().unwrap(),
        "--release-public-key",
        &public_key(&signing_key),
    ]);
    assert!(install.status.success());

    install_tempo_binary(&bin_dir);

    assert!(TempoRun::new(&home, &bin_dir)
        .installed()
        .run(&["wallet", "services"])
        .status
        .success());
}

#[test]
fn manifest_install_requires_valid_signature() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().join("home");
    let bin_dir = home.join("bin");
    let source_dir = setup_source_dir(&tmp, false, true);

    let good_signer = test_signing_key();
    let bad_signer = SigningKey::from_bytes(&[9u8; 32]);
    let manifest_path = write_manifest_with_signer(
        &tmp,
        &source_dir,
        &sha256_file(&source_dir.join("tempo-wallet")),
        &bad_signer,
    );

    let install = TempoRun::new(&home, &bin_dir).run(&[
        "add",
        "wallet",
        "--release-manifest",
        manifest_path.to_str().unwrap(),
        "--release-public-key",
        &public_key(&good_signer),
    ]);

    assert!(!install.status.success());
    let stderr = String::from_utf8(install.stderr).unwrap();
    assert!(stderr.contains("signature verification failed"));
}

#[test]
fn dry_run_manifest_path_does_not_write_files() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().join("home");
    let bin_dir = home.join("bin");
    let source_dir = setup_source_dir(&tmp, true, true);
    let signing_key = test_signing_key();

    let manifest_path = write_extension_manifest(
        &tmp,
        "wallet",
        &source_dir.join("tempo-wallet"),
        &signing_key,
    );

    let install = TempoRun::new(&home, &bin_dir).run(&[
        "add",
        "wallet",
        "--release-manifest",
        manifest_path.to_str().unwrap(),
        "--release-public-key",
        &public_key(&signing_key),
        "--dry-run",
    ]);

    assert!(install.status.success());
    assert!(!bin_dir.join("tempo").exists());
    assert!(!bin_dir.join("tempo-wallet").exists());
}

#[test]
fn dry_run_manifest_with_legacy_node_does_not_mutate_state() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().join("home");
    let bin_dir = home.join("bin");
    let source_dir = setup_source_dir(&tmp, false, true);
    install_legacy_tempoup_node(&bin_dir);

    let signing_key = test_signing_key();
    let manifest_path = write_extension_manifest(
        &tmp,
        "wallet",
        &source_dir.join("tempo-wallet"),
        &signing_key,
    );

    let install = TempoRun::new(&home, &bin_dir).run(&[
        "add",
        "wallet",
        "--release-manifest",
        manifest_path.to_str().unwrap(),
        "--release-public-key",
        &public_key(&signing_key),
        "--dry-run",
    ]);

    assert!(install.status.success());
    assert!(bin_dir.join("tempo").exists());
    assert!(!bin_dir.join("tempo-core").exists());
    assert!(!bin_dir.join("tempo-wallet").exists());
}

#[test]
fn manual_manifest_install_rejects_insecure_http_manifest() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().join("home");
    let bin_dir = home.join("bin");

    let install = TempoRun::new(&home, &bin_dir).run(&[
        "add",
        "wallet",
        "--release-manifest",
        "http://insecure.example.com/manifest.json",
        "--release-public-key",
        "ZmFrZS1rZXk=",
    ]);

    assert!(!install.status.success());
    let stderr = String::from_utf8(install.stderr).unwrap();
    assert!(stderr.contains("insecure release manifest URL"));
}

#[test]
#[ignore] // flaky in CI: tempoup binary-swap race
fn core_command_auto_installs_via_tempoup_when_available() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().join("home");
    let bin_dir = home.join("bin");
    let source_dir = setup_source_dir(&tmp, true, false);
    let tool_bin = tmp.path().join("tools");
    fs::create_dir_all(&tool_bin).expect("mkdir tools");

    let tempoup_body = format!(
        "cp \"{}\" \"$TEMPO_BIN_DIR/tempo\"",
        source_dir.join("tempo-core").display()
    );
    write_exec_script(&tool_bin.join("tempoup"), &tempoup_body);

    install_tempo_binary(&bin_dir);

    let path = format!("{}:/usr/bin:/bin", tool_bin.display());
    let node = TempoRun::new(&home, &bin_dir)
        .installed()
        .path(&path)
        .run(&["node", "--chain", "testnet"]);
    assert!(node.status.success());
    assert!(String::from_utf8(node.stdout)
        .unwrap()
        .contains("core:node --chain testnet"));
    assert!(String::from_utf8(node.stderr)
        .unwrap()
        .contains("tempo restored and core moved"));

    assert!(bin_dir.join("tempo-core").exists());
    let version = TempoRun::new(&home, &bin_dir)
        .installed()
        .path(&path)
        .run(&["--version"]);
    assert!(version.status.success());

    let consensus = TempoRun::new(&home, &bin_dir)
        .installed()
        .path(&path)
        .run(&["consensus", "status"]);
    assert!(consensus.status.success());
    assert!(String::from_utf8(consensus.stdout)
        .unwrap()
        .contains("core:consensus status"));
}

#[test]
#[ignore] // flaky in CI: tempoup binary-swap race
fn debug_logs_for_extension_and_core_paths() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().join("home");
    let bin_dir = home.join("bin");
    let source_dir = setup_source_dir(&tmp, true, true);
    let tool_bin = tmp.path().join("tools");
    fs::create_dir_all(&tool_bin).expect("mkdir tools");

    let tempoup_body = format!(
        "cp \"{}\" \"$TEMPO_BIN_DIR/tempo\"",
        source_dir.join("tempo-core").display()
    );
    write_exec_script(&tool_bin.join("tempoup"), &tempoup_body);
    install_tempo_binary(&bin_dir);

    add_extension(
        &tmp,
        &home,
        &bin_dir,
        "wallet",
        &source_dir.join("tempo-wallet"),
    );

    let path = format!("{}:/usr/bin:/bin", tool_bin.display());

    let wallet = TempoRun::new(&home, &bin_dir)
        .installed()
        .path(&path)
        .debug()
        .run(&["wallet", "services"]);
    assert!(wallet.status.success());
    let wallet_stderr = String::from_utf8(wallet.stderr).unwrap();
    assert!(wallet_stderr.contains("debug: extension=wallet"));
    assert!(wallet_stderr.contains("debug: extension found locally"));

    let node = TempoRun::new(&home, &bin_dir)
        .installed()
        .path(&path)
        .debug()
        .run(&["node", "--chain", "testnet"]);
    assert!(node.status.success());
    let node_stderr = String::from_utf8(node.stderr).unwrap();
    assert!(node_stderr.contains("debug: extension=node"));
    assert!(node_stderr.contains("debug: classified as core subcommand"));
    assert!(node_stderr.contains("debug: tempo-core missing, attempting tempoup auto-install"));
}

#[test]
fn core_command_without_tempoup_returns_install_hint() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().join("home");
    let bin_dir = home.join("bin");

    install_tempo_binary(&bin_dir);

    let node = TempoRun::new(&home, &bin_dir)
        .installed()
        .path("/usr/bin:/bin")
        .run(&["node"]);
    assert!(!node.status.success());
    let stderr = String::from_utf8(node.stderr).unwrap();
    assert!(stderr.contains("Run: tempoup"));
}

#[test]
fn core_command_when_tempoup_fails_returns_install_hint() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().join("home");
    let bin_dir = home.join("bin");
    let tool_bin = tmp.path().join("tools");
    fs::create_dir_all(&tool_bin).expect("mkdir tools");

    write_exec_script(&tool_bin.join("tempoup"), "exit 42");
    install_tempo_binary(&bin_dir);

    let path = format!("{}:/usr/bin:/bin", tool_bin.display());
    let node = TempoRun::new(&home, &bin_dir)
        .installed()
        .path(&path)
        .run(&["node"]);
    assert!(!node.status.success());
    let stderr = String::from_utf8(node.stderr).unwrap();
    assert!(stderr.contains("Run: tempoup"));
}

#[test]
fn add_arbitrary_extension_via_manifest() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().join("home");
    let bin_dir = home.join("bin");
    let source_dir = tmp.path().join("source");
    fs::create_dir_all(&source_dir).expect("mkdir source");

    write_exec_script(&source_dir.join("tempo-bridge"), r#"echo "bridge:$*""#);

    add_extension(
        &tmp,
        &home,
        &bin_dir,
        "bridge",
        &source_dir.join("tempo-bridge"),
    );
    assert!(bin_dir.join("tempo-bridge").exists());

    install_tempo_binary(&bin_dir);

    let bridge = TempoRun::new(&home, &bin_dir)
        .installed()
        .run(&["bridge", "status"]);
    assert!(bridge.status.success());
    assert!(String::from_utf8(bridge.stdout)
        .unwrap()
        .contains("bridge:status"));
}

#[test]
fn no_args_no_core_prints_help() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().join("home");
    let bin_dir = home.join("bin");
    install_tempo_binary(&bin_dir);

    let out = TempoRun::new(&home, &bin_dir).installed().run(&[]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("Tempo CLI"));
    assert!(stdout.contains("Usage:"));
}

#[test]
fn extra_positional_in_management_args_is_rejected() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().join("home");
    let bin_dir = home.join("bin");

    let out = TempoRun::new(&home, &bin_dir).run(&["add", "core", "0.1.0", "extra"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("unexpected positional argument"));
}

// ---------------------------------------------------------------------------
// Skill install / remove tests
// ---------------------------------------------------------------------------

#[test]
fn manifest_with_skill_installs_skill_to_agent_dirs() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().join("home");
    let bin_dir = home.join("bin");
    let source_dir = setup_source_dir(&tmp, false, true);

    // Create agent parent dirs so skills get installed there.
    let claude_dir = home.join(".claude");
    let agents_dir = home.join(".agents");
    fs::create_dir_all(&claude_dir).expect("mkdir .claude");
    fs::create_dir_all(&agents_dir).expect("mkdir .agents");

    // Write a skill file and manifest referencing it.
    let skill_content = "# Test Skill\nThis is a test skill.";
    let skill_path = tmp.path().join("SKILL.md");
    fs::write(&skill_path, skill_content).expect("write skill");
    let skill_url = format!("file://{}", skill_path.display());
    let skill_sha256 = sha256_str(skill_content);

    let signing_key = test_signing_key();
    let manifest_path = write_extension_manifest_with_skill(
        &tmp,
        "wallet",
        &source_dir.join("tempo-wallet"),
        &signing_key,
        Some(&skill_url),
        Some(&skill_sha256),
    );

    let install = TempoRun::new(&home, &bin_dir).run(&[
        "add",
        "wallet",
        "--release-manifest",
        manifest_path.to_str().unwrap(),
        "--release-public-key",
        &public_key(&signing_key),
    ]);
    assert!(install.status.success());

    // Verify skill was installed to both agent dirs.
    let claude_skill = claude_dir.join("skills/tempo-wallet/SKILL.md");
    let agents_skill = agents_dir.join("skills/tempo-wallet/SKILL.md");
    assert!(claude_skill.exists(), "skill not installed to .claude");
    assert!(agents_skill.exists(), "skill not installed to .agents");
    assert_eq!(fs::read_to_string(&claude_skill).unwrap(), skill_content);

    let stdout = String::from_utf8(install.stdout).unwrap();
    assert!(stdout.contains("installed tempo-wallet skill to 2 agent(s)"));
}

#[test]
fn manifest_with_skill_rejects_bad_checksum() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().join("home");
    let bin_dir = home.join("bin");
    let source_dir = setup_source_dir(&tmp, false, true);

    let claude_dir = home.join(".claude");
    fs::create_dir_all(&claude_dir).expect("mkdir .claude");

    let skill_content = "# Test Skill";
    let skill_path = tmp.path().join("SKILL.md");
    fs::write(&skill_path, skill_content).expect("write skill");
    let skill_url = format!("file://{}", skill_path.display());

    let signing_key = test_signing_key();
    let manifest_path = write_extension_manifest_with_skill(
        &tmp,
        "wallet",
        &source_dir.join("tempo-wallet"),
        &signing_key,
        Some(&skill_url),
        Some("deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef"),
    );

    let install = TempoRun::new(&home, &bin_dir).run(&[
        "add",
        "wallet",
        "--release-manifest",
        manifest_path.to_str().unwrap(),
        "--release-public-key",
        &public_key(&signing_key),
    ]);
    // Binary install succeeds, skill is skipped due to checksum mismatch.
    assert!(install.status.success());

    let claude_skill = claude_dir.join("skills/tempo-wallet/SKILL.md");
    assert!(
        !claude_skill.exists(),
        "skill should not be installed with bad checksum"
    );

    let stderr = String::from_utf8(install.stderr).unwrap();
    assert!(stderr.contains("skill checksum mismatch"));
}

#[test]
fn remove_extension_removes_skills() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().join("home");
    let bin_dir = home.join("bin");
    let source_dir = setup_source_dir(&tmp, false, true);

    // Set up agent dir and install with skill.
    let claude_dir = home.join(".claude");
    fs::create_dir_all(&claude_dir).expect("mkdir .claude");

    let skill_content = "# Test Skill";
    let skill_path = tmp.path().join("SKILL.md");
    fs::write(&skill_path, skill_content).expect("write skill");
    let skill_url = format!("file://{}", skill_path.display());
    let skill_sha256 = sha256_str(skill_content);

    let signing_key = test_signing_key();
    let manifest_path = write_extension_manifest_with_skill(
        &tmp,
        "wallet",
        &source_dir.join("tempo-wallet"),
        &signing_key,
        Some(&skill_url),
        Some(&skill_sha256),
    );
    assert!(TempoRun::new(&home, &bin_dir)
        .run(&[
            "add",
            "wallet",
            "--release-manifest",
            manifest_path.to_str().unwrap(),
            "--release-public-key",
            &public_key(&signing_key),
        ])
        .status
        .success());

    let claude_skill = claude_dir.join("skills/tempo-wallet/SKILL.md");
    assert!(claude_skill.exists(), "skill should exist after install");

    // Remove the extension and verify skill is cleaned up.
    assert!(TempoRun::new(&home, &bin_dir)
        .run(&["remove", "wallet"])
        .status
        .success());
    assert!(
        !claude_skill.exists(),
        "skill should be removed after remove"
    );
    assert!(
        !claude_dir.join("skills/tempo-wallet").exists(),
        "skill directory should be removed"
    );
}

#[test]
fn skill_not_installed_when_no_agent_dirs_exist() {
    let tmp = TempDir::new().expect("tempdir");
    let home = tmp.path().join("home");
    let bin_dir = home.join("bin");
    let source_dir = setup_source_dir(&tmp, false, true);

    let skill_content = "# Test Skill";
    let skill_path = tmp.path().join("SKILL.md");
    fs::write(&skill_path, skill_content).expect("write skill");
    let skill_url = format!("file://{}", skill_path.display());
    let skill_sha256 = sha256_str(skill_content);

    let signing_key = test_signing_key();
    let manifest_path = write_extension_manifest_with_skill(
        &tmp,
        "wallet",
        &source_dir.join("tempo-wallet"),
        &signing_key,
        Some(&skill_url),
        Some(&skill_sha256),
    );

    let install = TempoRun::new(&home, &bin_dir).run(&[
        "add",
        "wallet",
        "--release-manifest",
        manifest_path.to_str().unwrap(),
        "--release-public-key",
        &public_key(&signing_key),
    ]);
    // Install succeeds — skill just isn't installed (no agent dirs).
    assert!(install.status.success());
    assert!(bin_dir.join("tempo-wallet").exists());

    let stdout = String::from_utf8(install.stdout).unwrap();
    assert!(!stdout.contains("installed tempo-wallet skill"));
}
