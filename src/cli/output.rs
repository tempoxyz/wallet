//! Output formatting and display utilities for the CLI

use crate::{config::validate_path, http::HttpResponse};
use anyhow::{Context, Result};

use super::{Cli, OutputFormat, QueryArgs};

/// Handle a regular (non-402) HTTP response
pub fn handle_regular_response(cli: &Cli, query: &QueryArgs, response: HttpResponse) -> Result<()> {
    match query.effective_output_format(cli.json_output) {
        OutputFormat::Json => {
            if let Ok(json_value) = serde_json::from_slice::<serde_json::Value>(&response.body) {
                let output = serde_json::to_string_pretty(&json_value)?;
                write_output_to(query.output.as_deref(), cli, output)?;
            } else {
                output_response_body(query.output.as_deref(), cli, &response.body)?;
            }
        }
        OutputFormat::Yaml => {
            if let Ok(json_value) = serde_json::from_slice::<serde_json::Value>(&response.body) {
                let output = serde_yaml::to_string(&json_value)?;
                write_output_to(query.output.as_deref(), cli, output)?;
            } else {
                output_response_body(query.output.as_deref(), cli, &response.body)?;
            }
        }
        OutputFormat::Text => {
            if query.include_headers || query.head_only {
                println!("HTTP {}", response.status_code);
                for (name, value) in &response.headers {
                    println!("{name}: {value}");
                }
                println!();
            }

            if !query.head_only {
                output_response_body(query.output.as_deref(), cli, &response.body)?;
            }
        }
    }

    Ok(())
}

/// Write response body to file or stdout
pub fn output_response_body(output_file: Option<&str>, cli: &Cli, body: &[u8]) -> Result<()> {
    if let Some(output_file) = output_file {
        validate_path(output_file, true).context("Invalid output path")?;
        std::fs::write(output_file, body).context("Failed to write output file")?;
        if cli.is_verbose() && cli.should_show_output() {
            eprintln!("Saved to: {output_file}");
        }
    } else {
        use std::io::Write;
        let mut stdout = std::io::stdout();
        stdout
            .write_all(body)
            .context("Failed to write response to stdout")?;
        stdout.write_all(b"\n").context("Failed to write newline")?;
    }
    Ok(())
}

/// Write string output to file or stdout based on CLI options
pub fn write_output(cli: &Cli, content: impl AsRef<str>) -> Result<()> {
    write_output_to(None, cli, content)
}

/// Write string output to a specific file or stdout
pub fn write_output_to(
    output_file: Option<&str>,
    cli: &Cli,
    content: impl AsRef<str>,
) -> Result<()> {
    let content = content.as_ref();
    if let Some(output_file) = output_file {
        validate_path(output_file, true).context("Invalid output path")?;
        std::fs::write(output_file, content).context("Failed to write output file")?;
        if cli.is_verbose() && cli.should_show_output() {
            eprintln!("Saved to: {output_file}");
        }
    } else {
        println!("{content}");
    }
    Ok(())
}
