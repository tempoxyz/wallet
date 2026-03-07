//! Output formatting and display utilities for the CLI.

use clap::ValueEnum;
use serde::Serialize;

// ---------------------------------------------------------------------------
// Output format
// ---------------------------------------------------------------------------

/// Output format for CLI commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum OutputFormat {
    Text,
    Json,
    Toon,
}

impl OutputFormat {
    /// Whether this format produces structured (non-text) output.
    pub(crate) fn is_structured(&self) -> bool {
        matches!(self, OutputFormat::Json | OutputFormat::Toon)
    }

    /// Serialize a value according to this format.
    ///
    /// JSON uses compact encoding; use [`serde_json::to_string_pretty`]
    /// directly when indented JSON is needed.
    pub(crate) fn serialize(&self, value: &impl serde::Serialize) -> anyhow::Result<String> {
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

            if is_right_boundary((end < chars.len()).then_some(chars[end])) {
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

/// Output/display options extracted from CLI arguments.
///
/// Used by response formatting functions; kept separate from
/// `HttpClient` to avoid coupling HTTP/payment layers to
/// presentation concerns.
#[derive(Clone, Debug)]
pub(crate) struct OutputOptions {
    pub(crate) output_format: OutputFormat,
    pub(crate) include_headers: bool,
    pub(crate) output_file: Option<String>,
    pub(crate) verbosity: crate::util::Verbosity,
    pub(crate) dump_headers: Option<String>,
    pub(crate) write_meta: Option<String>,
}

impl OutputOptions {
    /// Whether agent-level log messages should be printed (`-v`).
    pub(crate) fn log_enabled(&self) -> bool {
        self.verbosity.log_enabled()
    }

    /// Whether payment summaries should be printed (always, unless `--quiet`).
    pub(crate) fn payment_log_enabled(&self) -> bool {
        self.verbosity.show_output
    }
}

/// Render a structured payload as a string (`json` compact or `toon`).
pub(crate) fn format_structured(
    format: OutputFormat,
    value: &impl Serialize,
) -> anyhow::Result<String> {
    debug_assert!(format.is_structured());
    format.serialize(value)
}

/// Render a structured payload as a string, using pretty JSON for `json` output.
pub(crate) fn format_structured_pretty_json(
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
pub(crate) fn emit_structured(format: OutputFormat, value: &impl Serialize) -> anyhow::Result<()> {
    println!("{}", format_structured(format, value)?);
    Ok(())
}

/// Emit structured payload when structured output is selected.
///
/// Returns `true` when structured output was emitted.
pub(crate) fn emit_structured_if_selected(
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
pub(crate) fn emit_by_format(
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
pub(crate) fn run_text_only(
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
pub(crate) fn emit_formatted_or_fallback(
    formatter: impl FnOnce() -> anyhow::Result<String>,
    fallback: impl FnOnce() -> String,
) {
    match formatter() {
        Ok(output) => println!("{output}"),
        Err(_) => println!("{}", fallback()),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn test_output_format_is_structured() {
        assert!(!OutputFormat::Text.is_structured());
        assert!(OutputFormat::Json.is_structured());
        assert!(OutputFormat::Toon.is_structured());
    }

    #[test]
    fn test_output_format_serialize_json() {
        let data = serde_json::json!({"name": "Alice", "age": 30});
        let result = OutputFormat::Json.serialize(&data).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed, data);
    }

    #[test]
    fn test_output_format_serialize_toon() {
        let data = serde_json::json!({"name": "Alice", "age": 30});
        let result = OutputFormat::Toon.serialize(&data).unwrap();
        assert!(!result.is_empty());
        assert!(serde_json::from_str::<serde_json::Value>(&result).is_err());
    }

    #[test]
    fn test_output_format_serialize_toon_roundtrip() {
        let data = serde_json::json!({"name": "Alice", "age": 30});
        let encoded = OutputFormat::Toon.serialize(&data).unwrap();
        let decoded: serde_json::Value = toon_format::decode_default(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_output_format_serialize_toon_preserves_channel_id_hex_prefix() {
        let data = serde_json::json!({
            "sessions": [{"channel_id": "0x010203"}]
        });
        let encoded = OutputFormat::Toon.serialize(&data).unwrap();
        let decoded: serde_json::Value = toon_format::decode_default(&encoded).unwrap();
        assert_eq!(decoded["sessions"][0]["channel_id"], "0x010203");
    }

    #[test]
    fn quote_toon_ambiguous_hex_literals_quotes_tabular_hex_cells() {
        let input = "sessions[1]{channel_id,network}:\n  0x0102,tempo\n";
        let output = quote_toon_ambiguous_hex_literals(input);
        assert!(output.contains("\"0x0102\""));
    }

    #[test]
    fn format_structured_json_roundtrip() {
        let value = serde_json::json!({"ok": true, "count": 2});
        let rendered = format_structured(OutputFormat::Json, &value).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(parsed, value);
    }

    #[test]
    fn format_structured_pretty_json_is_pretty_for_json() {
        let value = serde_json::json!({"ok": true, "count": 2});
        let rendered = format_structured_pretty_json(OutputFormat::Json, &value).unwrap();
        assert!(rendered.contains('\n'));
        let parsed: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(parsed, value);
    }

    #[test]
    fn format_structured_pretty_json_roundtrips_for_toon() {
        let value = serde_json::json!({"ok": true, "count": 2});
        let rendered = format_structured_pretty_json(OutputFormat::Toon, &value).unwrap();
        let parsed: serde_json::Value = toon_format::decode_default(&rendered).unwrap();
        assert_eq!(parsed, value);
    }

    #[test]
    fn emit_by_format_uses_text_renderer_for_text() {
        let called = Cell::new(false);
        emit_by_format(OutputFormat::Text, &serde_json::json!({"ok": true}), || {
            called.set(true);
            Ok(())
        })
        .unwrap();
        assert!(called.get());
    }

    #[test]
    fn emit_by_format_skips_text_renderer_for_structured() {
        let called = Cell::new(false);
        emit_by_format(OutputFormat::Json, &serde_json::json!({"ok": true}), || {
            called.set(true);
            Ok(())
        })
        .unwrap();
        assert!(!called.get());
    }

    #[test]
    fn emit_structured_if_selected_behaves_by_format() {
        let value = serde_json::json!({"ok": true});
        let emitted = emit_structured_if_selected(OutputFormat::Text, &value).unwrap();
        assert!(!emitted);

        let emitted = emit_structured_if_selected(OutputFormat::Json, &value).unwrap();
        assert!(emitted);
    }

    #[test]
    fn run_text_only_runs_only_for_text() {
        let called = Cell::new(false);
        let ran = run_text_only(OutputFormat::Text, || {
            called.set(true);
            Ok(())
        })
        .unwrap();
        assert!(ran);
        assert!(called.get());

        called.set(false);
        let ran = run_text_only(OutputFormat::Json, || {
            called.set(true);
            Ok(())
        })
        .unwrap();
        assert!(!ran);
        assert!(!called.get());
    }
}
