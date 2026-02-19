//! HTTP request handling for the CLI
//!
//! This module provides the RequestContext type and related functionality
//! for building and executing HTTP requests.

use std::io::Read;

use crate::http::{has_header, HttpClient, HttpClientBuilder, HttpResponse};
use anyhow::Result;
use thiserror::Error;

use crate::cli::{Cli, QueryArgs};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::test_utils::make_query_args;

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
