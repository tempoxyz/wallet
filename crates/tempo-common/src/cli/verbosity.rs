//! Verbosity configuration shared across HTTP and CLI layers.

/// Verbosity configuration shared across HTTP and CLI layers.
#[derive(Clone, Copy, Debug)]
pub struct Verbosity {
    pub level: u8,
    pub show_output: bool,
}

impl Verbosity {
    /// Whether agent-level log messages should be printed (`-v`).
    #[must_use]
    pub const fn log_enabled(&self) -> bool {
        self.level >= 1 && self.show_output
    }

    /// Whether debug-level log messages should be printed (`-vv`).
    #[must_use]
    pub const fn debug_enabled(&self) -> bool {
        self.level >= 2 && self.show_output
    }
}
