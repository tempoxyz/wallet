//! Shell completions and version JSON helpers (CLI-only concerns).

use anyhow::Result;
use clap::CommandFactory;
use clap_complete::{generate, shells};

use crate::cli::{Cli, Shell};

/// Print version information as structured JSON and exit.
pub fn print_version_json() {
    let json = serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "git_commit": env!("TEMPO_GIT_SHA"),
        "build_date": env!("TEMPO_BUILD_DATE"),
        "profile": env!("TEMPO_BUILD_PROFILE"),
    });
    println!("{}", serde_json::to_string_pretty(&json).unwrap());
}

/// Generate shell completions to stdout for the provided shell.
pub fn generate_completions(shell: Shell) -> Result<()> {
    let mut cmd = Cli::command();
    let bin_name = cmd.get_name().to_string();

    match shell {
        Shell::Bash => generate(shells::Bash, &mut cmd, bin_name, &mut std::io::stdout()),
        Shell::Zsh => generate(shells::Zsh, &mut cmd, bin_name, &mut std::io::stdout()),
        Shell::Fish => generate(shells::Fish, &mut cmd, bin_name, &mut std::io::stdout()),
        Shell::PowerShell => generate(
            shells::PowerShell,
            &mut cmd,
            bin_name,
            &mut std::io::stdout(),
        ),
    }

    Ok(())
}
