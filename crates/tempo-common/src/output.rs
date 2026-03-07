//! Shared output formatting and display utilities for extension CLIs.

use clap::ValueEnum;
use serde::Serialize;

/// Output format for CLI commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    Text,
    Json,
    Toon,
}

impl OutputFormat {
    /// Whether this format produces structured (non-text) output.
    pub fn is_structured(&self) -> bool {
        matches!(self, OutputFormat::Json | OutputFormat::Toon)
    }

    /// Serialize a value according to this format.
    ///
    /// JSON uses compact encoding; use [`serde_json::to_string_pretty`]
    /// directly when indented JSON is needed.
    pub fn serialize(&self, value: &impl serde::Serialize) -> anyhow::Result<String> {
        match self {
            OutputFormat::Json => Ok(serde_json::to_string(value)?),
            OutputFormat::Toon => {
                let encoded = toon_format::encode_default(value)
                    .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}"))?;
                Ok(quote_toon_ambiguous_hex_literals(&encoded))
            }
            OutputFormat::Text => unreachable!("serialize called with Text format"),
        }
    }
}

/// TOON can decode unquoted `0x...` values in tabular rows as `0 x...`.
/// Quote standalone hex literals after encoding so they round-trip losslessly.
fn quote_toon_ambiguous_hex_literals(input: &str) -> String {
    fn is_escaped_quote(chars: &[char], idx: usize) -> bool {
        let mut backslashes = 0usize;
        let mut i = idx;
        while i > 0 {
            i -= 1;
            if chars[i] == '\\' {
                backslashes += 1;
            } else {
                break;
            }
        }
        backslashes % 2 == 1
    }

    fn is_left_boundary(ch: Option<char>) -> bool {
        matches!(
            ch,
            None | Some(' ' | ':' | ',' | '|' | '\t' | '[' | '{' | '\n')
        )
    }

    fn is_right_boundary(ch: Option<char>) -> bool {
        matches!(
            ch,
            None | Some(' ' | ':' | ',' | '|' | '\t' | ']' | '}' | '\n')
        )
    }

    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    let mut in_quotes = false;

    while i < chars.len() {
        let ch = chars[i];

        if ch == '"' && !is_escaped_quote(&chars, i) {
            in_quotes = !in_quotes;
            out.push(ch);
            i += 1;
            continue;
        }

        if !in_quotes
            && ch == '0'
            && i + 2 < chars.len()
            && chars[i + 1] == 'x'
            && chars[i + 2].is_ascii_hexdigit()
            && is_left_boundary((i > 0).then_some(chars[i - 1]))
        {
            let mut end = i + 2;
            while end < chars.len() && chars[end].is_ascii_hexdigit() {
                end += 1;
            }

            if is_right_boundary(chars.get(end).copied()) {
                out.push('"');
                for c in &chars[i..end] {
                    out.push(*c);
                }
                out.push('"');
                i = end;
                continue;
            }
        }

        out.push(ch);
        i += 1;
    }

    out
}

/// Render a structured payload as a string (`json` compact or `toon`).
pub fn format_structured(format: OutputFormat, value: &impl Serialize) -> anyhow::Result<String> {
    debug_assert!(format.is_structured());
    format.serialize(value)
}

/// Render a structured payload as a string, using pretty JSON for `json` output.
pub fn format_structured_pretty_json(
    format: OutputFormat,
    value: &impl Serialize,
) -> anyhow::Result<String> {
    match format {
        OutputFormat::Json => Ok(serde_json::to_string_pretty(value)?),
        OutputFormat::Toon => format_structured(format, value),
        OutputFormat::Text => unreachable!("format_structured_pretty_json called with text format"),
    }
}

/// Emit structured payload (`json` or `toon`) to stdout.
pub fn emit_structured(format: OutputFormat, value: &impl Serialize) -> anyhow::Result<()> {
    println!("{}", format_structured(format, value)?);
    Ok(())
}

/// Emit structured payload when structured output is selected.
///
/// Returns `true` when structured output was emitted.
pub fn emit_structured_if_selected(
    format: OutputFormat,
    value: &impl Serialize,
) -> anyhow::Result<bool> {
    if format.is_structured() {
        emit_structured(format, value)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Emit either structured payload or text output depending on selected format.
pub fn emit_by_format(
    format: OutputFormat,
    structured_value: &impl Serialize,
    text_renderer: impl FnOnce() -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    if format.is_structured() {
        emit_structured(format, structured_value)
    } else {
        text_renderer()
    }
}

/// Run a text renderer only when output format is text.
///
/// Returns `true` when the text renderer ran.
pub fn run_text_only(
    format: OutputFormat,
    text_renderer: impl FnOnce() -> anyhow::Result<()>,
) -> anyhow::Result<bool> {
    if format.is_structured() {
        Ok(false)
    } else {
        text_renderer()?;
        Ok(true)
    }
}

/// Emit already-formatted output text to stdout, falling back to a static string on failures.
pub fn emit_formatted_or_fallback(
    formatter: impl FnOnce() -> anyhow::Result<String>,
    fallback: impl FnOnce() -> String,
) {
    match formatter() {
        Ok(output) => println!("{output}"),
        Err(_) => println!("{}", fallback()),
    }
}
