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
            OutputFormat::Toon => toon_format::encode_default(value)
                .map_err(|e| anyhow::anyhow!("TOON encoding failed: {e}")),
            OutputFormat::Text => unreachable!("serialize called with Text format"),
        }
    }
}

/// Output/display options extracted from CLI arguments.
///
/// Used by response formatting functions; kept separate from
/// `HttpClient` to avoid coupling HTTP/payment layers to
/// presentation concerns.
#[derive(Clone, Debug)]
pub(crate) struct OutputOptions {
    pub output_format: OutputFormat,
    pub include_headers: bool,
    pub output_file: Option<String>,
    pub verbosity: crate::util::Verbosity,
    pub dump_headers: Option<String>,
    pub write_meta: Option<String>,
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

#[cfg(test)]
mod tests {
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
}
