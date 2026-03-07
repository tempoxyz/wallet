//! HTTP request context building for query commands.

use anyhow::Result;
use base64::Engine;

use crate::cli::args::QueryArgs;
use crate::cli::output::OutputOptions;
use crate::cli::Cli;
use crate::error::TempoWalletError;
use crate::http::{HttpClient, HttpRequestPlan};
use crate::network::NetworkId;

use super::input::{
    has_header, join_form_pairs, parse_data_urlencode, parse_headers, resolve_method_and_body,
    should_auto_add_json_content_type, validate_header_size,
};

/// Build a `HttpClient` from CLI arguments.
///
/// This is the boundary where CLI-specific types are converted into
/// domain types used by the HTTP and payment layers.
pub(super) fn build_http_client(cli: &Cli, query: &QueryArgs) -> Result<HttpClient> {
    for header in &query.headers {
        validate_header_size(header)?;
        if header.contains('\r') || header.contains('\n') {
            anyhow::bail!(TempoWalletError::InvalidHeader(
                "header contains CR/LF characters".to_string()
            ));
        }
    }

    // Kept as Option so the payment dispatch only enforces network matching
    // when the user explicitly passed --network.
    let network = cli
        .network
        .as_deref()
        .map(|s| s.parse::<NetworkId>().map_err(|e| anyhow::anyhow!(e)))
        .transpose()?;

    let verbosity = cli.verbosity();

    // Determine method/body. HEAD and -G modes suppress the body; otherwise use full inputs.
    let suppress_body = query.head || query.get;
    let method_override = if query.head {
        Some("HEAD")
    } else if query.get && query.method.is_none() {
        Some("GET")
    } else {
        query.method.as_deref()
    };
    let (data, json, toon) = if suppress_body {
        (&[][..], None, None)
    } else {
        (
            query.data.as_slice(),
            query.json.as_deref(),
            query.toon.as_deref(),
        )
    };
    let (method, body) = resolve_method_and_body(method_override, data, json, toon)?;

    let headers = build_extra_headers(query, suppress_body, data);

    // If not using -G, merge --data-urlencode into body (form-encoded)
    let body = if !query.get && !query.data_urlencode.is_empty() {
        let mut base = body.unwrap_or_default();
        let enc_pairs = parse_data_urlencode(&query.data_urlencode)?;
        let form = join_form_pairs(&enc_pairs);
        if !base.is_empty() {
            base.push(b'&');
        }
        base.extend_from_slice(form.as_bytes());
        Some(base)
    } else {
        body
    };

    // Build retry policy from CLI flags
    let mut retry_codes: Vec<u16> = query
        .retry_http
        .as_deref()
        .map(|s| s.split(',').filter_map(|s| s.trim().parse().ok()).collect())
        .unwrap_or_default();
    // Curl parity: when --retries is set but no explicit --retry-http, use default transient set
    if query.retries.is_some() && retry_codes.is_empty() {
        retry_codes = vec![408, 429, 500, 502, 503, 504];
    }

    let plan = HttpRequestPlan {
        method,
        headers,
        body,
        timeout_secs: query.max_time,
        connect_timeout_secs: query.connect_timeout,
        follow_redirects: query.location,
        follow_redirects_limit: query.max_redirs.map(|v| v as usize),
        user_agent: query
            .user_agent
            .clone()
            .unwrap_or_else(|| HttpRequestPlan::default().user_agent),
        insecure: query.insecure,
        proxy: query.proxy.clone(),
        no_proxy: query.no_proxy,
        http2: query.http2,
        http1_only: query.http1_1,
        max_retries: query.retries.unwrap_or(0),
        base_backoff_ms: query.retry_backoff_ms.unwrap_or(250),
        max_backoff_ms: 10_000,
        retry_status_codes: retry_codes,
        // Curl parity: honor Retry-After by default when --retries is used
        honor_retry_after: query.retries.is_some() || query.retry_after,
        // Curl default has exponential backoff without jitter; only apply when user opts in
        retry_jitter_pct: query.retry_jitter,
    };

    HttpClient::new(plan, verbosity, network, query.dry_run)
}

/// Build `OutputOptions` from CLI arguments + config.
///
/// Accepts the already-parsed URL to avoid redundant parsing.
pub(super) fn build_output_options(
    cli: &Cli,
    query: &QueryArgs,
    parsed_url: &url::Url,
) -> OutputOptions {
    OutputOptions {
        output_format: cli.resolve_output_format(),
        // -I (HEAD) implies showing headers, even if -i wasn't explicitly set
        include_headers: query.include_headers || query.head,
        output_file: if query.output.is_none() && query.remote_name {
            // Derive a filename from the URL's last path segment; fallback to 'index.html'
            let seg = parsed_url
                .path_segments()
                .and_then(|mut s| s.next_back())
                .filter(|v| !v.is_empty())
                .unwrap_or("index.html");
            Some(seg.to_string())
        } else {
            query.output.clone()
        },
        verbosity: cli.verbosity(),
        dump_headers: query.dump_header.clone(),
        write_meta: query.write_meta.clone(),
    }
}

/// Build extra headers (auth, referer, compressed, content-type) on top of
/// the raw user-supplied headers.
fn build_extra_headers(
    query: &QueryArgs,
    suppress_body: bool,
    data: &[String],
) -> Vec<(String, String)> {
    let raw_headers = &query.headers;
    let mut headers = parse_headers(raw_headers);
    // Add Authorization: Basic ... if -u/--user provided and not explicitly overridden by -H
    if let Some(ref user) = query.user {
        if !has_header(raw_headers, "authorization") {
            let encoded = base64::engine::general_purpose::STANDARD.encode(user);
            headers.push(("authorization".to_string(), format!("Basic {}", encoded)));
        }
    }
    // Add Authorization: Bearer if provided and not explicitly overridden
    if let Some(ref token) = query.bearer {
        if !has_header(raw_headers, "authorization") && query.user.is_none() {
            headers.push(("authorization".to_string(), format!("Bearer {}", token)));
        }
    }
    // Add Referer header if provided and not overridden via -H
    if let Some(ref referer) = query.referer {
        if !has_header(raw_headers, "referer") {
            headers.push(("referer".to_string(), referer.clone()));
        }
    }
    // Add Accept-Encoding on --compressed (reqwest negotiates automatically; header makes intent explicit)
    if query.compressed && !has_header(raw_headers, "accept-encoding") {
        headers.push(("accept-encoding".to_string(), "gzip, br".to_string()));
    }
    if !suppress_body {
        if should_auto_add_json_content_type(
            raw_headers,
            query.json.as_deref(),
            query.toon.as_deref(),
            data,
        ) {
            headers.push(("content-type".to_string(), "application/json".to_string()));
        } else if !query.data_urlencode.is_empty() && !has_header(raw_headers, "content-type") {
            headers.push((
                "content-type".to_string(),
                "application/x-www-form-urlencoded".to_string(),
            ));
        }
    }
    headers
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use url::Url;

    use crate::cli::args::Commands;
    use crate::cli::output::OutputFormat;

    /// Parse a CLI invocation into both `Cli` and `QueryArgs` for testing.
    fn parse(args: &[&str]) -> (Cli, QueryArgs) {
        let all: Vec<&str> = std::iter::once("tempo-wallet")
            .chain(std::iter::once("query"))
            .chain(args.iter().copied())
            .collect();
        let mut cli = Cli::try_parse_from(all).unwrap();
        let query = match cli.command.take() {
            Some(Commands::Query(q)) => *q,
            _ => panic!("expected Query command"),
        };
        (cli, query)
    }

    #[test]
    fn remote_name_derives_filename_from_url() {
        let (c, q) = parse(&["-O", "https://example.com/path/file.txt"]);
        let url = Url::parse(&q.url).unwrap();

        let opts = build_output_options(&c, &q, &url);
        assert_eq!(opts.output_file.as_deref(), Some("file.txt"));
    }

    #[test]
    fn remote_name_falls_back_to_index_html() {
        let (c, q) = parse(&["-O", "https://example.com/"]);
        let url = Url::parse(&q.url).unwrap();

        let opts = build_output_options(&c, &q, &url);
        assert_eq!(opts.output_file.as_deref(), Some("index.html"));
    }

    #[test]
    fn head_implies_include_headers() {
        let (c, q) = parse(&["-I", "https://example.com"]);
        let url = Url::parse(&q.url).unwrap();

        let opts = build_output_options(&c, &q, &url);
        assert!(opts.include_headers);
    }

    #[test]
    fn explicit_output_file_overrides_remote_name() {
        let (c, q) = parse(&["-o", "custom.txt", "https://example.com/path/file.txt"]);
        let url = Url::parse(&q.url).unwrap();

        let opts = build_output_options(&c, &q, &url);
        assert_eq!(opts.output_file.as_deref(), Some("custom.txt"));
    }

    #[test]
    fn no_output_flags_means_no_file() {
        let (c, q) = parse(&["https://example.com/path/file.txt"]);
        let url = Url::parse(&q.url).unwrap();

        let opts = build_output_options(&c, &q, &url);
        assert!(opts.output_file.is_none());
        assert!(!opts.include_headers);
        assert_eq!(opts.output_format, OutputFormat::Text);
    }

    #[test]
    fn json_output_flag() {
        let (c, q) = parse(&["-j", "https://example.com"]);
        let url = Url::parse(&q.url).unwrap();

        let opts = build_output_options(&c, &q, &url);
        assert_eq!(opts.output_format, OutputFormat::Json);
    }
}
