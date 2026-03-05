//! Shell completions generation.

use anyhow::Result;
use clap::CommandFactory;
use clap_complete::{generate, shells};

use crate::cli::args::Shell;
use crate::cli::{Cli, Context};

/// Run the completions command: generate for a specific shell, or list supported shells.
pub(crate) fn run(_ctx: &Context, shell: Option<Shell>) -> Result<()> {
    if let Some(shell) = shell {
        generate_completions(shell)
    } else {
        println!("Supported shells: bash, zsh, fish, powershell");
        Ok(())
    }
}

/// Generate shell completions to stdout for the provided shell.
fn generate_completions(shell: Shell) -> Result<()> {
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
