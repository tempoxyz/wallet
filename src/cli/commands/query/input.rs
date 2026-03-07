//! CLI input processing: header parsing, body resolution, method selection, and content-type detection.

use anyhow::{Context as _, Result};

use crate::error::TempoWalletError;

/// Maximum request body size (100 MB)
const MAX_BODY_SIZE: usize = 100 * 1024 * 1024;

/// Maximum header size (8 KB)
const MAX_HEADER_SIZE: usize = 8 * 1024;

/// Parse and validate a URL, ensuring it uses http or https.
pub(super) fn parse_and_validate_url(raw: &str) -> Result<url::Url> {
    let parsed = url::Url::parse(raw).map_err(|e| TempoWalletError::InvalidUrl(e.to_string()))?;
    let scheme = parsed.scheme();
    if scheme != "http" && scheme != "https" {
        anyhow::bail!(TempoWalletError::InvalidUrl(format!(
            "unsupported scheme '{scheme}'"
        )));
    }
    Ok(parsed)
}

/// Append `-d` data and `--data-urlencode` values to a URL's query string.
///
/// Used for `-G/--get` mode where request data is sent as query parameters
/// instead of the body.
pub(super) fn append_data_to_query(
    url: &mut url::Url,
    data: &[String],
    data_urlencode: &[String],
) -> Result<()> {
    // Raw -d data (verbatim, joined by '&')
    let mut raw = String::new();
    if !data.is_empty() {
        let combined = resolve_and_join_data(data)?;
        raw = String::from_utf8(combined).context("data is not valid UTF-8 for --get")?;
    }
    // Encoded data from --data-urlencode
    let enc_pairs = parse_data_urlencode(data_urlencode)?;
    let enc_joined = join_form_pairs(&enc_pairs);
    let appended = match (raw.is_empty(), enc_joined.is_empty()) {
        (true, _) => enc_joined,
        (_, true) => raw,
        _ => format!("{raw}&{enc_joined}"),
    };
    let new_query = match url.query() {
        Some(q) if !q.is_empty() => format!("{q}&{appended}"),
        _ => appended,
    };
    url.set_query(Some(&new_query));
    Ok(())
}

fn validate_body_size(len: usize) -> Result<()> {
    if len > MAX_BODY_SIZE {
        anyhow::bail!(TempoWalletError::BodyTooLarge(MAX_BODY_SIZE));
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
    use std::io::Read;

    if let Some(path) = data.strip_prefix('@') {
        if path == "-" {
            let mut buf = Vec::new();
            std::io::stdin()
                .read_to_end(&mut buf)
                .map_err(TempoWalletError::ReadStdin)?;
            validate_body_size(buf.len())?;
            Ok(buf)
        } else {
            let buf = std::fs::read(path).map_err(|e| TempoWalletError::ReadFile {
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

/// Resolve and join multiple `-d` data items with `&` separators.
fn resolve_and_join_data(data: &[String]) -> Result<Vec<u8>> {
    let mut combined = Vec::new();
    for item in data {
        let bytes = resolve_data(item)?;
        if !combined.is_empty() {
            combined.push(b'&');
        }
        combined.extend(bytes);
    }
    Ok(combined)
}

/// Reject a raw header string that exceeds the maximum allowed size.
pub(super) fn validate_header_size(header: &str) -> Result<()> {
    if header.len() > MAX_HEADER_SIZE {
        anyhow::bail!(TempoWalletError::HeaderTooLarge(MAX_HEADER_SIZE));
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
        let value: serde_json::Value = toon_format::decode_default(toon_data)
            .map_err(|e| anyhow::anyhow!("failed to decode TOON input: {e}"))?;
        Some(serde_json::to_string(&value)?.into_bytes())
    } else if let Some(json) = json {
        Some(json.as_bytes().to_vec())
    } else if !data.is_empty() {
        Some(resolve_and_join_data(data)?)
    } else {
        None
    };

    if let Some(ref b) = body {
        validate_body_size(b.len())?;
    }

    let method = match method {
        Some(m) => reqwest::Method::from_bytes(m.to_uppercase().as_bytes())
            .map_err(|_| anyhow::anyhow!("invalid HTTP method: {m}"))?,
        None => {
            if body.is_some() {
                reqwest::Method::POST
            } else {
                reqwest::Method::GET
            }
        }
    };

    Ok((method, body))
}

/// Parse --data-urlencode items into (name, value) tuples with URL-encoding applied.
pub(super) fn parse_data_urlencode(items: &[String]) -> Result<Vec<(Option<String>, String)>> {
    let mut pairs = Vec::new();
    for it in items {
        if let Some(rest) = it.strip_prefix('@') {
            // @filename — read file contents
            let content = std::fs::read(rest).map_err(|e| TempoWalletError::ReadFile {
                path: rest.to_string(),
                source: e,
            })?;
            let enc = urlencoding::encode_binary(&content).to_string();
            pairs.push((None, enc));
            continue;
        }
        if let Some(pos) = it.find("=@") {
            // name=@filename pattern (curl-style)
            let (name, file) = it.split_at(pos);
            let file = &file[2..];
            let content = std::fs::read(file).map_err(|e| TempoWalletError::ReadFile {
                path: file.to_string(),
                source: e,
            })?;
            let enc = urlencoding::encode_binary(&content).to_string();
            pairs.push((Some(name.to_string()), enc));
            continue;
        }
        if let Some((name, val)) = it.split_once('=') {
            pairs.push((Some(name.to_string()), urlencoding::encode(val).to_string()));
        } else {
            // raw value; encode as a nameless component
            pairs.push((None, urlencoding::encode(it).to_string()));
        }
    }
    Ok(pairs)
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
        let trimmed = data.trim();
        return trimmed.starts_with('{') || trimmed.starts_with('[');
    }
    false
}

/// Join parsed form pairs into a single query/form string.
///
/// Each pair is rendered as `name=value` (if named) or just `value`,
/// separated by `&`.
pub(super) fn join_form_pairs(pairs: &[(Option<String>, String)]) -> String {
    pairs
        .iter()
        .map(|(name, val)| match name {
            Some(n) => format!("{n}={val}"),
            None => val.clone(),
        })
        .collect::<Vec<_>>()
        .join("&")
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
    fn test_json_data_auto_detected() {
        let no_h: Vec<String> = vec![];
        let json_obj = vec![r#"{"key": "value"}"#.to_string()];
        let json_arr = vec!["[1, 2, 3]".to_string()];
        let json_ws = vec!["  {\"key\": \"value\"}".to_string()];
        let plain = vec!["plain text".to_string()];
        let kv = vec!["key=value".to_string()];
        assert!(should_auto_add_json_content_type(
            &no_h, None, None, &json_obj
        ));
        assert!(should_auto_add_json_content_type(
            &no_h, None, None, &json_arr
        ));
        assert!(should_auto_add_json_content_type(
            &no_h, None, None, &json_ws
        ));
        assert!(!should_auto_add_json_content_type(
            &no_h, None, None, &plain
        ));
        assert!(!should_auto_add_json_content_type(&no_h, None, None, &kv));
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
        let result = parse_data_urlencode(&items).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, Some("key".to_string()));
        assert_eq!(result[0].1, "hello%20world");
    }

    #[test]
    fn test_parse_data_urlencode_raw() {
        let items = vec!["hello world".to_string()];
        let result = parse_data_urlencode(&items).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, None);
        assert_eq!(result[0].1, "hello%20world");
    }

    #[test]
    fn test_parse_data_urlencode_file_not_found() {
        let items = vec!["@nonexistent_file_12345.txt".to_string()];
        let err = parse_data_urlencode(&items).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("failed to read file"), "got: {msg}");
    }

    #[test]
    fn test_parse_data_urlencode_named_file_not_found() {
        let items = vec!["field=@nonexistent_file_12345.txt".to_string()];
        let err = parse_data_urlencode(&items).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("failed to read file"), "got: {msg}");
    }
}
