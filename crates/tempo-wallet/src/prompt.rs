//! Interactive user prompts.

use tempo_common::error::{InputError, TempoError};

type PromptResult<T> = std::result::Result<T, TempoError>;

fn require_interactive_stdin(is_terminal: bool) -> PromptResult<()> {
    if is_terminal {
        Ok(())
    } else {
        Err(InputError::NonInteractiveConfirmationRequired.into())
    }
}

fn require_non_empty_stdin(read: usize) -> PromptResult<()> {
    if read == 0 {
        Err(InputError::NonInteractiveConfirmationRequired.into())
    } else {
        Ok(())
    }
}

/// Prompt the user for confirmation. Returns `true` if confirmed.
///
/// In non-interactive mode (piped stdin), returns an error suggesting `--yes`.
/// When `yes` is `true`, skips the prompt and returns `true` immediately.
pub(crate) fn confirm(prompt: &str, yes: bool) -> PromptResult<bool> {
    if yes {
        return Ok(true);
    }

    use std::io::IsTerminal;
    require_interactive_stdin(std::io::stdin().is_terminal())?;

    use std::io::{self, Write};
    print!("{prompt} [y/N] ");
    io::stdout().flush()?;

    let mut input = String::new();
    let read = io::stdin().read_line(&mut input)?;
    require_non_empty_stdin(read)?;
    Ok(input.trim().eq_ignore_ascii_case("y"))
}

#[cfg(test)]
mod tests {
    use super::{require_interactive_stdin, require_non_empty_stdin};
    use tempo_common::error::InputError;

    #[test]
    fn require_interactive_stdin_rejects_non_tty() {
        let err = require_interactive_stdin(false).expect_err("non-tty should be rejected");
        assert!(
            matches!(
                err,
                tempo_common::error::TempoError::Input(
                    InputError::NonInteractiveConfirmationRequired
                )
            ),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn require_non_empty_stdin_rejects_eof() {
        let err = require_non_empty_stdin(0).expect_err("EOF should be rejected");
        assert!(
            matches!(
                err,
                tempo_common::error::TempoError::Input(
                    InputError::NonInteractiveConfirmationRequired
                )
            ),
            "unexpected error: {err}"
        );
    }
}
