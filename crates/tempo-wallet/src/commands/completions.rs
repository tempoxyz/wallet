//! Shell completions generation.

use clap::CommandFactory;
use clap_complete::{generate, Shell};

use crate::args::Cli;
use tempo_common::cli::context::Context;
use tempo_common::error::TempoError;

/// Run the completions command: generate for a specific shell, or list supported shells.
pub(crate) fn run(_ctx: &Context, shell: Option<Shell>) -> Result<(), TempoError> {
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
