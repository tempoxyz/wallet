//! Output formatting and display utilities for the CLI

use crate::{config::validate_path, http::HttpResponse};
use anyhow::{Context, Result};

use super::{Cli, OutputFormat, QueryArgs};

/// Handle a regular (non-402) HTTP response
pub fn handle_regular_response(cli: &Cli, query: &QueryArgs, response: HttpResponse) -> Result<()> {
    match cli.output_format {
        OutputFormat::Json => {
            if let Ok(json_value) = serde_json::from_slice::<serde_json::Value>(&response.body) {
                let output = serde_json::to_string_pretty(&json_value)?;
                write_output_to(query.output.as_deref(), cli, output)?;
            } else {
                output_response_body(query.output.as_deref(), cli, &response.body)?;
            }
        }
        OutputFormat::Text => {
            if query.include_headers {
                println!("HTTP {}", response.status_code);
                for (name, value) in &response.headers {
                    println!("{name}: {value}");
                }
                println!();
            }

            output_response_body(query.output.as_deref(), cli, &response.body)?;
        }
    }

    Ok(())
}

/// Write response body to file or stdout.
///
/// Writes exact bytes with no trailing newline, matching curl-like semantics.
/// This preserves binary payloads and strict byte-stream consumers.
pub fn output_response_body(output_file: Option<&str>, cli: &Cli, body: &[u8]) -> Result<()> {
    if let Some(output_file) = output_file {
        if output_file == "-" {
            use std::io::Write;
            std::io::stdout()
                .write_all(body)
                .context("Failed to write response to stdout")?;
        } else {
            validate_path(output_file, true).context("Invalid output path")?;
            std::fs::write(output_file, body).context("Failed to write output file")?;
            if cli.is_verbose() && cli.should_show_output() {
                eprintln!("Saved to: {output_file}");
            }
        }
    } else {
        use std::io::Write;
        std::io::stdout()
            .write_all(body)
            .context("Failed to write response to stdout")?;
    }
    Ok(())
}

/// Write string output to a specific file or stdout
pub fn write_output_to(
    output_file: Option<&str>,
    cli: &Cli,
    content: impl AsRef<str>,
) -> Result<()> {
    let content = content.as_ref();
    if let Some(output_file) = output_file {
        if output_file == "-" {
            println!("{content}");
        } else {
            validate_path(output_file, true).context("Invalid output path")?;
            std::fs::write(output_file, content).context("Failed to write output file")?;
            if cli.is_verbose() && cli.should_show_output() {
                eprintln!("Saved to: {output_file}");
            }
        }
    } else {
        println!("{content}");
    }
    Ok(())
}
