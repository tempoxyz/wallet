//! Shell completions generation.

use anyhow::Result;
use clap::CommandFactory;
use clap_complete::{generate, Shell};

use crate::cli::Cli;
use tempo_common::cli::context::Context;

/// Run the completions command: generate for a specific shell, or list supported shells.
pub(crate) fn run(_ctx: &Context, shell: Option<Shell>) -> Result<()> {
    if let Some(shell) = shell {
        let mut cmd = Cli::command();
        let bin_name = cmd.get_name().to_string();
        generate(shell, &mut cmd, bin_name, &mut std::io::stdout());
        Ok(())
    } else {
        println!("Supported shells: bash, zsh, fish, powershell, elvish");
        Ok(())
    }
}
