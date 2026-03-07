#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

fn tempo_bin() -> String {
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_tempo") {
        return path;
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root");
    workspace_root
        .join("target")
        .join("debug")
        .join("tempo")
        .display()
        .to_string()
}

fn run_tempo(home: &Path, args: &[&str]) -> std::process::Output {
    Command::new(tempo_bin())
        .env("TEMPO_HOME", home)
        .env("HOME", home)
        .args(args)
        .output()
        .expect("run tempo")
}

#[test]
fn prints_help_with_no_args() {
    let tmp = TempDir::new().expect("tempdir");
    let out = run_tempo(tmp.path(), &[]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Tempo CLI"));
    assert!(stdout.contains("Usage:"));
}

#[test]
fn routes_wallet_extension_binary() {
    let tmp = TempDir::new().expect("tempdir");
    let out = run_tempo(tmp.path(), &["wallet", "--help"]);

    assert!(out.status.success());
    let output = String::from_utf8_lossy(&out.stdout);
    assert!(output.contains("login"));
    assert!(output.contains("whoami"));
}

#[test]
fn routes_mpp_extension_binary() {
    let tmp = TempDir::new().expect("tempdir");
    let out = run_tempo(tmp.path(), &["mpp", "--help"]);

    assert!(out.status.success());
    let output = String::from_utf8_lossy(&out.stdout);
    assert!(output.contains("sessions"));
    assert!(output.contains("services"));
}
