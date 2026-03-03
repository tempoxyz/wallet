//! Tracing/logging setup for the CLI.

use crate::cli::Cli;

/// Initialize tracing subscriber based on CLI verbosity and environment.
pub fn init_tracing(cli: &Cli) {
    use tracing_subscriber::EnvFilter;

    // Quiet mode (-q) is absolute: override any RUST_LOG with "off"
    let filter = if cli.silent {
        EnvFilter::new("off")
    } else if let Ok(env) = std::env::var("RUST_LOG") {
        EnvFilter::new(env)
    } else {
        // Map verbosity count to tracing level for the presto crate only;
        // keep all other crates at warn to avoid noise from hyper/reqwest/alloy.
        let filter_str = match cli.verbose {
            0 => "warn",
            1 => "warn,presto=info",
            2 => "warn,presto=debug,mpp=debug",
            _ => {
                "trace,hyper=warn,reqwest=warn,h2=warn,rustls=warn,tower=warn,mio=warn,polling=warn"
            }
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
