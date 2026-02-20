//! HTTP client and request handling.
//!
//! Provides [`HttpClient`] for making HTTP requests and [`RequestContext`]
//! for building requests from CLI arguments.

use std::collections::HashMap;
use std::io::Read;
use std::time::Duration;

use anyhow::Result;
use thiserror::Error;
use tracing::warn;

use crate::cli::{Cli, QueryArgs};
use crate::error;

// ==================== HTTP Response ====================

#[derive(Debug)]
pub struct HttpResponse {
    pub status_code: u32,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// Convert the response body to a UTF-8 string.
    ///
    /// # Errors
    /// Returns an error if the body is not valid UTF-8.
    pub fn body_string(&self) -> error::Result<String> {
        Ok(String::from_utf8(self.body.clone())?)
    }

    /// Check if this response indicates payment is required (HTTP 402).
    pub fn is_payment_required(&self) -> bool {
        self.status_code == 402
    }

    /// Get a header value by name (case-insensitive).
    pub fn get_header(&self, name: &str) -> Option<&String> {
        self.headers.get(&name.to_lowercase())
    }
}

// ==================== HTTP Client ====================

/// Configuration for building HTTP clients.
#[derive(Clone, Default)]
struct HttpClientConfig {
    verbose: bool,
    timeout: Option<u64>,
    follow_redirects: bool,
    user_agent: Option<String>,
    headers: Vec<(String, String)>,
}

/// Builder for configuring HTTP clients.
#[must_use]
pub struct HttpClientBuilder {
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

    /// Enable following HTTP redirects.
    pub fn follow_redirects(mut self, follow: bool) -> Self {
        self.config.follow_redirects = follow;
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

    /// Build the configured async HTTP client.
    pub fn build(self) -> error::Result<HttpClient> {
        HttpClient::from_config(self.config)
    }
}

impl Default for HttpClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Async HTTP client for making HTTP requests.
pub struct HttpClient {
    client: reqwest::Client,
}

impl HttpClient {
    /// Create a new async HTTP client with default settings.
    pub fn new() -> error::Result<Self> {
        HttpClientBuilder::new().build()
    }

    /// Create an async HTTP client from configuration.
    fn from_config(config: HttpClientConfig) -> error::Result<Self> {
        let mut builder = reqwest::Client::builder().connection_verbose(config.verbose);

        if let Some(timeout) = config.timeout {
            builder = builder.timeout(Duration::from_secs(timeout));
        }

        if config.follow_redirects {
            builder = builder.redirect(reqwest::redirect::Policy::limited(10));
        } else {
            builder = builder.redirect(reqwest::redirect::Policy::none());
        }

        if let Some(ref ua) = config.user_agent {
            builder = builder.user_agent(ua);
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
                        warn!(header_name = %name, header_value = %value, error = %e, "dropping header with invalid value");
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

    /// Perform a request with the specified HTTP method and optional body.
    pub async fn request(
        &self,
        method: reqwest::Method,
        url: &str,
        body: Option<&[u8]>,
    ) -> error::Result<HttpResponse> {
        let mut request = self.client.request(method, url);

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
    async fn convert_response(response: reqwest::Response) -> error::Result<HttpResponse> {
        let status_code = response.status().as_u16() as u32;

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
///
/// # Example
/// ```
/// use presto::http::has_header;
/// let headers = vec![
///     "Content-Type: application/json".to_string(),
///     "Content-Length: 123".to_string(),
/// ];
/// assert!(has_header(&headers, "content-type"));
/// assert!(has_header(&headers, "Content-Type"));
/// assert!(!has_header(&headers, "Authorization"));
/// ```
pub fn has_header(headers: &[String], name: &str) -> bool {
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
///
/// # Example
/// ```
/// use presto::http::parse_headers;
/// let headers = vec![
///     "Content-Type: application/json".to_string(),
///     "X-Custom: a".to_string(),
///     "X-Custom: b".to_string(),
/// ];
/// let parsed = parse_headers(&headers);
/// assert_eq!(parsed.len(), 3);
/// ```
pub fn parse_headers(headers: &[String]) -> Vec<(String, String)> {
    headers
        .iter()
        .filter_map(|header| {
            let (key, value) = header.split_once(':')?;
            Some((key.trim().to_lowercase(), value.trim().to_string()))
        })
        .collect()
}

// ==================== Request Context ====================

/// Maximum request body size (100 MB)
const MAX_BODY_SIZE: usize = 100 * 1024 * 1024;

/// Maximum header size (8 KB)
const MAX_HEADER_SIZE: usize = 8 * 1024;

#[derive(Error, Debug)]
pub enum RequestError {
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

fn validate_body_size(len: usize) -> std::result::Result<(), RequestError> {
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
fn resolve_data(data: &str) -> std::result::Result<Vec<u8>, RequestError> {
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

fn validate_header_size(header: &str) -> std::result::Result<(), RequestError> {
    if header.len() > MAX_HEADER_SIZE {
        return Err(RequestError::HeaderTooLarge {
            max: MAX_HEADER_SIZE,
        });
    }
    Ok(())
}

/// Context for making HTTP requests with optional payment headers
pub struct RequestContext {
    pub method: reqwest::Method,
    pub body: Option<Vec<u8>>,
    pub cli: Cli,
    pub query: QueryArgs,
}

impl RequestContext {
    /// Create a new request context from CLI and query arguments
    pub fn new(cli: Cli, query: QueryArgs) -> Result<Self> {
        for header in &query.headers {
            validate_header_size(header)?;
        }

        let (method, body) = get_request_method_and_body(&query)?;
        Ok(Self {
            method,
            body,
            cli,
            query,
        })
    }

    /// Build an HTTP client with the configured options
    pub fn build_client(&self, extra_headers: Option<&[(String, String)]>) -> Result<HttpClient> {
        let mut headers = self.query.parse_headers();

        if should_auto_add_json_content_type(&self.query) {
            headers.push(("Content-Type".to_string(), "application/json".to_string()));
        }

        if let Some(extra) = extra_headers {
            headers.extend_from_slice(extra);
        }

        let mut builder = HttpClientBuilder::new()
            .verbose(self.cli.is_verbose())
            .follow_redirects(!self.query.no_redirect)
            .user_agent(format!("presto/{}", env!("CARGO_PKG_VERSION")))
            .headers(&headers);

        if let Some(timeout) = self.query.get_timeout() {
            builder = builder.timeout(timeout);
        }

        Ok(builder.build()?)
    }

    /// Build a reqwest::Client with the same configuration as the normal HTTP client.
    ///
    /// Used for session/SSE flows that need direct access to reqwest's streaming API
    /// (e.g., bytes_stream() for SSE event parsing).
    pub fn build_reqwest_client(
        &self,
        extra_headers: Option<&[(String, String)]>,
    ) -> Result<reqwest::Client> {
        let client = self.build_client(extra_headers)?;
        Ok(client.inner_client())
    }

    /// Build a reqwest::RequestBuilder using the shared client configuration.
    ///
    /// Used by session flows that need a raw RequestBuilder for streaming.
    /// Headers, body, and content-type are applied from the query args,
    /// matching the behavior of the normal request path.
    pub fn build_reqwest_request(
        &self,
        url: &str,
        extra_headers: Option<&[(String, String)]>,
    ) -> Result<reqwest::RequestBuilder> {
        let client = self.build_reqwest_client(extra_headers)?;
        let method = self.method.clone();

        let mut builder = client.request(method, url);

        if let Some(ref body) = self.body {
            builder = builder.body(body.clone());
        }

        Ok(builder)
    }

    /// Execute an HTTP request
    pub async fn execute(
        &self,
        url: &str,
        extra_headers: Option<&[(String, String)]>,
    ) -> Result<HttpResponse> {
        let client = self.build_client(extra_headers)?;
        Ok(client
            .request(self.method.clone(), url, self.body.as_deref())
            .await?)
    }
}

/// Determine the HTTP method and body based on query arguments
fn get_request_method_and_body(query: &QueryArgs) -> Result<(reqwest::Method, Option<Vec<u8>>)> {
    let body = if let Some(ref json) = query.json {
        let bytes = json.as_bytes().to_vec();
        validate_body_size(bytes.len())?;
        Some(bytes)
    } else if !query.data.is_empty() {
        let mut combined = Vec::new();
        for item in &query.data {
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

    let method = query
        .method
        .as_ref()
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
/// - The user hasn't already provided a Content-Type header, AND
/// - Either the `--json` flag is used, OR the `-d` data looks like JSON
fn should_auto_add_json_content_type(query: &QueryArgs) -> bool {
    if has_header(&query.headers, "content-type") {
        return false;
    }

    if query.json.is_some() {
        return true;
    }
    if let Some(data) = query.data.first() {
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
    use crate::cli::test_utils::make_query_args;

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
        let query = make_query_args(&[
            "query",
            "--json",
            r#"{"key":"value"}"#,
            "http://example.com",
        ]);
        assert!(should_auto_add_json_content_type(&query));
    }

    #[test]
    fn test_should_auto_add_json_content_type_with_json_data() {
        let query = make_query_args(&["query", "-d", r#"{"key":"value"}"#, "http://example.com"]);
        assert!(should_auto_add_json_content_type(&query));
    }

    #[test]
    fn test_should_not_auto_add_when_user_provides_content_type() {
        let query = make_query_args(&[
            "query",
            "-H",
            "Content-Type: application/json",
            "-d",
            r#"{"key":"value"}"#,
            "http://example.com",
        ]);
        assert!(!should_auto_add_json_content_type(&query));
    }

    #[test]
    fn test_should_not_auto_add_content_type_case_insensitive() {
        let query = make_query_args(&[
            "query",
            "-H",
            "content-type: application/json",
            "-d",
            r#"{"key":"value"}"#,
            "http://example.com",
        ]);
        assert!(!should_auto_add_json_content_type(&query));

        let query = make_query_args(&[
            "query",
            "-H",
            "CONTENT-TYPE: application/json",
            "-d",
            r#"{"key":"value"}"#,
            "http://example.com",
        ]);
        assert!(!should_auto_add_json_content_type(&query));
    }

    #[test]
    fn test_should_not_auto_add_content_type_with_different_type() {
        let query = make_query_args(&[
            "query",
            "-H",
            "Content-Type: text/plain",
            "-d",
            r#"{"key":"value"}"#,
            "http://example.com",
        ]);
        assert!(!should_auto_add_json_content_type(&query));
    }

    #[test]
    fn test_should_auto_add_content_type_with_other_headers() {
        let query = make_query_args(&[
            "query",
            "-H",
            "Authorization: Bearer token",
            "-d",
            r#"{"key":"value"}"#,
            "http://example.com",
        ]);
        assert!(should_auto_add_json_content_type(&query));
    }

    #[test]
    fn test_should_not_auto_add_content_type_for_plain_data() {
        let query = make_query_args(&["query", "-d", "plain text", "http://example.com"]);
        assert!(!should_auto_add_json_content_type(&query));
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
        let query = make_query_args(&["query", "-d", "a=1", "-d", "b=2", "http://example.com"]);
        let (_method, body) = get_request_method_and_body(&query).unwrap();
        assert_eq!(body.unwrap(), b"a=1&b=2");
    }

    #[test]
    fn test_body_implies_post() {
        let query = make_query_args(&["query", "-d", "foo", "http://example.com"]);
        let (method, _body) = get_request_method_and_body(&query).unwrap();
        assert_eq!(method, reqwest::Method::POST);
    }

    #[test]
    fn test_explicit_method_overrides_body_implied_post() {
        let query = make_query_args(&["query", "-X", "PUT", "-d", "foo", "http://example.com"]);
        let (method, _body) = get_request_method_and_body(&query).unwrap();
        assert_eq!(method, reqwest::Method::PUT);
    }

    #[test]
    fn test_data_file_does_not_auto_add_json_content_type() {
        let query = make_query_args(&["query", "-d", "@Cargo.toml", "http://example.com"]);
        assert!(!should_auto_add_json_content_type(&query));
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
}
