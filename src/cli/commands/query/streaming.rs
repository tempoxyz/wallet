//! SSE response streaming with incremental stdout output.

use std::io::Write;

use anyhow::Result;
use futures::StreamExt;

use crate::cli::output::OutputOptions;
use crate::error::TempoWalletError;
use crate::http::{extract_headers, print_headers, HttpClient};
use crate::util::now_utc;

use super::receipt::write_meta_if_requested;

/// Parse a single SSE line and, if it's a `data:` field, write the NDJSON
/// object to `writer`. Returns the number of bytes written (0 if skipped).
fn write_sse_data_line(line: &[u8], writer: &mut impl Write) -> Result<usize> {
    let s = String::from_utf8_lossy(line);
    let st = s.trim_end_matches(['\r', '\n']);
    if st.is_empty() {
        return Ok(0);
    }
    // Only data: fields are extracted; event:/id:/retry: are intentionally skipped.
    let Some(rest) = st.strip_prefix("data:") else {
        return Ok(0);
    };
    let content = rest.trim_start();
    let data_value = serde_json::from_str::<serde_json::Value>(content)
        .unwrap_or_else(|_| serde_json::Value::String(content.to_string()));
    let obj = serde_json::json!({
        "event": "data",
        "data": data_value,
        "ts": now_utc(),
    });
    let out = serde_json::to_string(&obj)?;
    writer.write_all(out.as_bytes())?;
    writer.write_all(b"\n")?;
    Ok(out.len() + 1)
}

/// Drain complete lines from `buf` and convert SSE `data:` lines to NDJSON.
///
/// Returns the total number of bytes written to `writer`.
fn drain_sse_lines(buf: &mut Vec<u8>, writer: &mut impl Write) -> Result<usize> {
    let mut written = 0;
    while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
        let line: Vec<u8> = buf.drain(..=pos).collect();
        written += write_sse_data_line(&line, writer)?;
    }
    Ok(written)
}

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
    let resp = http.build_raw_request(url).send().await?;
    let status = resp.status().as_u16();
    let final_url_string = resp.url().to_string();
    let headers = extract_headers(resp.headers());

    if output_opts.include_headers {
        print_headers(status, &headers);
    }

    let mut bytes_written: usize = 0;
    let mut stdout = std::io::stdout().lock();
    if sse_json {
        let mut buf: Vec<u8> = Vec::new();
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await.transpose()? {
            buf.extend_from_slice(&chunk);
            bytes_written = bytes_written.saturating_add(drain_sse_lines(&mut buf, &mut stdout)?);
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

    // Build error message once for both SSE event and bail
    let error_msg = (status >= 400).then(|| {
        if status == 402 {
            "402 Payment Required (payment is not supported in streaming mode)".to_string()
        } else {
            super::receipt::format_http_error(status)
        }
    });

    // Emit error event before releasing the lock
    if let Some(ref msg) = error_msg {
        if sse_json {
            let obj = serde_json::json!({
                "event": "error",
                "message": msg,
                "ts": now_utc(),
            });
            let out = serde_json::to_string(&obj)?;
            stdout.write_all(out.as_bytes())?;
            stdout.write_all(b"\n")?;
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

    if let Some(msg) = error_msg {
        anyhow::bail!(TempoWalletError::Http(msg));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: feed raw SSE bytes through `drain_sse_lines` and return the output.
    fn convert_sse(input: &[u8]) -> (Vec<u8>, usize) {
        let mut buf = input.to_vec();
        let mut out = Vec::new();
        let written = drain_sse_lines(&mut buf, &mut out).unwrap();
        (out, written)
    }

    /// Parse each NDJSON line from output into a `serde_json::Value`.
    fn parse_ndjson(output: &[u8]) -> Vec<serde_json::Value> {
        String::from_utf8_lossy(output)
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[test]
    fn data_line_with_json_payload() {
        let (out, written) = convert_sse(b"data: {\"key\":\"value\"}\n");
        assert!(written > 0);
        let objs = parse_ndjson(&out);
        assert_eq!(objs.len(), 1);
        assert_eq!(objs[0]["event"], "data");
        assert_eq!(objs[0]["data"]["key"], "value");
        assert!(objs[0]["ts"].is_string());
    }

    #[test]
    fn data_line_with_plain_text() {
        let (out, _) = convert_sse(b"data: hello world\n");
        let objs = parse_ndjson(&out);
        assert_eq!(objs.len(), 1);
        assert_eq!(objs[0]["data"], "hello world");
    }

    #[test]
    fn event_and_id_lines_skipped() {
        let (out, written) = convert_sse(b"event: ping\nid: 42\nretry: 3000\n");
        assert_eq!(written, 0);
        assert!(out.is_empty());
    }

    #[test]
    fn empty_lines_skipped() {
        let (out, written) = convert_sse(b"\n\n\n");
        assert_eq!(written, 0);
        assert!(out.is_empty());
    }

    #[test]
    fn multiple_data_lines() {
        let input = b"data: first\ndata: second\n";
        let (out, _) = convert_sse(input);
        let objs = parse_ndjson(&out);
        assert_eq!(objs.len(), 2);
        assert_eq!(objs[0]["data"], "first");
        assert_eq!(objs[1]["data"], "second");
    }

    #[test]
    fn crlf_line_endings() {
        let (out, _) = convert_sse(b"data: crlf\r\n");
        let objs = parse_ndjson(&out);
        assert_eq!(objs.len(), 1);
        assert_eq!(objs[0]["data"], "crlf");
    }

    #[test]
    fn incomplete_line_stays_in_buffer() {
        let mut buf = b"data: partial".to_vec();
        let mut out = Vec::new();
        let written = drain_sse_lines(&mut buf, &mut out).unwrap();
        assert_eq!(written, 0);
        assert!(out.is_empty());
        // Incomplete data remains in the buffer
        assert_eq!(buf, b"data: partial");
    }

    #[test]
    fn data_without_space_after_colon() {
        let (out, _) = convert_sse(b"data:nospace\n");
        let objs = parse_ndjson(&out);
        assert_eq!(objs.len(), 1);
        assert_eq!(objs[0]["data"], "nospace");
    }

    #[test]
    fn mixed_lines_only_data_emitted() {
        let input = b"event: update\ndata: payload\nid: 1\n\n";
        let (out, _) = convert_sse(input);
        let objs = parse_ndjson(&out);
        assert_eq!(objs.len(), 1);
        assert_eq!(objs[0]["data"], "payload");
    }

    #[test]
    fn bytes_written_matches_output_length() {
        let (out, written) = convert_sse(b"data: test\n");
        assert_eq!(written, out.len());
    }
}
