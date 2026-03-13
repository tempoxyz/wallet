//! Shared output formatting and display utilities for extension CLIs.

use clap::ValueEnum;
use serde::Serialize;

use crate::error::TempoError;

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
    #[must_use]
    pub const fn is_structured(&self) -> bool {
        matches!(self, Self::Json | Self::Toon)
    }

    /// Serialize a value according to this format.
    ///
    /// JSON uses compact encoding; use [`serde_json::to_string_pretty`]
    /// directly when indented JSON is needed.
    ///
    /// # Errors
    ///
    /// Returns an error when JSON/TOON serialization fails.
    pub fn serialize(&self, value: &impl serde::Serialize) -> Result<String, TempoError> {
        match self {
            Self::Json => Ok(serde_json::to_string(value)?),
            Self::Toon => {
                let encoded = toon_format::encode_default(value).map_err(TempoError::ToonEncode)?;
                Ok(quote_toon_ambiguous_hex_literals(&encoded))
            }
            Self::Text => unreachable!("serialize called with Text format"),
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

    const fn is_left_boundary(ch: Option<char>) -> bool {
        matches!(
            ch,
            None | Some(' ' | ':' | ',' | '|' | '\t' | '[' | '{' | '\n')
        )
    }

    const fn is_right_boundary(ch: Option<char>) -> bool {
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
            && is_left_boundary(if i > 0 { Some(chars[i - 1]) } else { None })
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
pub(crate) fn format_structured(
    format: OutputFormat,
    value: &impl Serialize,
) -> Result<String, TempoError> {
    debug_assert!(format.is_structured());
    format.serialize(value)
}

/// Render a structured payload as a string, using pretty JSON for `json` output.
///
/// # Errors
///
/// Returns an error when structured serialization fails.
pub fn format_structured_pretty_json(
    format: OutputFormat,
    value: &impl Serialize,
) -> Result<String, TempoError> {
    match format {
        OutputFormat::Json => Ok(serde_json::to_string_pretty(value)?),
        OutputFormat::Toon => format_structured(format, value),
        OutputFormat::Text => unreachable!("format_structured_pretty_json called with text format"),
    }
}

/// Emit structured payload (`json` or `toon`) to stdout.
///
/// # Errors
///
/// Returns an error when structured serialization fails.
pub fn emit_structured(format: OutputFormat, value: &impl Serialize) -> Result<(), TempoError> {
    println!("{}", format_structured(format, value)?);
    Ok(())
}

/// Emit structured payload when structured output is selected.
///
/// Returns `true` when structured output was emitted.
///
/// # Errors
///
/// Returns an error when structured serialization fails.
pub fn emit_structured_if_selected(
    format: OutputFormat,
    value: &impl Serialize,
) -> Result<bool, TempoError> {
    if format.is_structured() {
        emit_structured(format, value)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Emit either structured payload or text output depending on selected format.
///
/// # Errors
///
/// Returns an error when rendering the selected output format fails.
pub fn emit_by_format(
    format: OutputFormat,
    structured_value: &impl Serialize,
    text_renderer: impl FnOnce() -> Result<(), TempoError>,
) -> Result<(), TempoError> {
    if format.is_structured() {
        emit_structured(format, structured_value)
    } else {
        text_renderer()
    }
}

/// Run a text renderer only when output format is text.
///
/// Returns `true` when the text renderer ran.
///
/// # Errors
///
/// Returns an error when text rendering fails.
pub fn run_text_only(
    format: OutputFormat,
    text_renderer: impl FnOnce() -> Result<(), TempoError>,
) -> Result<bool, TempoError> {
    if format.is_structured() {
        Ok(false)
    } else {
        text_renderer()?;
        Ok(true)
    }
}

/// Emit already-formatted output text to stdout, falling back to a static string on failures.
pub(crate) fn emit_formatted_or_fallback(
    formatter: impl FnOnce() -> Result<String, TempoError>,
    fallback: impl FnOnce() -> String,
) {
    match formatter() {
        Ok(output) => println!("{output}"),
        Err(_) => println!("{}", fallback()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── quote_toon_ambiguous_hex_literals ───────────────────────────────

    #[test]
    fn hex_at_string_start_gets_quoted() {
        assert_eq!(
            quote_toon_ambiguous_hex_literals("0xabc123"),
            "\"0xabc123\""
        );
    }

    #[test]
    fn hex_after_space_gets_quoted() {
        assert_eq!(
            quote_toon_ambiguous_hex_literals("key: 0xabc123"),
            "key: \"0xabc123\""
        );
    }

    #[test]
    fn hex_at_various_left_boundaries() {
        // comma
        assert_eq!(quote_toon_ambiguous_hex_literals(",0xabc"), ",\"0xabc\"");
        // pipe
        assert_eq!(quote_toon_ambiguous_hex_literals("|0xabc"), "|\"0xabc\"");
        // bracket
        assert_eq!(quote_toon_ambiguous_hex_literals("[0xabc]"), "[\"0xabc\"]");
        // brace
        assert_eq!(quote_toon_ambiguous_hex_literals("{0xabc}"), "{\"0xabc\"}");
        // tab
        assert_eq!(quote_toon_ambiguous_hex_literals("\t0xabc"), "\t\"0xabc\"");
        // newline
        assert_eq!(quote_toon_ambiguous_hex_literals("\n0xabc"), "\n\"0xabc\"");
    }

    #[test]
    fn hex_already_quoted_is_unchanged() {
        let input = r#""0xabc""#;
        assert_eq!(quote_toon_ambiguous_hex_literals(input), input);
    }

    #[test]
    fn non_boundary_hex_is_unchanged() {
        assert_eq!(quote_toon_ambiguous_hex_literals("foo0xabc"), "foo0xabc");
    }

    #[test]
    fn multiple_hex_values_all_quoted() {
        assert_eq!(
            quote_toon_ambiguous_hex_literals("0xabc 0xdef"),
            "\"0xabc\" \"0xdef\""
        );
    }

    #[test]
    fn no_hex_values_unchanged() {
        let input = "hello world, nothing special";
        assert_eq!(quote_toon_ambiguous_hex_literals(input), input);
    }

    #[test]
    fn bare_0x_without_hex_digits_not_quoted() {
        assert_eq!(
            quote_toon_ambiguous_hex_literals("0x not hex"),
            "0x not hex"
        );
    }

    #[test]
    fn right_boundary_chars_detected() {
        assert_eq!(quote_toon_ambiguous_hex_literals("0xabc]"), "\"0xabc\"]");
        assert_eq!(quote_toon_ambiguous_hex_literals("0xabc}"), "\"0xabc\"}");
        assert_eq!(quote_toon_ambiguous_hex_literals("0xabc,"), "\"0xabc\",");
        assert_eq!(quote_toon_ambiguous_hex_literals("0xabc\n"), "\"0xabc\"\n");
    }

    #[test]
    fn escaped_quotes_do_not_toggle_state() {
        // An escaped quote inside a quoted region should not close the
        // quoted section, so the trailing 0xabc stays inside quotes and
        // is left alone.
        let input = r#""hello \" 0xabc""#;
        let result = quote_toon_ambiguous_hex_literals(input);
        // 0xabc is still inside the quoted region, so it must not be double-quoted.
        assert_eq!(result, input);
    }

    // ── OutputFormat::is_structured ────────────────────────────────────

    #[test]
    fn text_is_not_structured() {
        assert!(!OutputFormat::Text.is_structured());
    }

    #[test]
    fn json_is_structured() {
        assert!(OutputFormat::Json.is_structured());
    }

    #[test]
    fn toon_is_structured() {
        assert!(OutputFormat::Toon.is_structured());
    }

    // ── OutputFormat::serialize ────────────────────────────────────────

    #[test]
    fn json_serialize_compact() {
        #[derive(Serialize)]
        struct Sample {
            name: String,
            count: u32,
        }

        let val = Sample {
            name: "test".into(),
            count: 42,
        };
        let out = OutputFormat::Json.serialize(&val).unwrap();
        assert_eq!(out, r#"{"name":"test","count":42}"#);
    }

    #[test]
    fn toon_serialize_quotes_hex() {
        let val = serde_json::json!({
            "address": "0xdeadbeef"
        });
        let out = OutputFormat::Toon.serialize(&val).unwrap();
        assert!(
            out.contains("\"0xdeadbeef\""),
            "hex should be quoted in TOON output, got: {out}"
        );
    }
}
