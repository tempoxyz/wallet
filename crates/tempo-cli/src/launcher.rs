//! Routes `tempo <extension>` to the right binary, handles auto-install
//! of missing extensions, and provides built-in commands (help, version,
//! add/update/remove).

use crate::installer::{
    binary_candidates, debug_log, executable_name, fetch_manifest_version,
    set_executable_permissions, InstallSource, Installer, InstallerError,
};
use crate::state::State;
use std::env;
use std::error::Error;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const EXTENSIONS_BASE_URL: &str = "https://cli.tempo.xyz/extensions";
const PUBLIC_KEY: &str = "bDpt6MpqpvjiIPBB2NroGZQ/2HrfV+roj2qUa2b+vjI=";

const CORE_BINARY: &str = "tempo-core";
const CORE_SUBCOMMANDS: &[&str] = &[
    "consensus",
    "core",
    "db",
    "init",
    "init-from-binary-dump",
    "node",
];

#[derive(Debug)]
pub(crate) enum LauncherError {
    Io(std::io::Error),
    Installer(InstallerError),
    InvalidArgs(String),
}

impl fmt::Display for LauncherError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "io error: {err}"),
            Self::Installer(err) => write!(f, "installer error: {err}"),
            Self::InvalidArgs(err) => write!(f, "invalid arguments: {err}"),
        }
    }
}

impl Error for LauncherError {}

impl From<std::io::Error> for LauncherError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<InstallerError> for LauncherError {
    fn from(value: InstallerError) -> Self {
        Self::Installer(value)
    }
}

struct ManagementArgs {
    extension: String,
    version: Option<String>,
    source: InstallSource,
    dry_run: bool,
}

pub(crate) struct Launcher {
    exe_path: Option<PathBuf>,
    exe_dir: Option<PathBuf>,
}

impl Launcher {
    pub(crate) fn new() -> Self {
        let exe_path = env::current_exe().ok();
        let exe_dir = exe_path
            .as_deref()
            .and_then(|path| path.parent().map(Path::to_path_buf));
        Self { exe_path, exe_dir }
    }

    pub(crate) fn run(&self, args: Vec<String>) -> Result<i32, LauncherError> {
        let Some(first) = args.get(1).map(String::as_str) else {
            return self.handle_no_args();
        };

        match first {
            "-h" | "--help" | "help" => {
                self.print_help();
                Ok(0)
            }
            "-V" | "--version" | "version" => {
                println!(
                    "tempo {} ({})",
                    env!("CARGO_PKG_VERSION"),
                    env!("TEMPO_GIT_SHA")
                );
                Ok(0)
            }
            "add" | "update" | "remove" => self.handle_management(first, &args[2..]),
            extension => self.handle_extension(extension, &args[2..], &args[1..]),
        }
    }

    fn handle_management(&self, action: &str, args: &[String]) -> Result<i32, LauncherError> {
        let parsed = parse_management_args(args)?;
        let installer = Installer::from_env()?;

        match action {
            "add" | "update" => {
                let source = if parsed.source.manifest.is_none() {
                    InstallSource {
                        manifest: Some(manifest_url(&parsed.extension, parsed.version.as_deref())),
                        public_key: Some(release_public_key()),
                    }
                } else {
                    parsed.source
                };
                installer.install(&parsed.extension, &source, parsed.dry_run, false)?
            }
            "remove" => installer.remove(&parsed.extension, parsed.dry_run)?,
            _ => unreachable!(),
        };

        Ok(0)
    }

    fn handle_no_args(&self) -> Result<i32, LauncherError> {
        if let Some(core) = self.find_binary(CORE_BINARY) {
            return run_child(core, &[], "tempo");
        }

        self.print_help();
        Ok(0)
    }

    fn handle_extension(
        &self,
        extension: &str,
        extension_args: &[String],
        core_args: &[String],
    ) -> Result<i32, LauncherError> {
        debug_log(&format!("extension={extension}"));
        let binary_name = format!("tempo-{extension}");
        let display_name = format!("tempo {extension}");
        if let Some(binary) = self.find_binary(&binary_name) {
            debug_log(&format!("extension found locally: {}", binary.display()));
            self.maybe_auto_update(extension);
            return run_child(binary, extension_args, &display_name);
        }

        if is_core_subcommand(extension) {
            debug_log("classified as core subcommand");
            if self.find_binary(CORE_BINARY).is_none() {
                debug_log("tempo-core missing, attempting tempoup auto-install");
                match self.try_auto_install_core() {
                    Ok(Some(core)) => return run_child(core, core_args, "tempo"),
                    Ok(None) => {}
                    Err(err) => return Err(err),
                }
            }
        } else {
            debug_log("classified as extension");
            match self.try_auto_install_extension(extension) {
                Ok(Some(binary)) => {
                    return run_child(binary, extension_args, &display_name);
                }
                Ok(None) => {}
                Err(err) => {
                    if let Some(core) = self.find_binary(CORE_BINARY) {
                        return run_child(core, core_args, "tempo");
                    }
                    return Err(err);
                }
            }
        }

        if let Some(core) = self.find_binary(CORE_BINARY) {
            return run_child(core, core_args, "tempo");
        }

        print_missing_install_hint(extension);
        Ok(1)
    }

    fn print_help(&self) {
        println!(
            "Tempo CLI {} ({})\n",
            env!("CARGO_PKG_VERSION"),
            env!("TEMPO_GIT_SHA")
        );
        println!("Usage: tempo <command> [args...]\n");
        println!("Management:");
        println!("  add <name>    Install an extension");
        println!("  update <name> Update an extension");
        println!("  remove <name> Remove an extension\n");
        println!("Run any installed extension as: tempo <name> [args...]");
        println!("Extensions are auto-installed on first use when available.");
    }

    fn try_auto_install_extension(
        &self,
        extension: &str,
    ) -> Result<Option<PathBuf>, LauncherError> {
        let manifest = manifest_url(extension, None);
        debug_log(&format!("auto-install manifest={manifest}"));

        let binary_name = format!("tempo-{extension}");

        let installer = Installer::from_env()?;
        match installer.install(
            extension,
            &InstallSource {
                manifest: Some(manifest),
                public_key: Some(release_public_key()),
            },
            false,
            false,
        ) {
            Ok(()) => Ok(self.find_binary(&binary_name)),
            Err(InstallerError::ReleaseManifestNotFound(_))
            | Err(InstallerError::ExtensionNotInManifest(_)) => Ok(None),
            Err(InstallerError::Network(err))
                if err.status() == Some(reqwest::StatusCode::NOT_FOUND) =>
            {
                Ok(None)
            }
            Err(err) => Err(err.into()),
        }
    }

    // Known limitation: core auto-install shells out to tempoup, which overwrites
    // the tempo binary and requires a snapshot/restore cycle. A future version
    // should treat core as a regular extension with its own signed manifest.
    fn try_auto_install_core(&self) -> Result<Option<PathBuf>, LauncherError> {
        #[cfg(windows)]
        {
            return Err(InstallerError::InvalidState(
                "core auto-install fallback while tempo is running is not supported on windows; run tempoup manually and retry"
                    .to_string(),
            )
            .into());
        }

        // On unix, tempoup overwrites the running `tempo` binary with core.
        // We snapshot the current bytes, let tempoup run, then restore.
        let tempo_bytes = match &self.exe_path {
            Some(path) => Some(fs::read(path)?),
            None => None,
        };

        let Some(tempo_bytes) = tempo_bytes else {
            return Err(InstallerError::InvalidState(
                "core install blocked: tempo binary path unavailable for restore".to_string(),
            )
            .into());
        };

        let installer = Installer::from_env()?;
        // TEMPO_BIN_DIR is tempoup's interface for target directory, not a tempo env var.
        let status = match Command::new("tempoup")
            .env("TEMPO_BIN_DIR", &installer.bin_dir)
            .status()
        {
            Ok(status) => status,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(LauncherError::Io(err)),
        };

        if !status.success() {
            return Ok(None);
        }

        let installed_tempo = installer.bin_dir.join(executable_name("tempo"));
        if !installed_tempo.is_file() {
            return Ok(None);
        }

        let core_path = installer.bin_dir.join(executable_name(CORE_BINARY));
        // Copy instead of rename so `installed_tempo` remains intact if restore fails.
        fs::copy(&installed_tempo, &core_path)?;

        // Now overwrite the installed tempo with the original CLI binary.
        // If this fails, the original tempoup-installed binary is still at `installed_tempo`.
        if let Err(err) = fs::write(&installed_tempo, &tempo_bytes) {
            let _ = fs::remove_file(&core_path);
            return Err(InstallerError::InvalidState(format!(
                "core install failed: could not restore tempo after tempoup ({err}). re-run tempoup"
            ))
            .into());
        }
        if let Err(err) = set_executable_permissions(&installed_tempo) {
            let _ = fs::remove_file(&core_path);
            return Err(InstallerError::InvalidState(format!(
                "core install failed: could not set tempo permissions ({err}). re-run tempoup"
            ))
            .into());
        }

        eprintln!(
            "warn: tempoup installed core as tempo; tempo restored and core moved to {}",
            core_path.display()
        );

        Ok(Some(core_path))
    }

    /// Check for extension updates and install if a newer version is available.
    ///
    /// Runs at most once every 6 hours per extension. Failures are silent —
    /// the existing binary is always used if the update check or install fails.
    fn maybe_auto_update(&self, extension: &str) {
        // TEMPO_HOME indicates a managed or test environment where updates
        // should be explicit (via `tempo update`), not automatic.
        if env::var_os("TEMPO_HOME").is_some() {
            return;
        }

        let mut state = State::load();
        if !state.needs_update_check(extension) {
            return;
        }

        let url = manifest_url(extension, None);
        let latest_version = match fetch_manifest_version(&url) {
            Ok(v) => v,
            Err(_) => {
                debug_log(&format!(
                    "auto-update: manifest fetch failed for {extension}"
                ));
                state.touch_check(extension);
                state.save();
                return;
            }
        };

        let installed_version = state
            .extensions
            .get(extension)
            .map(|e| e.installed_version.as_str());

        if installed_version != Some(latest_version.as_str()) {
            debug_log(&format!(
                "auto-update: {extension} {old} -> {latest_version}",
                old = installed_version.unwrap_or("(untracked)")
            ));
            if let Ok(installer) = Installer::from_env() {
                let source = InstallSource {
                    manifest: Some(url),
                    public_key: Some(release_public_key()),
                };
                if installer.install(extension, &source, false, true).is_ok()
                    && installed_version.is_some_and(|v| !v.is_empty())
                {
                    eprintln!("Updated tempo-{extension} to {latest_version}");
                }
            }
        }

        state.record_check(extension, &latest_version);
        state.save();
    }

    fn find_binary(&self, binary: &str) -> Option<PathBuf> {
        if let Some(dir) = &self.exe_dir {
            for candidate in &binary_candidates(binary) {
                let path = dir.join(candidate);
                if path.is_file() {
                    return Some(path);
                }
            }
        }

        crate::installer::resolve_from_path(binary)
    }
}

fn parse_management_args(args: &[String]) -> Result<ManagementArgs, LauncherError> {
    let mut extension = None;
    let mut version = None;
    let mut manifest = None;
    let mut public_key = None;
    let mut dry_run = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--release-manifest" => {
                i += 1;
                let value = args.get(i).ok_or_else(|| {
                    LauncherError::InvalidArgs("--release-manifest requires a value".to_string())
                })?;
                manifest = Some(value.clone());
            }
            "--release-public-key" => {
                i += 1;
                let value = args.get(i).ok_or_else(|| {
                    LauncherError::InvalidArgs("--release-public-key requires a value".to_string())
                })?;
                public_key = Some(value.clone());
            }
            "--dry-run" => {
                dry_run = true;
            }
            value if value.starts_with("--") => {
                return Err(LauncherError::InvalidArgs(format!("unknown flag: {value}")));
            }
            name => {
                if extension.is_none() {
                    extension = Some(name.to_string());
                } else if version.is_none() {
                    version = Some(name.to_string());
                } else {
                    return Err(LauncherError::InvalidArgs(
                        "unexpected positional argument".to_string(),
                    ));
                }
            }
        }
        i += 1;
    }

    let extension = extension.ok_or_else(|| {
        LauncherError::InvalidArgs("extension name required (e.g., core, wallet)".to_string())
    })?;

    Ok(ManagementArgs {
        extension,
        version,
        source: InstallSource {
            manifest,
            public_key,
        },
        dry_run,
    })
}

fn extensions_base_url() -> String {
    env::var("TEMPO_EXTENSIONS_URL").unwrap_or_else(|_| EXTENSIONS_BASE_URL.to_string())
}

fn release_public_key() -> String {
    env::var("TEMPO_RELEASE_PUBLIC_KEY").unwrap_or_else(|_| PUBLIC_KEY.to_string())
}

fn manifest_url(extension: &str, version: Option<&str>) -> String {
    let base = extensions_base_url();
    let base = base.trim_end_matches('/');
    match version {
        Some(v) => {
            let v = v.strip_prefix('v').unwrap_or(v);
            format!("{base}/tempo-{extension}/v{v}/manifest.json")
        }
        None => format!("{base}/tempo-{extension}/manifest.json"),
    }
}

fn run_child(binary: PathBuf, args: &[String], display_name: &str) -> Result<i32, LauncherError> {
    debug_log(&format!("exec {} args={args:?}", binary.display()));

    let mut cmd = Command::new(&binary);

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.arg0(display_name);
    }

    let status = cmd.args(args).status()?;
    let code = status.code().unwrap_or(1);
    Ok(code)
}

fn is_core_subcommand(name: &str) -> bool {
    CORE_SUBCOMMANDS.contains(&name)
}

fn print_missing_install_hint(extension: &str) {
    eprintln!("Unknown command '{extension}' and no compatible extension found.");
    if is_core_subcommand(extension) {
        eprintln!("Run: tempoup");
    } else {
        eprintln!("Run: tempo add {extension}");
    }
}

#[cfg(test)]
mod tests {
    use super::{is_core_subcommand, manifest_url, CORE_SUBCOMMANDS};
    use crate::installer::is_secure_or_local_manifest_location;

    #[test]
    fn core_subcommand_map_is_explicit() {
        for cmd in [
            "core",
            "node",
            "init",
            "db",
            "consensus",
            "init-from-binary-dump",
        ] {
            assert!(
                is_core_subcommand(cmd),
                "expected {cmd} to be a core subcommand"
            );
        }
    }

    #[test]
    fn non_core_names_are_not_core_subcommands() {
        for cmd in ["wallet", "bridge", "dev", "unknown"] {
            assert!(
                !is_core_subcommand(cmd),
                "expected {cmd} to not be a core subcommand"
            );
        }
    }

    #[test]
    fn core_subcommand_snapshot_is_stable() {
        assert_eq!(
            CORE_SUBCOMMANDS,
            [
                "consensus",
                "core",
                "db",
                "init",
                "init-from-binary-dump",
                "node"
            ]
        );
    }

    #[test]
    fn runtime_manifest_url_policy_enforces_https_or_local() {
        assert!(is_secure_or_local_manifest_location(
            "https://cli.tempo.xyz/extensions/tempo-wallet/manifest.json"
        ));
        assert!(is_secure_or_local_manifest_location(
            "file:///tmp/manifest.json"
        ));
        assert!(is_secure_or_local_manifest_location("./manifest.json"));
        assert!(!is_secure_or_local_manifest_location(
            "http://insecure.example.com/manifest.json"
        ));
    }

    #[test]
    fn manifest_url_uses_expected_format() {
        assert_eq!(
            manifest_url("wallet", None),
            "https://cli.tempo.xyz/extensions/tempo-wallet/manifest.json"
        );

        assert_eq!(
            manifest_url("wallet", Some("0.2.0")),
            "https://cli.tempo.xyz/extensions/tempo-wallet/v0.2.0/manifest.json"
        );

        assert_eq!(
            manifest_url("wallet", Some("v0.2.0")),
            "https://cli.tempo.xyz/extensions/tempo-wallet/v0.2.0/manifest.json",
            "v-prefix should not be doubled"
        );
    }
}
