//! CLI input processing: header parsing, body resolution, method selection, and content-type detection.

use std::io::Read;

use anyhow::Result;
use thiserror::Error;

/// Maximum request body size (100 MB)
const MAX_BODY_SIZE: usize = 100 * 1024 * 1024;

/// Maximum header size (8 KB)
const MAX_HEADER_SIZE: usize = 8 * 1024;

#[derive(Error, Debug)]
enum RequestError {
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

fn validate_body_size(len: usize) -> Result<()> {
    if len > MAX_BODY_SIZE {
        return Err(RequestError::BodyTooLarge { max: MAX_BODY_SIZE }.into());
    }
    Ok(())
}

/// Resolve a `-d` value to raw bytes.
///
/// Supports curl-compatible syntax:
/// - `@filename` — read the file as binary
/// - `@-` — read stdin as binary
/// - anything else — treat as a literal UTF-8 string
pub(super) fn resolve_data(data: &str) -> Result<Vec<u8>> {
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

/// Reject a raw header string that exceeds the maximum allowed size.
pub(super) fn validate_header_size(header: &str) -> Result<()> {
    if header.len() > MAX_HEADER_SIZE {
        return Err(RequestError::HeaderTooLarge {
            max: MAX_HEADER_SIZE,
        }
        .into());
    }
    Ok(())
}

/// Check if a header name exists in raw header strings (case-insensitive).
pub(super) fn has_header(headers: &[String], name: &str) -> bool {
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
pub(super) fn parse_headers(headers: &[String]) -> Vec<(String, String)> {
    headers
        .iter()
        .filter_map(|header| {
            let (key, value) = header.split_once(':')?;
            Some((key.trim().to_lowercase(), value.trim().to_string()))
        })
        .collect()
}

/// Determine the HTTP method and body from raw query inputs.
pub(super) fn resolve_method_and_body(
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

/// Parse --data-urlencode items into (name, value) tuples with URL-encoding applied.
pub(super) fn parse_data_urlencode(items: &[String]) -> Vec<(Option<String>, String)> {
    let mut pairs = Vec::new();
    for it in items {
        if let Some(rest) = it.strip_prefix('@') {
            // @filename — read file contents
            if let Ok(content) = std::fs::read(rest) {
                let enc = urlencoding::encode_binary(&content).to_string();
                pairs.push((None, enc));
            }
            continue;
        }
        if let Some(pos) = it.find("=@") {
            // name@filename pattern (curl-style)
            let (name, file) = it.split_at(pos);
            let file = &file[2..];
            if let Ok(content) = std::fs::read(file) {
                let enc = urlencoding::encode_binary(&content).to_string();
                pairs.push((Some(name.to_string()), enc));
            }
            continue;
        }
        if let Some((name, val)) = it.split_once('=') {
            pairs.push((Some(name.to_string()), urlencoding::encode(val).to_string()));
        } else {
            // raw value; encode as a nameless component
            pairs.push((None, urlencoding::encode(it).to_string()));
        }
    }
    pairs
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
pub(super) fn should_auto_add_json_content_type(
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
        let (_method, body) = resolve_method_and_body(None, &data, None, None).unwrap();
        assert_eq!(body.unwrap(), b"a=1&b=2");
    }

    #[test]
    fn test_body_implies_post() {
        let data = vec!["foo".to_string()];
        let (method, _body) = resolve_method_and_body(None, &data, None, None).unwrap();
        assert_eq!(method, reqwest::Method::POST);
    }

    #[test]
    fn test_explicit_method_overrides_body_implied_post() {
        let data = vec!["foo".to_string()];
        let (method, _body) = resolve_method_and_body(Some("PUT"), &data, None, None).unwrap();
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
        let (_method, body) = resolve_method_and_body(None, &[], None, Some(toon_data)).unwrap();
        let body = body.expect("body should be present");
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["name"], "Alice");
        assert_eq!(parsed["age"], 30);
    }

    #[test]
    fn test_toon_input_implies_post() {
        let toon_data = "name: Alice";
        let (method, _body) = resolve_method_and_body(None, &[], None, Some(toon_data)).unwrap();
        assert_eq!(method, reqwest::Method::POST);
    }

    #[test]
    fn test_toon_input_explicit_method_preserved() {
        let toon_data = "name: Alice";
        let (method, _body) =
            resolve_method_and_body(Some("PUT"), &[], None, Some(toon_data)).unwrap();
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
        let result = resolve_method_and_body(None, &[], None, Some(toon_data));
        assert!(result.is_err(), "expected error for invalid TOON input");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("TOON"),
            "error should mention TOON, got: {msg}"
        );
    }

    #[test]
    fn test_parse_data_urlencode_simple() {
        let items = vec!["key=hello world".to_string()];
        let result = parse_data_urlencode(&items);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, Some("key".to_string()));
        assert_eq!(result[0].1, "hello%20world");
    }

    #[test]
    fn test_parse_data_urlencode_raw() {
        let items = vec!["hello world".to_string()];
        let result = parse_data_urlencode(&items);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, None);
        assert_eq!(result[0].1, "hello%20world");
    }
}
