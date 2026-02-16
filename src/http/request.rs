//! HTTP request handling for the CLI
//!
//! This module provides the RequestContext type and related functionality
//! for building and executing HTTP requests.

use std::io::Read;

use crate::http::{has_header, HttpClient, HttpClientBuilder, HttpMethod, HttpResponse};
use anyhow::{bail, Result};

use crate::cli::{Cli, QueryArgs};

/// Maximum request body size (100 MB)
const MAX_BODY_SIZE: usize = 100 * 1024 * 1024;

/// Maximum header size (8 KB)
const MAX_HEADER_SIZE: usize = 8 * 1024;

fn validate_body_size(len: usize) -> Result<()> {
    if len > MAX_BODY_SIZE {
        bail!(
            "Request body exceeds maximum size of {} bytes",
            MAX_BODY_SIZE
        );
    }
    Ok(())
}

/// Resolve a `-d` value to raw bytes.
///
/// Supports curl-compatible syntax:
/// - `@filename` — read the file as binary
/// - `@-` — read stdin as binary
/// - anything else — treat as a literal UTF-8 string
fn resolve_data(data: &str) -> Result<Vec<u8>> {
    if let Some(path) = data.strip_prefix('@') {
        if path == "-" {
            let mut buf = Vec::new();
            std::io::stdin()
                .read_to_end(&mut buf)
                .map_err(|e| anyhow::anyhow!("failed to read stdin: {e}"))?;
            validate_body_size(buf.len())?;
            Ok(buf)
        } else {
            let buf = std::fs::read(path)
                .map_err(|e| anyhow::anyhow!("failed to read file '{}': {}", path, e))?;
            validate_body_size(buf.len())?;
            Ok(buf)
        }
    } else {
        let bytes = data.as_bytes().to_vec();
        validate_body_size(bytes.len())?;
        Ok(bytes)
    }
}

fn validate_header_size(header: &str) -> Result<()> {
    if header.len() > MAX_HEADER_SIZE {
        bail!("Header exceeds maximum size of {} bytes", MAX_HEADER_SIZE);
    }
    Ok(())
}

/// Context for making HTTP requests with optional payment headers
pub struct RequestContext {
    pub method: HttpMethod,
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
            .follow_redirects(self.query.follow_redirects)
            .insecure(self.query.insecure)
            .headers(&headers);

        if let Some(timeout) = self.query.get_timeout() {
            builder = builder.timeout(timeout);
        }

        if let Some(user_agent) = &self.query.user_agent {
            builder = builder.user_agent(user_agent);
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
fn get_request_method_and_body(query: &QueryArgs) -> Result<(HttpMethod, Option<Vec<u8>>)> {
    let body = if let Some(ref json) = query.json {
        let bytes = json.as_bytes().to_vec();
        validate_body_size(bytes.len())?;
        Some(bytes)
    } else if let Some(ref data) = query.data {
        Some(resolve_data(data)?)
    } else {
        None
    };

    let method = query
        .method
        .as_ref()
        .map(HttpMethod::from)
        .unwrap_or_else(|| {
            if body.is_some() {
                HttpMethod::Post
            } else {
                HttpMethod::Get
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
    if let Some(data) = &query.data {
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
}
