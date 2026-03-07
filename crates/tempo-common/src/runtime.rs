//! Shared CLI runtime helpers used by extension binaries.

use clap::ValueEnum;

/// Color output mode shared by extension CLIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}

/// Initialize tracing subscriber based on CLI verbosity and environment.
pub fn init_tracing(silent: bool, verbose: u8, target_crates: &[&str]) {
    use tracing_subscriber::EnvFilter;

    let filter = if silent {
        EnvFilter::new("off")
    } else if let Ok(env) = std::env::var("RUST_LOG") {
        EnvFilter::new(env)
    } else {
        let app_targets = match verbose {
            0 => String::new(),
            1 => target_crates
                .iter()
                .map(|name| format!("{name}=info"))
                .collect::<Vec<_>>()
                .join(","),
            2 => target_crates
                .iter()
                .map(|name| format!("{name}=debug"))
                .collect::<Vec<_>>()
                .join(","),
            _ => {
                return tracing_subscriber::fmt()
                    .with_env_filter(
                        "trace,hyper=warn,reqwest=warn,h2=warn,rustls=warn,tower=warn,mio=warn,polling=warn",
                    )
                    .with_target(false)
                    .with_writer(std::io::stderr)
                    .without_time()
                    .init();
            }
        };

        let filter_str = if app_targets.is_empty() {
            "warn".to_string()
        } else {
            format!("warn,{app_targets}")
        };
        EnvFilter::new(filter_str)
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(std::io::stderr)
        .without_time()
        .init();
}

/// Initialize color support based on user preference and NO_COLOR env var.
pub fn init_color_support(color: ColorMode) {
    use colored::control;
    use std::io::IsTerminal;

    let no_color_env = std::env::var("NO_COLOR").is_ok();

    match color {
        ColorMode::Always => control::set_override(true),
        ColorMode::Never => control::set_override(false),
        ColorMode::Auto => {
            if no_color_env || !std::io::stdout().is_terminal() {
                control::set_override(false);
            }
        }
    }
}

/// Build a structured error payload used by extension binaries.
pub fn render_error_payload(err: &anyhow::Error, code: &str) -> serde_json::Value {
    let message = err.to_string();
    let cause = err.chain().nth(1).map(|c| c.to_string());

    let mut obj = serde_json::json!({
        "code": code,
        "message": message,
    });

    if let Some(c) = cause {
        if let serde_json::Value::Object(ref mut map) = obj {
            map.insert("cause".into(), serde_json::Value::String(c));
        }
    }

    obj
}

/// Fallback structured error string when formatting fails.
pub fn render_error_fallback(code: &str) -> String {
    format!("{{\"code\":\"{code}\",\"message\":\"error\"}}")
}
