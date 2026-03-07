//! SSE response streaming with incremental stdout output.

use std::io::Write;

use anyhow::Result;
use futures::StreamExt;

use crate::error::TempoWalletError;
use crate::http::HttpClient;

use super::receipt::{print_headers, write_meta_if_requested};
use crate::cli::output::OutputOptions;
use crate::http::http_status_text;

/// Execute a streaming request and write the body to stdout incrementally.
///
/// Bypasses [`HttpClient::execute()`] and uses the raw reqwest client directly
/// because streaming requires access to `reqwest::Response::bytes_stream()`,
/// which `execute()` does not expose (it consumes the response into `HttpResponse`).
pub(super) async fn execute_streaming(
    http: &HttpClient,
    url: &str,
    output_opts: &OutputOptions,
    sse_json: bool,
) -> Result<()> {
    let start = std::time::Instant::now();
    let mut req = http.client().request(http.plan.method.clone(), url);
    for (name, value) in &http.plan.headers {
        req = req.header(name.as_str(), value.as_str());
    }
    if let Some(ref body) = http.plan.body {
        req = req.body(body.clone());
    }
    let resp = req.send().await?;
    let status = resp.status().as_u16();
    let final_url_string = resp.url().to_string();
    let headers: Vec<(String, String)> = resp
        .headers()
        .iter()
        .filter_map(|(k, v)| {
            v.to_str()
                .ok()
                .map(|s| (k.as_str().to_lowercase(), s.to_string()))
        })
        .collect();

    if output_opts.include_headers {
        print_headers(status, &headers);
    }

    let mut bytes_written: usize = 0;
    let mut stdout = std::io::stdout().lock();
    if sse_json {
        // Convert SSE to NDJSON objects with event/data/ts schema
        let mut buf: Vec<u8> = Vec::new();
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await.transpose()? {
            buf.extend_from_slice(&chunk);
            // Process complete lines
            while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = buf.drain(..=pos).collect();
                let s = String::from_utf8_lossy(&line);
                let st = s.trim_end_matches(['\r', '\n']);
                if st.is_empty() {
                    continue;
                }
                // Only data: fields are extracted; event:/id:/retry: are intentionally skipped.
                if let Some(rest) = st.strip_prefix("data:") {
                    let content = rest.trim_start();
                    let data_value = serde_json::from_str::<serde_json::Value>(content)
                        .unwrap_or_else(|_| serde_json::Value::String(content.to_string()));
                    let obj = serde_json::json!({
                        "event": "data",
                        "data": data_value,
                        "ts": crate::util::now_utc(),
                    });
                    let out = serde_json::to_string(&obj)?;
                    stdout.write_all(out.as_bytes())?;
                    stdout.write_all(b"\n")?;
                    bytes_written = bytes_written.saturating_add(out.len() + 1);
                }
            }
            stdout.flush().ok();
        }
    } else {
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await.transpose()? {
            bytes_written = bytes_written.saturating_add(chunk.len());
            stdout.write_all(&chunk)?;
            stdout.flush().ok();
        }
    }
    drop(stdout);

    // Write meta if requested
    if let Err(e) = write_meta_if_requested(
        output_opts,
        status,
        &headers,
        start.elapsed().as_millis(),
        bytes_written,
        &final_url_string,
    ) {
        tracing::warn!("failed to write response metadata: {e}");
    }

    if status >= 400 {
        let msg = format!("{} {}", status, http_status_text(status));
        if sse_json {
            let obj = serde_json::json!({
                "event": "error",
                "message": msg,
                "ts": crate::util::now_utc(),
            });
            let out = serde_json::to_string(&obj)?;
            let mut stdout = std::io::stdout().lock();
            stdout.write_all(out.as_bytes())?;
            stdout.write_all(b"\n")?;
            stdout.flush().ok();
        }
        anyhow::bail!(TempoWalletError::Http(msg));
    }

    Ok(())
}
