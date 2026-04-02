use reqwest::header::{ACCEPT, CONTENT_TYPE, COOKIE};
use serde_json::{json, Value};

use crate::error::{ConfigError, NetworkError, TempoError};

const SERVER_FN_BASE_PATH: &str = "/_serverFn/";

pub fn origin_from_auth_url(auth_url: &str) -> Result<String, TempoError> {
    let parsed = reqwest::Url::parse(auth_url).map_err(|source| ConfigError::InvalidUrl {
        context: "auth server",
        source,
    })?;

    Ok(parsed.origin().ascii_serialization())
}

pub fn server_fn_url(origin: &str, function_id: &str) -> Result<reqwest::Url, TempoError> {
    if function_id.is_empty() {
        return Err(ConfigError::Invalid("server function id cannot be empty".to_string()).into());
    }

    let base = reqwest::Url::parse(origin).map_err(|source| ConfigError::InvalidUrl {
        context: "server function origin",
        source,
    })?;

    base.join(&format!("{SERVER_FN_BASE_PATH}{function_id}"))
        .map_err(|source| {
            ConfigError::InvalidUrl {
                context: "server function",
                source,
            }
            .into()
        })
}

pub async fn call_json(
    client: &reqwest::Client,
    origin: &str,
    function_id: &str,
    data: &Value,
    session_token: Option<&str>,
) -> Result<Value, TempoError> {
    let url = server_fn_url(origin, function_id)?;

    let mut request = client
        .post(url)
        .header("x-tsr-serverFn", "true")
        .header(ACCEPT, "application/json")
        .header(CONTENT_TYPE, "application/json")
        .json(&json!({ "data": data }));

    if let Some(token) = session_token {
        request = request.header(COOKIE, format!("session={token}"));
    }

    let response = request.send().await.map_err(NetworkError::Reqwest)?;
    let status = response.status();
    let body = response.text().await.map_err(NetworkError::Reqwest)?;

    if !status.is_success() {
        return Err(NetworkError::HttpStatus {
            operation: "call app server function",
            status: status.as_u16(),
            body: Some(body),
        }
        .into());
    }

    serde_json::from_str::<Value>(&body)
        .map_err(|source| NetworkError::ResponseParse {
            context: "app server function response",
            source,
        })
        .map_err(TempoError::from)
}
