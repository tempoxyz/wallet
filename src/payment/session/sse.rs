use anyhow::{Context, Result};
use futures::StreamExt;
use std::io::Write;

use mpp::server::sse::{parse_event, SseEvent};

use super::persist::persist_session;
use super::types::{SessionContext, SessionState};
use super::voucher::{build_voucher_credential, post_voucher};

/// Stream SSE events from a response, handling voucher top-ups mid-stream.
///
/// Persists cumulative amount updates during streaming so that if the
/// process is interrupted, the session record reflects the last voucher sent.
///
/// The server has a known race condition where its `wait_for_update` notification
/// can be lost (tokio::sync::Notify without permit storage). When a voucher POST
/// arrives but the server hasn't started awaiting yet, the notification is dropped
/// and the stream stalls. We work around this by re-posting the same voucher if
/// no progress is seen within a short timeout after the last need-voucher event.
pub(super) async fn stream_sse_response(
    ctx: &SessionContext<'_>,
    state: &mut SessionState,
    response: reqwest::Response,
) -> Result<()> {
    let cli = &ctx.request_ctx.cli;
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut token_count: u64 = 0;
    let mut stdout = std::io::stdout();

    let mut stream_done = false;

    // Cap SSE buffer to prevent unbounded growth from malformed streams
    // that never emit the \n\n event delimiter.
    const MAX_BUFFER_SIZE: usize = 4 * 1024 * 1024; // 4 MB

    // Reuse a single client for voucher POSTs to maintain connection affinity
    // with the server (important when behind a load balancer).
    let voucher_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_default();

    // Track pending voucher for retry on stall. When we send a voucher but
    // the server's notify is lost, we need to re-send to wake it up.
    let mut pending_voucher_auth: Option<String> = None;
    let mut voucher_retry_count: u32 = 0;
    const MAX_VOUCHER_RETRIES: u32 = 5;

    // Normal timeout for when we're actively receiving tokens.
    let normal_timeout = std::time::Duration::from_secs(30);
    // Short timeout after sending a voucher — if the server doesn't resume
    // quickly, the notify was likely lost and we should re-post.
    let voucher_stall_timeout = std::time::Duration::from_secs(3);

    loop {
        if stream_done {
            break;
        }

        let timeout = if pending_voucher_auth.is_some() {
            voucher_stall_timeout
        } else {
            normal_timeout
        };

        let chunk = match tokio::time::timeout(timeout, stream.next()).await {
            Ok(Some(chunk)) => chunk,
            Ok(None) => break, // stream ended
            Err(_) => {
                // Timeout — if we have a pending voucher, re-post it
                if let Some(ref auth) = pending_voucher_auth {
                    voucher_retry_count += 1;
                    if voucher_retry_count > MAX_VOUCHER_RETRIES {
                        if cli.is_verbose() && cli.should_show_output() {
                            eprintln!(
                                "[stream stall — voucher not accepted after {} retries]",
                                MAX_VOUCHER_RETRIES
                            );
                        }
                        break;
                    }
                    if cli.is_verbose() && cli.should_show_output() {
                        eprintln!(
                            "[re-posting voucher (retry {}/{})]",
                            voucher_retry_count, MAX_VOUCHER_RETRIES
                        );
                    }
                    let verbose = cli.is_verbose() && cli.should_show_output();
                    post_voucher(&voucher_client, ctx.url, auth, verbose);
                    continue;
                }
                if cli.is_verbose() && cli.should_show_output() {
                    eprintln!("[stream timeout — no data for 30s]");
                }
                break;
            }
        };
        let chunk = chunk.context("Stream error")?;
        let chunk_str = String::from_utf8_lossy(&chunk);
        // Normalize \r\n to \n so SSE event boundary detection works with
        // servers/proxies that emit CRLF line endings.
        if chunk_str.contains('\r') {
            buffer.push_str(&chunk_str.replace("\r\n", "\n"));
        } else {
            buffer.push_str(&chunk_str);
        }

        if buffer.len() > MAX_BUFFER_SIZE {
            anyhow::bail!("SSE buffer exceeded {MAX_BUFFER_SIZE} bytes without a complete event — aborting stream");
        }

        while let Some(pos) = buffer.find("\n\n") {
            let event_str = buffer[..pos + 2].to_string();
            buffer = buffer[pos + 2..].to_string();

            if let Some(event) = parse_event(&event_str) {
                match event {
                    SseEvent::Message(data) => {
                        // Any message means the voucher was accepted
                        pending_voucher_auth = None;
                        voucher_retry_count = 0;

                        if data.trim() == "[DONE]" {
                            stream_done = true;
                            break;
                        }
                        if let Some(content) = extract_sse_content(&data) {
                            token_count += 1;
                            write!(stdout, "{}", content)?;
                            stdout.flush()?;
                        }
                        // Detect OpenAI finish_reason to know stream is ending
                        if is_stream_finished(&data) {
                            stream_done = true;
                            break;
                        }
                    }
                    SseEvent::PaymentNeedVoucher(nv) => {
                        let required: u128 = nv.required_cumulative.parse().unwrap_or(0);
                        if cli.is_verbose() && cli.should_show_output() {
                            eprintln!("[voucher top-up: required={}]", required);
                        }

                        state.cumulative_amount = required;

                        // Persist the updated cumulative mid-stream
                        let _ = persist_session(ctx, state);

                        let voucher =
                            build_voucher_credential(ctx.signer, ctx.echo, ctx.did, state).await?;
                        let auth = mpp::format_authorization(&voucher)
                            .context("Failed to format voucher")?;

                        let verbose = cli.is_verbose() && cli.should_show_output();
                        post_voucher(&voucher_client, ctx.url, &auth, verbose);

                        // Track this voucher for retry if the server stalls
                        pending_voucher_auth = Some(auth);
                        voucher_retry_count = 0;
                    }
                    SseEvent::PaymentReceipt(receipt) => {
                        pending_voucher_auth = None;
                        if cli.is_verbose() && cli.should_show_output() {
                            eprintln!();
                            eprintln!("Stream receipt:");
                            eprintln!("  Channel: {}", receipt.channel_id);
                            eprintln!("  Spent: {}", receipt.spent);
                            if let Some(units) = receipt.units {
                                eprintln!("  Units: {}", units);
                            }
                            if let Some(ref tx) = receipt.tx_hash {
                                eprintln!("  TX: {}", tx);
                            }
                        }
                        // Receipt signals stream completion
                        stream_done = true;
                        break;
                    }
                }
            }
        }
    }

    writeln!(stdout)?;

    if cli.is_verbose() && cli.should_show_output() {
        eprintln!("Tokens streamed: {}", token_count);
        let cumulative_f64 = state.cumulative_amount as f64 / 1e6;
        let symbol = ctx.token_symbol();
        eprintln!("Voucher cumulative: {cumulative_f64:.6} {symbol}");
    }

    Ok(())
}

/// Check if an OpenAI streaming chunk signals completion.
///
/// Returns true if the chunk contains `"finish_reason":"stop"` (or any non-null
/// finish_reason), indicating the model is done generating.
fn is_stream_finished(raw: &str) -> bool {
    let trimmed = raw.trim();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(reason) = v
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("finish_reason"))
        {
            return !reason.is_null();
        }
    }
    false
}

/// Extract token content from an OpenAI chat completion chunk.
///
/// Handles the `data:` payload from SSE streams in OpenAI format:
/// `{"choices":[{"delta":{"content":"token"}}]}`.
/// Returns None for `[DONE]`, empty deltas, or unparseable data.
fn extract_sse_content(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed == "[DONE]" {
        return None;
    }
    // Try to parse as OpenAI chat completion chunk
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(content) = v
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("delta"))
            .and_then(|d| d.get("content"))
            .and_then(|c| c.as_str())
        {
            if !content.is_empty() {
                return Some(content.to_string());
            }
            return None;
        }
        // No delta.content — could be role-only or finish delta
        return None;
    }
    // Not JSON — return raw content as-is (plain text SSE)
    Some(trimmed.to_string())
}
