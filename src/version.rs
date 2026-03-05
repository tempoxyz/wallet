//! Version checking and self-update logic.

use std::io::{self, Write};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;

use crate::config::Config;

const BINARIES_BASE_URL: &str = "https://presto-binaries.tempo.xyz";
const CHECK_INTERVAL_SECS: u64 = 6 * 60 * 60;

/// Check for updates (at most once per 6 hours) and print a notice if newer.
///
/// Mutates `config.version` to cache the check timestamp and latest version,
/// then persists the config to disk. Silently swallows all errors — never
/// affects CLI behavior.
pub(crate) async fn check_for_updates(config: &mut Config) {
    let _: Result<()> = async {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

        if now.saturating_sub(config.version.last_check) < CHECK_INTERVAL_SECS {
            print_update_notice(&config.version.latest_version);
            return Ok(());
        }

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()?;
        let resp = client
            .get(format!("{BINARIES_BASE_URL}/VERSION"))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Ok(());
        }
        let body = resp.text().await?;
        let latest = body.trim().to_string();

        if parse_version(&latest).is_none() {
            return Ok(());
        }

        config.version.last_check = now;
        config.version.latest_version = latest.clone();
        let _ = config.save();

        print_update_notice(&latest);
        Ok(())
    }
    .await;
}

/// Download and run the install script to update to the latest version.
pub(crate) fn run_update(yes: bool) -> Result<()> {
    let install_url = format!("{BINARIES_BASE_URL}/install.sh");

    if !yes {
        eprintln!("This will run a remote install script: {install_url}\n");
        eprint!("Proceed? [y/N]: ");
        io::stderr().flush().ok();
        let mut line = String::new();
        io::stdin().read_line(&mut line).ok();
        let ans = line.trim().to_ascii_lowercase();
        if ans != "y" && ans != "yes" {
            eprintln!("Aborted.");
            return Ok(());
        }
    }

    eprintln!("Updating presto to the latest version...\n");

    let status = std::process::Command::new("bash")
        .arg("-c")
        .arg(format!("curl -fsSL {install_url} | bash"))
        .status()
        .map_err(|e| anyhow::anyhow!("failed to run update script: {e}"))?;

    if !status.success() {
        anyhow::bail!("update failed (exit code {})", status.code().unwrap_or(1));
    }

    Ok(())
}

/// Parse a `v?MAJOR.MINOR.PATCH` string into its components.
/// Rejects trailing components, pre-release suffixes, and non-numeric parts.
fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let s = s.strip_prefix('v').unwrap_or(s);
    let mut parts = s.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

fn print_update_notice(latest: &str) {
    if latest.is_empty() {
        return;
    }
    let current = env!("CARGO_PKG_VERSION");
    let is_newer = matches!(
        (parse_version(latest), parse_version(current)),
        (Some(a), Some(b)) if a > b
    );
    if is_newer {
        eprintln!(
            "  Update available: {} → {}. Run `presto update` to upgrade.\n",
            current, latest,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_version() {
        assert_eq!(parse_version("0.6.0"), Some((0, 6, 0)));
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("invalid"), None);
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("1.0"), None);
        assert_eq!(parse_version("1.0.0.1"), None);
        assert_eq!(parse_version("1.0.0\x1b[2J"), None);
        assert_eq!(parse_version("1.0.0-beta"), None);
        assert_eq!(parse_version("<html>404</html>"), None);
    }

    #[test]
    fn test_parse_version_comparison() {
        assert!(parse_version("0.7.0") > parse_version("0.6.0"));
        assert!(parse_version("1.0.0") > parse_version("0.9.9"));
        assert!(parse_version("0.6.1") > parse_version("0.6.0"));
        assert_eq!(parse_version("0.6.0"), parse_version("0.6.0"));
        assert!(parse_version("0.5.0") < parse_version("0.6.0"));
        assert!(parse_version("v0.7.0") > parse_version("0.6.0"));
        assert!(parse_version("v1.0.0") > parse_version("v0.9.9"));
    }
}
