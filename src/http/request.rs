//! HTTP request handling for the CLI
//!
//! This module provides the RequestContext type and related functionality
//! for building and executing HTTP requests.

use crate::http::{has_header, HttpClient, HttpClientBuilder, HttpMethod, HttpResponse};
use anyhow::{bail, Result};

use crate::cli::Cli;

/// Maximum request body size (100 MB)
const MAX_BODY_SIZE: usize = 100 * 1024 * 1024;

/// Maximum header size (8 KB)
const MAX_HEADER_SIZE: usize = 8 * 1024;

fn validate_body_size(data: &str) -> Result<()> {
    if data.len() > MAX_BODY_SIZE {
        bail!(
            "Request body exceeds maximum size of {} bytes",
            MAX_BODY_SIZE
        );
    }
    Ok(())
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
}

impl RequestContext {
    /// Create a new request context from CLI arguments
    pub fn new(cli: Cli) -> Result<Self> {
        // Validate header sizes
        for header in &cli.headers {
            validate_header_size(header)?;
        }

        // Validate body size
        if let Some(ref data) = cli.data {
            validate_body_size(data)?;
        }
        if let Some(ref json) = cli.json {
            validate_body_size(json)?;
        }

        let (method, body) = get_request_method_and_body(&cli);
        Ok(Self { method, body, cli })
    }

    /// Build an HTTP client with the configured options
    pub fn build_client(&self, extra_headers: Option<&[(String, String)]>) -> Result<HttpClient> {
        let mut headers = self.cli.parse_headers();

        if should_auto_add_json_content_type(&self.cli) {
            headers.push(("Content-Type".to_string(), "application/json".to_string()));
        }

        if let Some(extra) = extra_headers {
            headers.extend_from_slice(extra);
        }

        let mut builder = HttpClientBuilder::new()
            .verbose(self.cli.is_verbose())
            .follow_redirects(self.cli.follow_redirects)
            .headers(&headers);

        if let Some(timeout) = self.cli.get_timeout() {
            builder = builder.timeout(timeout);
        }

        if let Some(user_agent) = &self.cli.user_agent {
            builder = builder.user_agent(user_agent);
        }

        Ok(builder.build()?)
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

/// Determine the HTTP method and body based on CLI flags
fn get_request_method_and_body(cli: &Cli) -> (HttpMethod, Option<Vec<u8>>) {
    // Get the body from --data or --json
    let body = cli
        .json
        .as_ref()
        .or(cli.data.as_ref())
        .map(|s| s.as_bytes().to_vec());

    // Determine method: explicit -X flag, or POST if body present, or GET
    let method = cli
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

    (method, body)
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
fn should_auto_add_json_content_type(cli: &Cli) -> bool {
    // Don't add Content-Type if the user already provided one
    if has_header(&cli.headers, "content-type") {
        return false;
    }

    if cli.json.is_some() {
        return true;
    }
    if let Some(data) = &cli.data {
        return is_json_data(data);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::test_utils::make_cli;

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
        let cli = make_cli(&["--json", r#"{"key":"value"}"#, "http://example.com"]);
        assert!(should_auto_add_json_content_type(&cli));
    }

    #[test]
    fn test_should_auto_add_json_content_type_with_json_data() {
        let cli = make_cli(&["-d", r#"{"key":"value"}"#, "http://example.com"]);
        assert!(should_auto_add_json_content_type(&cli));
    }

    #[test]
    fn test_should_not_auto_add_when_user_provides_content_type() {
        // User explicitly provides Content-Type header - should NOT auto-add
        let cli = make_cli(&[
            "-H",
            "Content-Type: application/json",
            "-d",
            r#"{"key":"value"}"#,
            "http://example.com",
        ]);
        assert!(!should_auto_add_json_content_type(&cli));
    }

    #[test]
    fn test_should_not_auto_add_content_type_case_insensitive() {
        // Test case-insensitive matching
        let cli = make_cli(&[
            "-H",
            "content-type: application/json",
            "-d",
            r#"{"key":"value"}"#,
            "http://example.com",
        ]);
        assert!(!should_auto_add_json_content_type(&cli));

        let cli = make_cli(&[
            "-H",
            "CONTENT-TYPE: application/json",
            "-d",
            r#"{"key":"value"}"#,
            "http://example.com",
        ]);
        assert!(!should_auto_add_json_content_type(&cli));
    }

    #[test]
    fn test_should_not_auto_add_content_type_with_different_type() {
        // User provides a different Content-Type - should respect their choice
        let cli = make_cli(&[
            "-H",
            "Content-Type: text/plain",
            "-d",
            r#"{"key":"value"}"#,
            "http://example.com",
        ]);
        assert!(!should_auto_add_json_content_type(&cli));
    }

    #[test]
    fn test_should_auto_add_content_type_with_other_headers() {
        // Other headers don't affect the decision
        let cli = make_cli(&[
            "-H",
            "Authorization: Bearer token",
            "-d",
            r#"{"key":"value"}"#,
            "http://example.com",
        ]);
        assert!(should_auto_add_json_content_type(&cli));
    }

    #[test]
    fn test_should_not_auto_add_content_type_for_plain_data() {
        let cli = make_cli(&["-d", "plain text", "http://example.com"]);
        assert!(!should_auto_add_json_content_type(&cli));
    }
}
