//! Interactive user prompts.

/// Prompt the user for confirmation. Returns `true` if confirmed.
///
/// In non-interactive mode (piped stdin), returns an error suggesting `--yes`.
/// When `yes` is `true`, skips the prompt and returns `true` immediately.
pub(crate) fn confirm(prompt: &str, yes: bool) -> anyhow::Result<bool> {
    if yes {
        return Ok(true);
    }

    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal() {
        anyhow::bail!("Use --yes for non-interactive mode");
    }

    use std::io::{self, Write};
    print!("{prompt} [y/N] ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().eq_ignore_ascii_case("y"))
}
