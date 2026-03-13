//! CLI → domain conversion: URL parsing, HTTP client construction, request planning.

use base64::Engine;

use crate::args::QueryArgs;
use crate::http::{HttpClient, HttpRequestPlan, DEFAULT_USER_AGENT};
use tempo_common::cli::context::Context;
use tempo_common::error::{InputError, TempoError};
use tempo_common::network::NetworkId;

use super::headers::{
    has_header, parse_headers, should_auto_add_json_content_type, validate_header_size,
};
use super::payload::{
    append_data_to_query, join_form_pairs, parse_data_urlencode, resolve_method_and_body,
    validate_body_size,
};

/// Default HTTP status codes considered transient/retryable (curl parity).
const DEFAULT_RETRY_STATUS_CODES: &[u16] = &[408, 429, 500, 502, 503, 504];

/// Fully prepared request: parsed URL and configured HTTP client.
pub(crate) struct PreparedRequest {
    pub(crate) url: url::Url,
    pub(crate) http: HttpClient,
}

/// Parse, validate, and build the HTTP client from CLI arguments.
///
/// Handles URL parsing, `-G/--get` query-string appending, and client
/// construction — everything needed before execution.
pub(crate) fn prepare(ctx: &Context, query: &QueryArgs) -> Result<PreparedRequest, TempoError> {
    let mut url = parse_and_validate_url(&query.url)?;

    // Support -G/--get: append -d and --data-urlencode to query string and force GET if no explicit -X
    if query.get && (!query.data.is_empty() || !query.data_urlencode.is_empty()) {
        append_data_to_query(&mut url, &query.data, &query.data_urlencode)?;
    }

    let http = build_client(ctx, query)?;
    Ok(PreparedRequest { url, http })
}

/// Parse and validate a URL, ensuring it uses http or https.
fn parse_and_validate_url(raw: &str) -> Result<url::Url, TempoError> {
    let parsed = url::Url::parse(raw).map_err(InputError::UrlParse)?;
    let scheme = parsed.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(InputError::UnsupportedUrlScheme(scheme.to_string()).into());
    }
    Ok(parsed)
}

/// Build a `HttpClient` from CLI arguments.
///
/// This is the boundary where CLI-specific types are converted into
/// domain types used by the HTTP and payment layers.
fn build_client(ctx: &Context, query: &QueryArgs) -> Result<HttpClient, TempoError> {
    let plan = build_request_plan(query)?;

    // Keep Option so payment dispatch can distinguish an explicit --network.
    let network: Option<NetworkId> = ctx.requested_network;

    HttpClient::new(plan, ctx.verbosity, network, query.dry_run)
}

/// Assemble the HTTP request plan from CLI arguments.
///
/// Resolves method, body, headers, retry policy, and timeouts into a
/// ready-to-execute `HttpRequestPlan`.
fn build_request_plan(query: &QueryArgs) -> Result<HttpRequestPlan, TempoError> {
    for header in &query.headers {
        validate_header_size(header)?;
        if header.contains('\r') || header.contains('\n') {
            return Err(InputError::HeaderContainsControlChars.into());
        }
    }

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
        validate_body_size(base.len())?;
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
        retry_codes = DEFAULT_RETRY_STATUS_CODES.to_vec();
    }

    Ok(HttpRequestPlan {
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
            .unwrap_or_else(|| DEFAULT_USER_AGENT.to_string()),
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
    })
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
