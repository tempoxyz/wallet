//! HTTP client and request handling.
//!
//! Provides [`HttpClient`] for making HTTP requests, [`RequestContext`] for
//! executing requests, and [`RequestRuntime`] for runtime configuration.

use std::collections::HashMap;
use std::io::Read;
use std::time::Duration;

use anyhow::Result;
use std::sync::OnceLock;
use thiserror::Error;
use tracing::warn;

// ==================== HTTP Response ====================

#[derive(Debug)]
pub(crate) struct HttpResponse {
    pub status_code: u16,
    /// Response headers with **lowercased** keys.
    ///
    /// Header names are normalized to lowercase during conversion.
    /// Use [`get_header`](Self::get_header) for case-insensitive lookup.
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    /// The final URL after following any redirects.
    pub final_url: Option<String>,
}

impl HttpResponse {
    /// Convert the response body to a UTF-8 string.
    ///
    /// # Errors
    /// Returns an error if the body is not valid UTF-8.
    pub fn body_string(&self) -> Result<String> {
        Ok(std::str::from_utf8(&self.body)?.to_string())
    }

    /// Check if this response indicates payment is required (HTTP 402).
    pub fn is_payment_required(&self) -> bool {
        self.status_code == 402
    }

    /// Get a header value by name.
    ///
    /// Header names are stored lowercase; pass a lowercase key.
    pub fn get_header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(|s| s.as_str())
    }
}

// ==================== HTTP Client ====================

/// Configuration for building HTTP clients.
#[derive(Clone, Default)]
struct HttpClientConfig {
    verbose: bool,
    timeout: Option<u64>,
    connect_timeout: Option<u64>,
    follow_redirects: bool,
    follow_redirects_limit: Option<usize>,
    user_agent: Option<String>,
    insecure: bool,
    proxy: Option<String>,
    no_proxy: bool,
    http2: bool,
    http1_only: bool,
    headers: Vec<(String, String)>,
}

/// Builder for configuring HTTP clients.
#[must_use]
pub(crate) struct HttpClientBuilder {
    config: HttpClientConfig,
}

impl HttpClientBuilder {
    /// Create a new HTTP client builder with default settings.
    pub fn new() -> Self {
        Self {
            config: HttpClientConfig::default(),
        }
    }

    /// Enable verbose output for debugging.
    pub fn verbose(mut self, verbose: bool) -> Self {
        self.config.verbose = verbose;
        self
    }

    /// Set request timeout in seconds.
    pub fn timeout(mut self, seconds: u64) -> Self {
        self.config.timeout = Some(seconds);
        self
    }

    /// Set connect timeout in seconds.
    pub fn connect_timeout(mut self, seconds: u64) -> Self {
        self.config.connect_timeout = Some(seconds);
        self
    }

    /// Enable following HTTP redirects.
    pub fn follow_redirects(mut self, follow: bool) -> Self {
        self.config.follow_redirects = follow;
        self
    }

    /// Set a maximum number of redirects when following redirects.
    pub fn follow_redirects_limit(mut self, limit: Option<usize>) -> Self {
        self.config.follow_redirects_limit = limit;
        self
    }

    /// Set custom User-Agent header.
    pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
        self.config.user_agent = Some(ua.into());
        self
    }

    /// Add multiple headers at once.
    pub fn headers(mut self, headers: &[(String, String)]) -> Self {
        self.config.headers.extend_from_slice(headers);
        self
    }

    /// Allow invalid TLS certificates (insecure)
    pub fn insecure(mut self, insecure: bool) -> Self {
        self.config.insecure = insecure;
        self
    }

    /// Configure a proxy for all requests.
    pub fn proxy(mut self, url: Option<String>) -> Self {
        self.config.proxy = url;
        self
    }

    /// Disable use of proxies completely.
    pub fn no_proxy(mut self, no_proxy: bool) -> Self {
        self.config.no_proxy = no_proxy;
        self
    }

    /// Prefer HTTP/2 if available.
    pub fn http2(mut self, enable: bool) -> Self {
        self.config.http2 = enable;
        self
    }

    /// Force HTTP/1.1 only.
    pub fn http1_only(mut self, enable: bool) -> Self {
        self.config.http1_only = enable;
        self
    }

    /// Build the configured async HTTP client.
    pub fn build(self) -> Result<HttpClient> {
        HttpClient::from_config(self.config)
    }
}

impl Default for HttpClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Async HTTP client for making HTTP requests.
pub(crate) struct HttpClient {
    client: reqwest::Client,
}

impl HttpClient {
    /// Create a new async HTTP client with default settings.
    pub fn new() -> Result<Self> {
        HttpClientBuilder::new().build()
    }

    /// Create an async HTTP client from configuration.
    fn from_config(config: HttpClientConfig) -> Result<Self> {
        let mut builder = reqwest::Client::builder().connection_verbose(config.verbose);

        if let Some(timeout) = config.timeout {
            builder = builder.timeout(Duration::from_secs(timeout));
        }

        if let Some(connect_timeout) = config.connect_timeout {
            builder = builder.connect_timeout(Duration::from_secs(connect_timeout));
        }

        if config.follow_redirects {
            let limit = config.follow_redirects_limit.unwrap_or(10);
            builder = builder.redirect(reqwest::redirect::Policy::limited(limit));
        } else {
            builder = builder.redirect(reqwest::redirect::Policy::none());
        }

        if let Some(ref ua) = config.user_agent {
            builder = builder.user_agent(ua);
        }

        if config.insecure {
            builder = builder.danger_accept_invalid_certs(true);
        }

        if config.no_proxy {
            builder = builder.no_proxy();
        } else if let Some(ref p) = config.proxy {
            let proxy = reqwest::Proxy::all(p)?;
            builder = builder.proxy(proxy);
        }

        if config.http1_only {
            builder = builder.http1_only();
        } else if config.http2 {
            // Enable HTTP/2 features; ALPN will negotiate when available
            builder = builder.http2_adaptive_window(true);
        }

        if !config.headers.is_empty() {
            let mut header_map = reqwest::header::HeaderMap::new();
            for (name, value) in &config.headers {
                let header_name = match reqwest::header::HeaderName::from_bytes(name.as_bytes()) {
                    Ok(n) => n,
                    Err(e) => {
                        warn!(header_name = %name, error = %e, "dropping header with invalid name");
                        continue;
                    }
                };
                let header_value = match reqwest::header::HeaderValue::from_str(value) {
                    Ok(v) => v,
                    Err(e) => {
                        let safe = crate::util::redact_header_value(name, value);
                        warn!(header_name = %name, header_value = %safe, error = %e, "dropping header with invalid value");
                        continue;
                    }
                };
                header_map.insert(header_name, header_value);
            }
            builder = builder.default_headers(header_map);
        }

        let client = builder.build()?;
        Ok(HttpClient { client })
    }

    /// Perform a request with additional per-request headers.
    ///
    /// Unlike [`request`], the extra headers are added to this specific request
    /// rather than baked into the client's default headers, allowing connection
    /// pool reuse across requests with different headers (e.g. 402 → payment retry).
    pub async fn request_with_headers(
        &self,
        method: reqwest::Method,
        url: &str,
        body: Option<&[u8]>,
        extra_headers: &[(String, String)],
    ) -> Result<HttpResponse> {
        let mut request = self.client.request(method, url);

        for (name, value) in extra_headers {
            request = request.header(name, value);
        }

        if let Some(data) = body {
            request = request.body(data.to_vec());
        }

        let response = request.send().await?;
        Self::convert_response(response).await
    }

    /// Get a clone of the underlying reqwest client.
    ///
    /// Used for session/SSE flows that need direct access to reqwest's streaming API.
    pub fn inner_client(&self) -> reqwest::Client {
        self.client.clone()
    }

    /// Convert a reqwest response to our HttpResponse type
    async fn convert_response(response: reqwest::Response) -> Result<HttpResponse> {
        let status_code = response.status().as_u16();
        let final_url = Some(response.url().to_string());

        // Convert headers to HashMap with lowercase keys
        let mut headers = HashMap::new();
        for (key, value) in response.headers() {
            if let Ok(value_str) = value.to_str() {
                headers.insert(key.as_str().to_lowercase(), value_str.to_string());
            }
        }

        let body = response.bytes().await?.to_vec();

        Ok(HttpResponse {
            status_code,
            headers,
            body,
            final_url,
        })
    }
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new().unwrap_or_else(|_| Self {
            client: reqwest::Client::new(),
        })
    }
}

// ==================== Header Utilities ====================

/// Utility function to check if a header exists in the response (case-insensitive).
pub(crate) fn has_header(headers: &[String], name: &str) -> bool {
    let name_lower = name.to_lowercase();
    headers.iter().any(|h| {
        h.split_once(':')
            .is_some_and(|(k, _)| k.trim().to_lowercase() == name_lower)
    })
}

/// Parse raw header strings into a list of (name, value) pairs.
///
/// Preserves duplicate headers (important for HTTP headers like Set-Cookie).
/// Header names are lowercased for consistency. Malformed entries are skipped.
pub(crate) fn parse_headers(headers: &[String]) -> Vec<(String, String)> {
    headers
        .iter()
        .filter_map(|header| {
            let (key, value) = header.split_once(':')?;
            Some((key.trim().to_lowercase(), value.trim().to_string()))
        })
        .collect()
}

// ==================== Runtime & Plan ====================

/// Runtime flags for logging and payment decisions.
///
/// Derived from CLI arguments at the boundary layer (`request.rs`);
/// HTTP and payment modules depend on this instead of raw CLI types.
#[derive(Clone, Debug)]
pub(crate) struct RequestRuntime {
    pub verbosity: u8,
    pub show_output: bool,
    pub network: Option<String>,
    pub dry_run: bool,
}

impl RequestRuntime {
    /// Whether agent-level log messages should be printed (`-v`).
    pub fn log_enabled(&self) -> bool {
        self.verbosity >= 1 && self.show_output
    }

    /// Whether debug-level log messages should be printed (`-vv`).
    pub fn debug_enabled(&self) -> bool {
        self.verbosity >= 2 && self.show_output
    }
}

/// Pre-resolved HTTP request plan, independent of CLI types.
#[derive(Clone, Debug)]
pub(crate) struct HttpRequestPlan {
    pub method: reqwest::Method,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub timeout_secs: Option<u64>,
    pub connect_timeout_secs: Option<u64>,
    pub retries: u32,
    pub retry_backoff_ms: u64,
    pub follow_redirects: bool,
    pub follow_redirects_limit: Option<usize>,
    pub user_agent: String,
    pub insecure: bool,
    pub proxy: Option<String>,
    pub no_proxy: bool,
    pub http2: bool,
    pub http1_only: bool,
    pub verbose_connection: bool,
}

// ==================== Request Context ====================

/// Maximum request body size (100 MB)
const MAX_BODY_SIZE: usize = 100 * 1024 * 1024;

/// Maximum header size (8 KB)
const MAX_HEADER_SIZE: usize = 8 * 1024;

#[derive(Error, Debug)]
pub(crate) enum RequestError {
    #[error("Request body exceeds maximum size of {max} bytes")]
    BodyTooLarge { max: usize },

    #[error("Header exceeds maximum size of {max} bytes")]
    HeaderTooLarge { max: usize },

    #[error("failed to read stdin: {0}")]
    ReadStdin(#[source] std::io::Error),

    #[error("failed to read file '{path}': {source}")]
    ReadFile {
        path: String,
        source: std::io::Error,
    },
}

pub(crate) fn validate_body_size(len: usize) -> std::result::Result<(), RequestError> {
    if len > MAX_BODY_SIZE {
        return Err(RequestError::BodyTooLarge { max: MAX_BODY_SIZE });
    }
    Ok(())
}

/// Resolve a `-d` value to raw bytes.
///
/// Supports curl-compatible syntax:
/// - `@filename` — read the file as binary
/// - `@-` — read stdin as binary
/// - anything else — treat as a literal UTF-8 string
pub(crate) fn resolve_data(data: &str) -> std::result::Result<Vec<u8>, RequestError> {
    if let Some(path) = data.strip_prefix('@') {
        if path == "-" {
            let mut buf = Vec::new();
            std::io::stdin()
                .read_to_end(&mut buf)
                .map_err(RequestError::ReadStdin)?;
            validate_body_size(buf.len())?;
            Ok(buf)
        } else {
            let buf = std::fs::read(path).map_err(|e| RequestError::ReadFile {
                path: path.to_string(),
                source: e,
            })?;
            validate_body_size(buf.len())?;
            Ok(buf)
        }
    } else {
        let bytes = data.as_bytes().to_vec();
        validate_body_size(bytes.len())?;
        Ok(bytes)
    }
}

pub(crate) fn validate_header_size(header: &str) -> std::result::Result<(), RequestError> {
    if header.len() > MAX_HEADER_SIZE {
        return Err(RequestError::HeaderTooLarge {
            max: MAX_HEADER_SIZE,
        });
    }
    Ok(())
}

/// Context for making HTTP requests with optional payment headers.
///
/// Built from `RequestRuntime` + `HttpRequestPlan` at the CLI boundary;
/// HTTP and payment modules use this without depending on CLI types.
///
/// The base HTTP client (and its connection pool) is lazily initialized once
/// and reused across all requests, so a 402 → payment → retry cycle can
/// reuse the same TLS connection.
pub(crate) struct RequestContext {
    pub runtime: RequestRuntime,
    pub plan: HttpRequestPlan,
    /// Lazily-initialized base HTTP client shared across all requests.
    base_client: OnceLock<HttpClient>,
}

impl RequestContext {
    /// Create a new request context from runtime flags and a request plan.
    pub fn new(runtime: RequestRuntime, plan: HttpRequestPlan) -> Self {
        Self {
            runtime,
            plan,
            base_client: OnceLock::new(),
        }
    }

    /// Whether verbose log messages should be printed.
    pub fn log_enabled(&self) -> bool {
        self.runtime.log_enabled()
    }

    /// Get (or lazily create) the cached base HTTP client.
    fn get_or_build_client(&self) -> Result<&HttpClient> {
        if let Some(client) = self.base_client.get() {
            return Ok(client);
        }
        let client = self.build_new_client(&self.plan.headers)?;
        // Race is fine — worst case we build twice, but only one wins.
        Ok(self.base_client.get_or_init(|| client))
    }

    /// Build a fresh HTTP client with the given headers baked in.
    fn build_new_client(&self, headers: &[(String, String)]) -> Result<HttpClient> {
        let mut builder = HttpClientBuilder::new()
            .verbose(self.plan.verbose_connection)
            .follow_redirects(self.plan.follow_redirects)
            .follow_redirects_limit(self.plan.follow_redirects_limit)
            .user_agent(&self.plan.user_agent)
            .insecure(self.plan.insecure)
            .proxy(self.plan.proxy.clone())
            .no_proxy(self.plan.no_proxy)
            .http2(self.plan.http2)
            .http1_only(self.plan.http1_only)
            .headers(headers);

        if let Some(timeout) = self.plan.timeout_secs {
            builder = builder.timeout(timeout);
        }

        if let Some(connect_timeout) = self.plan.connect_timeout_secs {
            builder = builder.connect_timeout(connect_timeout);
        }

        builder.build()
    }

    /// Build an HTTP client from the plan, optionally adding extra headers.
    ///
    /// When `extra_headers` is `None`, returns the cached base client.
    /// When extra headers are provided, builds a fresh client (needed for
    /// session/SSE flows that bake headers into the client).
    pub fn build_client(&self, extra_headers: Option<&[(String, String)]>) -> Result<HttpClient> {
        if let Some(extra) = extra_headers {
            let mut headers = self.plan.headers.clone();
            headers.extend_from_slice(extra);
            self.build_new_client(&headers)
        } else {
            // Return a clone of the cached client so the pool is shared
            let base = self.get_or_build_client()?;
            Ok(HttpClient {
                client: base.client.clone(),
            })
        }
    }

    /// Build a reqwest::Client with the same configuration as the normal HTTP client.
    ///
    /// Used for session/SSE flows that need direct access to reqwest's streaming API
    /// (e.g., bytes_stream() for SSE event parsing).
    pub fn build_reqwest_client(
        &self,
        extra_headers: Option<&[(String, String)]>,
    ) -> Result<reqwest::Client> {
        if extra_headers.is_some() {
            let client = self.build_client(extra_headers)?;
            Ok(client.inner_client())
        } else {
            Ok(self.get_or_build_client()?.inner_client())
        }
    }

    /// Build a reqwest::RequestBuilder using the shared client configuration.
    ///
    /// Uses the cached base client for connection reuse. Extra headers (if any)
    /// are applied per-request rather than baked into the client.
    pub fn build_reqwest_request(
        &self,
        url: &str,
        extra_headers: Option<&[(String, String)]>,
    ) -> Result<reqwest::RequestBuilder> {
        let client = self.get_or_build_client()?.inner_client();

        let mut builder = client.request(self.plan.method.clone(), url);

        if let Some(extra) = extra_headers {
            for (name, value) in extra {
                builder = builder.header(name, value);
            }
        }

        if let Some(ref body) = self.plan.body {
            builder = builder.body(body.clone());
        }

        Ok(builder)
    }

    /// Execute an HTTP request, reusing the cached connection pool.
    ///
    /// Extra headers are applied per-request so the underlying TLS connection
    /// can be reused across calls with different headers (e.g. 402 → retry
    /// with Authorization).
    pub async fn execute(
        &self,
        url: &str,
        extra_headers: Option<&[(String, String)]>,
    ) -> Result<HttpResponse> {
        let client = self.get_or_build_client()?;
        let mut attempt: u32 = 0;
        let max_retries = self.plan.retries;
        let mut backoff = self.plan.retry_backoff_ms;
        let headers = extra_headers.unwrap_or(&[]);
        loop {
            match client
                .request_with_headers(
                    self.plan.method.clone(),
                    url,
                    self.plan.body.as_deref(),
                    headers,
                )
                .await
            {
                Ok(resp) => return Ok(resp),
                Err(e) => {
                    let is_transient = {
                        if let Some(re) = e.downcast_ref::<reqwest::Error>() {
                            re.is_connect() || re.is_timeout()
                        } else {
                            false
                        }
                    };
                    if is_transient && attempt < max_retries {
                        attempt += 1;
                        if self.runtime.debug_enabled() {
                            eprintln!(
                                "[retry {} of {} after {}ms: {}]",
                                attempt, max_retries, backoff, e
                            );
                        }
                        tokio::time::sleep(Duration::from_millis(backoff)).await;
                        // Exponential backoff capped at 10s
                        backoff = (backoff.saturating_mul(2)).min(10_000);
                        continue;
                    }
                    return Err(e);
                }
            }
        }
    }
}

/// Determine the HTTP method and body from raw query inputs.
pub(crate) fn get_request_method_and_body(
    method: Option<&str>,
    data: &[String],
    json: Option<&str>,
    toon: Option<&str>,
) -> Result<(reqwest::Method, Option<Vec<u8>>)> {
    let body = if let Some(toon_data) = toon {
        // Decode TOON to JSON value, then serialize as JSON bytes for the request body
        let value: serde_json::Value = toon_format::decode_default(toon_data)
            .map_err(|e| anyhow::anyhow!("failed to decode TOON input: {e}"))?;
        let bytes = serde_json::to_string(&value)?.into_bytes();
        validate_body_size(bytes.len())?;
        Some(bytes)
    } else if let Some(json) = json {
        let bytes = json.as_bytes().to_vec();
        validate_body_size(bytes.len())?;
        Some(bytes)
    } else if !data.is_empty() {
        let mut combined = Vec::new();
        for item in data {
            let resolved = resolve_data(item)?;
            if !combined.is_empty() {
                combined.push(b'&');
            }
            combined.extend(resolved);
        }
        validate_body_size(combined.len())?;
        Some(combined)
    } else {
        None
    };

    let method = method
        .map(|m| {
            reqwest::Method::from_bytes(m.to_uppercase().as_bytes()).unwrap_or(reqwest::Method::GET)
        })
        .unwrap_or_else(|| {
            if body.is_some() {
                reqwest::Method::POST
            } else {
                reqwest::Method::GET
            }
        });

    Ok((method, body))
}

fn is_json_data(data: &str) -> bool {
    let trimmed = data.trim();
    trimmed.starts_with('{') || trimmed.starts_with('[')
}

/// Determine if we should automatically add a JSON Content-Type header.
///
/// Returns true if:
/// - The provided headers don't already contain a Content-Type header, AND
/// - Either json/toon data is provided, OR the first data value looks like JSON
pub(crate) fn should_auto_add_json_content_type(
    headers: &[String],
    json: Option<&str>,
    toon: Option<&str>,
    data: &[String],
) -> bool {
    if has_header(headers, "content-type") {
        return false;
    }

    if json.is_some() || toon.is_some() {
        return true;
    }
    if let Some(data) = data.first() {
        if data.starts_with('@') {
            return false;
        }
        return is_json_data(data);
    }
    false
}

// ==================== Tests ====================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_header() {
        let headers = vec![
            "Content-Type: application/json".to_string(),
            "Content-Length: 123".to_string(),
        ];
        assert!(has_header(&headers, "content-type"));
        assert!(has_header(&headers, "Content-Type"));
        assert!(has_header(&headers, "CONTENT-TYPE"));
        assert!(!has_header(&headers, "Authorization"));
    }

    #[test]
    fn test_has_header_empty() {
        let headers: Vec<String> = vec![];
        assert!(!has_header(&headers, "Content-Type"));
    }

    #[test]
    fn test_has_header_malformed() {
        let headers = vec!["NoColonHeader".to_string()];
        assert!(!has_header(&headers, "NoColonHeader"));
    }

    #[test]
    fn test_has_header_with_whitespace() {
        let headers = vec!["  Content-Type  :  application/json  ".to_string()];
        assert!(has_header(&headers, "content-type"));
    }

    #[test]
    fn test_parse_headers() {
        let headers = vec![
            "Content-Type: application/json".to_string(),
            "Content-Length: 123".to_string(),
        ];
        let parsed = parse_headers(&headers);
        assert_eq!(parsed.len(), 2);
        assert_eq!(
            parsed[0],
            ("content-type".to_string(), "application/json".to_string())
        );
        assert_eq!(parsed[1], ("content-length".to_string(), "123".to_string()));
    }

    #[test]
    fn test_parse_headers_preserves_duplicates() {
        let headers = vec![
            "X-Custom: first".to_string(),
            "X-Custom: second".to_string(),
        ];
        let parsed = parse_headers(&headers);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].1, "first");
        assert_eq!(parsed[1].1, "second");
    }

    #[test]
    fn test_parse_headers_skips_malformed() {
        let headers = vec![
            "Content-Type: application/json".to_string(),
            "MalformedHeader".to_string(),
            "Content-Length: 123".to_string(),
        ];
        let parsed = parse_headers(&headers);
        assert_eq!(parsed.len(), 2);
        assert!(parsed.iter().all(|(k, _)| k != "malformedheader"));
    }

    #[test]
    fn test_is_json_data() {
        assert!(is_json_data(r#"{"key": "value"}"#));
        assert!(is_json_data(r#"[1, 2, 3]"#));
        assert!(is_json_data("  {\"key\": \"value\"}"));
        assert!(!is_json_data("plain text"));
        assert!(!is_json_data("key=value"));
    }

    #[test]
    fn test_should_auto_add_json_content_type_with_json_flag() {
        let headers: Vec<String> = vec![];
        assert!(should_auto_add_json_content_type(
            &headers,
            Some(r#"{"key":"value"}"#),
            None,
            &[]
        ));
    }

    #[test]
    fn test_should_auto_add_json_content_type_with_json_data() {
        let headers: Vec<String> = vec![];
        let data = vec![r#"{"key":"value"}"#.to_string()];
        assert!(should_auto_add_json_content_type(
            &headers, None, None, &data
        ));
    }

    #[test]
    fn test_should_not_auto_add_when_user_provides_content_type() {
        let headers = vec!["Content-Type: application/json".to_string()];
        let data = vec![r#"{"key":"value"}"#.to_string()];
        assert!(!should_auto_add_json_content_type(
            &headers, None, None, &data
        ));
    }

    #[test]
    fn test_should_not_auto_add_content_type_case_insensitive() {
        let headers = vec!["content-type: application/json".to_string()];
        let data = vec![r#"{"key":"value"}"#.to_string()];
        assert!(!should_auto_add_json_content_type(
            &headers, None, None, &data
        ));

        let headers = vec!["CONTENT-TYPE: application/json".to_string()];
        assert!(!should_auto_add_json_content_type(
            &headers, None, None, &data
        ));
    }

    #[test]
    fn test_should_not_auto_add_content_type_with_different_type() {
        let headers = vec!["Content-Type: text/plain".to_string()];
        let data = vec![r#"{"key":"value"}"#.to_string()];
        assert!(!should_auto_add_json_content_type(
            &headers, None, None, &data
        ));
    }

    #[test]
    fn test_should_auto_add_content_type_with_other_headers() {
        let headers = vec!["Authorization: Bearer token".to_string()];
        let data = vec![r#"{"key":"value"}"#.to_string()];
        assert!(should_auto_add_json_content_type(
            &headers, None, None, &data
        ));
    }

    #[test]
    fn test_should_not_auto_add_content_type_for_plain_data() {
        let headers: Vec<String> = vec![];
        let data = vec!["plain text".to_string()];
        assert!(!should_auto_add_json_content_type(
            &headers, None, None, &data
        ));
    }

    #[test]
    fn test_resolve_data_nonexistent_file_error() {
        let err = resolve_data("@nonexistent_file_12345.txt").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("failed to read file"), "got: {msg}");
        assert!(msg.contains("nonexistent_file_12345.txt"), "got: {msg}");
    }

    #[test]
    fn test_multiple_data_values_joined_with_ampersand() {
        let data = vec!["a=1".to_string(), "b=2".to_string()];
        let (_method, body) = get_request_method_and_body(None, &data, None, None).unwrap();
        assert_eq!(body.unwrap(), b"a=1&b=2");
    }

    #[test]
    fn test_body_implies_post() {
        let data = vec!["foo".to_string()];
        let (method, _body) = get_request_method_and_body(None, &data, None, None).unwrap();
        assert_eq!(method, reqwest::Method::POST);
    }

    #[test]
    fn test_explicit_method_overrides_body_implied_post() {
        let data = vec!["foo".to_string()];
        let (method, _body) = get_request_method_and_body(Some("PUT"), &data, None, None).unwrap();
        assert_eq!(method, reqwest::Method::PUT);
    }

    #[test]
    fn test_data_file_does_not_auto_add_json_content_type() {
        let headers: Vec<String> = vec![];
        let data = vec!["@Cargo.toml".to_string()];
        assert!(!should_auto_add_json_content_type(
            &headers, None, None, &data
        ));
    }

    #[test]
    fn test_validate_header_size_limit() {
        let header = format!("X-Big: {}", "a".repeat(MAX_HEADER_SIZE));
        let err = validate_header_size(&header).unwrap_err();
        assert!(
            err.to_string().contains("exceeds maximum size"),
            "got: {err}"
        );
    }

    #[test]
    fn test_toon_input_decoded_to_json_body() {
        let toon_data = "name: Alice\nage: 30";
        let (_method, body) =
            get_request_method_and_body(None, &[], None, Some(toon_data)).unwrap();
        let body = body.expect("body should be present");
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["name"], "Alice");
        assert_eq!(parsed["age"], 30);
    }

    #[test]
    fn test_toon_input_implies_post() {
        let toon_data = "name: Alice";
        let (method, _body) =
            get_request_method_and_body(None, &[], None, Some(toon_data)).unwrap();
        assert_eq!(method, reqwest::Method::POST);
    }

    #[test]
    fn test_toon_input_explicit_method_preserved() {
        let toon_data = "name: Alice";
        let (method, _body) =
            get_request_method_and_body(Some("PUT"), &[], None, Some(toon_data)).unwrap();
        assert_eq!(method, reqwest::Method::PUT);
    }

    #[test]
    fn test_should_auto_add_content_type_with_toon() {
        let headers: Vec<String> = vec![];
        assert!(should_auto_add_json_content_type(
            &headers,
            None,
            Some("name: test"),
            &[]
        ));
    }

    #[test]
    fn test_should_not_auto_add_content_type_with_toon_when_user_provides() {
        let headers = vec!["Content-Type: text/plain".to_string()];
        assert!(!should_auto_add_json_content_type(
            &headers,
            None,
            Some("name: test"),
            &[]
        ));
    }

    #[test]
    fn test_toon_input_invalid_errors() {
        let toon_data = "[3}: invalid";
        let result = get_request_method_and_body(None, &[], None, Some(toon_data));
        assert!(result.is_err(), "expected error for invalid TOON input");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("TOON"),
            "error should mention TOON, got: {msg}"
        );
    }
}
