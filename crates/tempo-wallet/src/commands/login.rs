//! Login command — browser-based wallet authentication (device code + PKCE flow).

use std::time::{Duration, Instant};

use alloy::primitives::Address;
use alloy::signers::local::PrivateKeySigner;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use colored::Colorize;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use url::Url;
use zeroize::Zeroizing;

use super::whoami::show_whoami;
use crate::analytics::{self, CallbackReceivedPayload, LoginFailurePayload, WalletCreatedPayload};
use tempo_common::cli::context::Context;
use tempo_common::cli::output::OutputFormat;
use tempo_common::error::{ConfigError, InputError, KeyError, NetworkError, TempoError};
use tempo_common::keys::{Keystore, WalletType};
use tempo_common::network::NetworkId;
use tempo_common::security::sanitize_error;

const CALLBACK_TIMEOUT_SECS: u64 = 900; // 15 minutes
const POLL_INTERVAL_SECS: u64 = 2;

pub(crate) async fn run(ctx: &Context) -> Result<(), TempoError> {
    ctx.track_event(analytics::LOGIN_STARTED);

    let already_logged_in = ctx.keys.has_key_for_network(ctx.network);

    if !already_logged_in {
        let result = do_login(ctx).await;

        if let Some(ref a) = ctx.analytics {
            track_login_result(a, &result);
        }
        result?;
    }

    if ctx.output_format == OutputFormat::Text {
        let msg = if already_logged_in {
            "Already logged in.\n"
        } else {
            "\nWallet connected!\n"
        };
        eprintln!("{msg}");
    }

    let keys = ctx.keys.reload()?;
    show_whoami(ctx, Some(&keys), None).await
}

fn track_login_result(a: &tempo_common::analytics::Analytics, result: &Result<(), TempoError>) {
    match result {
        Ok(_) => a.track_event(analytics::LOGIN_SUCCESS),
        Err(e) => {
            let is_timeout = matches!(e, TempoError::Key(KeyError::LoginExpired));
            if is_timeout {
                a.track_event(analytics::LOGIN_TIMEOUT);
            } else {
                a.track(
                    analytics::LOGIN_FAILURE,
                    LoginFailurePayload {
                        error: sanitize_error(&e.to_string()),
                    },
                );
            }
        }
    }
}

async fn do_login(ctx: &Context) -> Result<(), TempoError> {
    let auth_server_url =
        std::env::var("TEMPO_AUTH_URL").unwrap_or_else(|_| ctx.network.auth_url().to_string());

    let parsed_url = Url::parse(&auth_server_url).map_err(|source| InputError::UrlParseFor {
        context: "auth server",
        source,
    })?;
    let auth_base_url = parsed_url.origin().ascii_serialization();

    let local_signer = PrivateKeySigner::random();
    let uncompressed = local_signer
        .credential()
        .verifying_key()
        .to_encoded_point(false);
    let pub_key = format!("0x{}", hex::encode(uncompressed.as_bytes()));

    let (code_verifier, code_challenge) = generate_pkce_pair()?;

    let client = reqwest::Client::builder()
        .build()
        .map_err(NetworkError::Reqwest)?;
    let code = create_device_code(&client, &auth_base_url, &pub_key, &code_challenge).await?;

    let mut auth_url = parsed_url;
    auth_url.query_pairs_mut().append_pair("code", &code);
    let url_str = auth_url.to_string();

    if ctx.output_format == OutputFormat::Text {
        prompt_and_open_browser(&code, &url_str);
    }

    ctx.track_event(analytics::CALLBACK_WINDOW_OPENED);

    let callback = poll_until_authorized(&client, &auth_base_url, &code, &code_verifier).await?;

    ctx.track(
        analytics::CALLBACK_RECEIVED,
        CallbackReceivedPayload {
            duration_secs: callback.duration_secs,
        },
    );

    save_keys(ctx.network, &ctx.keys, callback, local_signer)?;

    ctx.track(
        analytics::WALLET_CREATED,
        WalletCreatedPayload {
            wallet_type: "passkey".to_string(),
        },
    );
    ctx.track_event(analytics::KEY_CREATED);
    if let Some(ref a) = ctx.analytics {
        a.identify(&ctx.keys);
    }

    Ok(())
}

/// Display the verification code and open the browser for authentication.
fn prompt_and_open_browser(code: &str, url: &str) {
    let display_code = if code.len() == 8 {
        format!("{}-{}", &code[..4], &code[4..])
    } else {
        code.to_string()
    };
    eprintln!();
    eprintln!("Verification code: {}", display_code.bold());
    eprintln!();

    try_open_browser(url);

    eprintln!("Waiting for authentication...");
}

fn try_open_browser(url: &str) {
    if let Err(e) = webbrowser::open(url) {
        eprintln!("Failed to open browser: {}", e);
        eprintln!("Please open this URL manually: {}", url);
    }
}

struct AuthCallback {
    account_address: String,
    key_authorization: Option<String>,
    duration_secs: u64,
}

/// Poll the auth server until the user approves or the timeout expires.
async fn poll_until_authorized(
    client: &reqwest::Client,
    base_url: &str,
    code: &str,
    code_verifier: &str,
) -> Result<AuthCallback, TempoError> {
    let start = Instant::now();
    let timeout = Duration::from_secs(CALLBACK_TIMEOUT_SECS);

    loop {
        if start.elapsed() >= timeout {
            return Err(KeyError::LoginExpired.into());
        }

        let resp = poll_device_code(client, base_url, code, code_verifier).await?;

        if let Some(err) = &resp.error {
            if err.to_lowercase().contains("expired") {
                return Err(KeyError::LoginExpired.into());
            }
            // Intentional server-provided reason passthrough: the poll response only
            // contains a string error field (no structured source object).
            return Err(NetworkError::ResponseSchema {
                context: "login poll response",
                reason: err.clone(),
            }
            .into());
        }

        if resp.status == PollStatus::Authorized {
            return Ok(AuthCallback {
                account_address: resp
                    .account_address
                    .ok_or_else(|| TempoError::from(InputError::MissingAuthorizedAccountAddress))?,
                key_authorization: resp.key_authorization,
                duration_secs: start.elapsed().as_secs(),
            });
        }

        tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
    }
}

/// Save authentication keys to keys.toml (NOT in the OS keychain).
fn save_keys(
    network: NetworkId,
    keys: &Keystore,
    callback: AuthCallback,
    local_signer: PrivateKeySigner,
) -> Result<(), TempoError> {
    let wallet_address: Address =
        callback
            .account_address
            .parse()
            .map_err(|_| ConfigError::InvalidAddress {
                context: "authorized response account_address",
                value: callback.account_address.clone(),
            })?;

    let validated = tempo_common::keys::authorization::validate(
        callback.key_authorization.as_deref(),
        local_signer.address(),
    )?;
    let key_auth_hex = validated.as_ref().map(|v| v.hex.clone());

    let access_key_hex = Zeroizing::new(format!("0x{}", hex::encode(local_signer.to_bytes())));
    let access_key_address = local_signer.address();

    let mut keys = keys.clone();

    let default_chain_id = network.chain_id();
    let chain_id = validated
        .as_ref()
        .and_then(|v| (v.chain_id != 0).then_some(v.chain_id))
        .unwrap_or(default_chain_id);

    let entry = keys.upsert_by_wallet_address_and_chain(wallet_address, chain_id);

    let keep_provisioned = entry.key_address_matches(access_key_address) && entry.provisioned;

    if let Some(ref v) = validated {
        entry.key_type = v.key_type;
        entry.expiry = Some(v.expiry);
        entry.limits = v.limits.clone();
    }

    entry.wallet_type = WalletType::Passkey;
    entry.set_wallet_address(wallet_address);
    entry.set_key_address(Some(access_key_address));
    entry.key = Some(access_key_hex);
    entry.key_authorization = key_auth_hex;
    entry.provisioned = keep_provisioned;

    keys.save()
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum PollStatus {
    Authorized,
    #[serde(other)]
    Pending,
}

#[derive(Debug, Deserialize)]
struct PollResponse {
    status: PollStatus,
    account_address: Option<String>,
    key_authorization: Option<String>,
    error: Option<String>,
}

async fn create_device_code(
    client: &reqwest::Client,
    base_url: &str,
    pub_key: &str,
    code_challenge: &str,
) -> Result<String, TempoError> {
    let url = format!("{}/cli-auth/device-code", base_url);
    let resp = client
        .post(&url)
        .json(&serde_json::json!({
            "pub_key": pub_key,
            "key_type": "secp256k1",
            "code_challenge": code_challenge,
        }))
        .send()
        .await
        .map_err(NetworkError::Reqwest)?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.map_err(NetworkError::Reqwest)?;
        return Err(NetworkError::HttpStatus {
            operation: "request device code",
            status: status.as_u16(),
            body: Some(body),
        }
        .into());
    }

    #[derive(Deserialize)]
    struct DeviceCodeResponse {
        code: String,
    }

    let body = resp.text().await.map_err(NetworkError::Reqwest)?;
    serde_json::from_str::<DeviceCodeResponse>(&body)
        .map(|r| r.code)
        .map_err(|source| NetworkError::ResponseParse {
            context: "login device code response",
            source,
        })
        .map_err(TempoError::from)
}

async fn poll_device_code(
    client: &reqwest::Client,
    base_url: &str,
    code: &str,
    code_verifier: &str,
) -> Result<PollResponse, TempoError> {
    let url = format!("{}/cli-auth/poll/{}", base_url, code);
    let resp = client
        .post(&url)
        .json(&serde_json::json!({
            "code_verifier": code_verifier,
        }))
        .send()
        .await
        .map_err(NetworkError::Reqwest)?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(KeyError::LoginExpired.into());
    }

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.map_err(NetworkError::Reqwest)?;
        return Err(NetworkError::HttpStatus {
            operation: "poll login status",
            status: status.as_u16(),
            body: Some(body),
        }
        .into());
    }

    let body = resp.text().await.map_err(NetworkError::Reqwest)?;
    serde_json::from_str::<PollResponse>(&body)
        .map_err(|source| NetworkError::ResponseParse {
            context: "login poll response",
            source,
        })
        .map_err(TempoError::from)
}

fn generate_pkce_pair() -> Result<(String, String), TempoError> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|source| KeyError::SigningOperationSource {
        operation: "generate PKCE verifier",
        source: Box::new(source),
    })?;
    let verifier = URL_SAFE_NO_PAD.encode(bytes);

    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let challenge = URL_SAFE_NO_PAD.encode(hasher.finalize());

    Ok((verifier, challenge))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pkce_pair_lengths() {
        let (verifier, challenge) = generate_pkce_pair().expect("pkce generation should succeed");
        assert_eq!(verifier.len(), 43);
        assert_eq!(challenge.len(), 43);
    }

    #[test]
    fn test_pkce_pair_is_base64url() {
        let (verifier, challenge) = generate_pkce_pair().expect("pkce generation should succeed");
        let is_base64url = |s: &str| {
            s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        };
        assert!(is_base64url(&verifier));
        assert!(is_base64url(&challenge));
    }

    #[test]
    fn test_pkce_challenge_is_deterministic() {
        let mut hasher = Sha256::new();
        hasher.update(b"deterministic-verifier");
        let c1 = URL_SAFE_NO_PAD.encode(hasher.finalize());

        let mut hasher = Sha256::new();
        hasher.update(b"deterministic-verifier");
        let c2 = URL_SAFE_NO_PAD.encode(hasher.finalize());

        assert_eq!(c1, c2);
    }

    #[test]
    fn test_pkce_pairs_are_unique() {
        let (v1, _) = generate_pkce_pair().expect("pkce generation should succeed");
        let (v2, _) = generate_pkce_pair().expect("pkce generation should succeed");
        assert_ne!(v1, v2);
    }
}
