//! MPP CLI output helpers built on top of shared output primitives.

pub(crate) use tempo_common::output::{
    emit_by_format, format_structured_pretty_json, run_text_only, OutputFormat,
};

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
    pub(crate) verbosity: tempo_common::util::Verbosity,
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
