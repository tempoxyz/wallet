//! Login command — browser-based wallet authentication (device code + PKCE flow).

use std::io::IsTerminal;
use std::time::{Duration, Instant};

use alloy::signers::local::PrivateKeySigner;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use url::Url;
use zeroize::Zeroizing;

use super::whoami::{show_whoami, show_whoami_stderr};
use crate::analytics::{self, Analytics, Event};
use crate::cli::{Context, OutputFormat};
use crate::error::TempoWalletError;
use crate::keys::{Keystore, WalletType};
use crate::network::NetworkId;
use crate::util::sanitize_error;

const CALLBACK_TIMEOUT_SECS: u64 = 900; // 15 minutes
const POLL_INTERVAL_SECS: u64 = 2;

// ==================== Command Entry Point ====================

pub(crate) async fn run(ctx: &Context) -> anyhow::Result<()> {
    let net_str = ctx.network.as_str().to_string();
    if let Some(ref a) = ctx.analytics {
        a.track(
            Event::LoginStarted,
            analytics::NetworkPayload {
                network: net_str.clone(),
            },
        );
    }

    let result = run_impl(ctx).await;

    if let Some(ref a) = ctx.analytics {
        match &result {
            Ok(()) => {
                a.track(
                    Event::LoginSuccess,
                    analytics::NetworkPayload { network: net_str },
                );
            }
            Err(e) => {
                let err_str = e.to_string();
                let is_timeout = e.chain().any(|cause| {
                    matches!(cause.downcast_ref(), Some(TempoWalletError::LoginExpired))
                });

                if is_timeout {
                    a.track(
                        Event::LoginTimeout,
                        analytics::NetworkPayload { network: net_str },
                    );
                } else {
                    a.track(
                        Event::LoginFailure,
                        analytics::LoginFailurePayload {
                            network: net_str,
                            error: sanitize_error(&err_str),
                        },
                    );
                }
            }
        }
    }

    result
}

async fn run_impl(ctx: &Context) -> anyhow::Result<()> {
    // Skip login if a wallet is already connected with a key for the target network.
    if ctx.keys.has_wallet() {
        let has_key_for_network = ctx
            .keys
            .keys
            .iter()
            .any(|k| k.chain_id == ctx.network.chain_id());

        if has_key_for_network {
            if ctx.output_format == OutputFormat::Text {
                println!("Already logged in.\n");
            }

            show_whoami(&ctx.config, ctx.output_format, ctx.network, None, &ctx.keys).await?;
            return Ok(());
        }
    }

    let wallet_address = setup_wallet(ctx.network, ctx.analytics.as_ref(), &ctx.keys).await?;

    // Ensure a config file exists so the user has something to edit.
    let _ = ctx.config.save();

    let fresh_keys = ctx.keys.reload()?;
    if ctx.output_format == OutputFormat::Text {
        eprintln!("\nWallet connected!\n");
        show_whoami_stderr(&ctx.config, ctx.network, Some(&wallet_address), &fresh_keys).await?;
    } else {
        show_whoami(
            &ctx.config,
            ctx.output_format,
            ctx.network,
            Some(&wallet_address),
            &fresh_keys,
        )
        .await?;
    }

    Ok(())
}

// ==================== Passkey Authentication ====================

struct AuthCallback {
    account_address: String,
    key_authorization: Option<String>,
}

/// Run the browser-based device code + PKCE authentication flow.
///
/// Generates a local signing key, opens the browser for wallet authentication,
/// polls until the user approves, and saves the resulting key authorization.
async fn setup_wallet(
    network: NetworkId,
    analytics: Option<&Analytics>,
    keys: &Keystore,
) -> Result<String, TempoWalletError> {
    let auth_server_url = std::env::var("TEMPO_AUTH_URL")
        .unwrap_or_else(|_| network.auth_url().to_string());

    let parsed_url = Url::parse(&auth_server_url)
        .map_err(|e| TempoWalletError::InvalidUrl(format!("Invalid auth server URL: {}", e)))?;
    let auth_base_url = parsed_url.origin().ascii_serialization();

    let local_signer = PrivateKeySigner::random();
    let uncompressed = local_signer
        .credential()
        .verifying_key()
        .to_encoded_point(false);
    let pub_key = format!("0x{}", hex::encode(uncompressed.as_bytes()));

    let code_verifier = generate_code_verifier();
    let code_challenge = compute_code_challenge(&code_verifier);

    let client = reqwest::Client::new();
    let device_code_resp = create_device_code(
        &client,
        &auth_base_url,
        &pub_key,
        "secp256k1",
        &code_challenge,
    )
    .await?;

    let code = &device_code_resp.code;

    let mut auth_url = parsed_url;
    auth_url.query_pairs_mut().append_pair("code", code);
    let url_str = auth_url.to_string();

    eprintln!();
    let display_code = if code.len() == 8 {
        format!("{}-{}", &code[..4], &code[4..])
    } else {
        code.to_string()
    };
    eprintln!("Verification code: \x1b[1m{}\x1b[0m", display_code);

    if std::io::stdin().is_terminal() {
        eprint!(
            "\x1b[1mPress Enter\x1b[0m to open your browser to {}... ",
            url_str
        );
        std::io::Write::flush(&mut std::io::stderr()).ok();
        let browser_url = url_str.clone();
        // Using a plain thread so it won't prevent process exit after auth completes.
        std::thread::spawn(move || {
            let _ = std::io::stdin().read_line(&mut String::new());
            if let Err(e) = webbrowser::open(&browser_url) {
                eprintln!("Failed to open browser: {}", e);
                eprintln!("Please open this URL manually: {}", browser_url);
            }
        });
    } else {
        eprintln!("Opening browser to {}...", url_str);
        if let Err(e) = webbrowser::open(&url_str) {
            eprintln!("Failed to open browser: {}", e);
            eprintln!("Please open this URL manually: {}", url_str);
        }
    }

    if let Some(a) = analytics {
        a.track(
            Event::CallbackWindowOpened,
            analytics::NetworkPayload {
                network: network.to_string(),
            },
        );
    }

    eprintln!("Waiting for authentication...");
    let wait_start = Instant::now();
    let timeout = Duration::from_secs(CALLBACK_TIMEOUT_SECS);

    let callback = loop {
        if wait_start.elapsed() >= timeout {
            return Err(TempoWalletError::LoginExpired);
        }

        let poll_resp = poll_device_code(&client, &auth_base_url, code, &code_verifier).await?;

        if let Some(err) = &poll_resp.error {
            if err.to_lowercase().contains("expired") {
                return Err(TempoWalletError::LoginExpired);
            }
            return Err(TempoWalletError::Http(err.clone()));
        }

        if poll_resp.status == "authorized" {
            break AuthCallback {
                account_address: poll_resp.account_address.ok_or_else(|| {
                    TempoWalletError::Http(
                        "Missing account_address in authorized response".to_string(),
                    )
                })?,
                key_authorization: poll_resp.key_authorization,
            };
        }

        tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
    };

    if let Some(a) = analytics {
        a.track(
            Event::CallbackReceived,
            analytics::CallbackReceivedPayload {
                network: network.to_string(),
                duration_secs: wait_start.elapsed().as_secs(),
            },
        );
    }

    save_keys(network, analytics, keys, callback, local_signer)
}

// ==================== Key Persistence ====================

/// Save authentication keys to keys.toml (NOT in the OS keychain).
fn save_keys(
    network: NetworkId,
    analytics: Option<&Analytics>,
    keys: &Keystore,
    callback: AuthCallback,
    local_signer: PrivateKeySigner,
) -> Result<String, TempoWalletError> {
    let validated = crate::keys::authorization::validate(
        callback.key_authorization.as_deref(),
        local_signer.address(),
    )?;
    let key_auth_hex = validated.as_ref().map(|v| v.hex.clone());

    let access_key_hex = Zeroizing::new(format!("0x{}", hex::encode(local_signer.to_bytes())));
    let access_key_address = local_signer.address().to_string();

    // Clone to mutate and save; preserves other accounts.
    let mut keys = keys.clone();

    // Resolve the chain_id before upserting so we can key by (wallet, chain).
    // Use the chain_id from the key authorization if present and non-zero,
    // otherwise fall back to the network the user requested.
    let default_chain_id = network.chain_id();
    let chain_id = validated
        .as_ref()
        .and_then(|v| (v.chain_id != 0).then_some(v.chain_id))
        .unwrap_or(default_chain_id);

    let entry = keys.upsert_by_wallet_and_chain(&callback.account_address, chain_id);
    let wallet_address_result = callback.account_address.clone();

    // Only preserve provisioned state when key is unchanged.
    let keep_provisioned = {
        let same_key = entry
            .key_address
            .as_deref()
            .is_some_and(|a| a == access_key_address);
        same_key && entry.provisioned
    };

    let (key_type, expiry, token_limits) = if let Some(ref v) = validated {
        (v.key_type.clone(), Some(v.expiry), v.limits.clone())
    } else {
        (entry.key_type.clone(), entry.expiry, entry.limits.clone())
    };

    entry.wallet_type = WalletType::Passkey;
    entry.wallet_address = callback.account_address;
    entry.key_type = key_type;
    entry.key_address = Some(access_key_address);
    entry.key = Some(access_key_hex);
    entry.key_authorization = key_auth_hex;
    entry.expiry = expiry;
    entry.limits = token_limits;
    entry.provisioned = keep_provisioned;

    keys.save()?;

    if let Some(a) = analytics {
        a.track(
            Event::WalletCreated,
            analytics::WalletCreatedPayload {
                network: network.to_string(),
                wallet_type: "passkey".to_string(),
            },
        );
        a.track(
            Event::KeyCreated,
            analytics::NetworkPayload {
                network: network.to_string(),
            },
        );
        a.identify(&keys);
    }

    Ok(wallet_address_result)
}

// ==================== Device Code API ====================

#[derive(Debug, Deserialize)]
struct DeviceCodeResponse {
    code: String,
}

#[derive(Debug, Deserialize)]
struct PollResponse {
    status: String,
    account_address: Option<String>,
    key_authorization: Option<String>,
    error: Option<String>,
}

async fn create_device_code(
    client: &reqwest::Client,
    base_url: &str,
    pub_key: &str,
    key_type: &str,
    code_challenge: &str,
) -> Result<DeviceCodeResponse, TempoWalletError> {
    let url = format!("{}/cli-auth/device-code", base_url);
    let resp = client
        .post(&url)
        .json(&serde_json::json!({
            "pub_key": pub_key,
            "key_type": key_type,
            "code_challenge": code_challenge,
        }))
        .send()
        .await
        .map_err(|e| TempoWalletError::Http(format!("Failed to create device code: {}", e)))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(TempoWalletError::Http(format!(
            "Device code request failed ({}): {}",
            status, body
        )));
    }

    resp.json::<DeviceCodeResponse>()
        .await
        .map_err(|e| TempoWalletError::Http(format!("Failed to parse device code response: {}", e)))
}

async fn poll_device_code(
    client: &reqwest::Client,
    base_url: &str,
    code: &str,
    code_verifier: &str,
) -> Result<PollResponse, TempoWalletError> {
    let url = format!("{}/cli-auth/poll/{}", base_url, code);
    let resp = client
        .post(&url)
        .json(&serde_json::json!({
            "code_verifier": code_verifier,
        }))
        .send()
        .await
        .map_err(|e| TempoWalletError::Http(format!("Failed to poll device code: {}", e)))?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(TempoWalletError::LoginExpired);
    }

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(TempoWalletError::Http(format!(
            "Poll request failed ({}): {}",
            status, body
        )));
    }

    resp.json::<PollResponse>()
        .await
        .map_err(|e| TempoWalletError::Http(format!("Failed to parse poll response: {}", e)))
}

// ==================== PKCE ====================

fn generate_code_verifier() -> String {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).expect("failed to generate random bytes");
    // Truncate to 43 chars: PKCE spec (RFC 7636 §4.1) requires 43–128 unreserved characters.
    hex::encode(bytes)[..43].to_string()
}

fn compute_code_challenge(code_verifier: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(code_verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(hasher.finalize())
}

// ==================== Tests ====================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_code_challenge_produces_43_char_base64url() {
        let verifier = "test-code-verifier-12345678901234567890";
        let challenge = compute_code_challenge(verifier);
        assert_eq!(challenge.len(), 43);
        assert!(challenge
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn test_code_challenge_is_deterministic() {
        let verifier = "deterministic-verifier";
        let c1 = compute_code_challenge(verifier);
        let c2 = compute_code_challenge(verifier);
        assert_eq!(c1, c2);
    }

    #[test]
    fn test_different_inputs_produce_different_outputs() {
        let c1 = compute_code_challenge("input-a");
        let c2 = compute_code_challenge("input-b");
        assert_ne!(c1, c2);
    }

    #[test]
    fn test_generate_code_verifier_length() {
        let verifier = generate_code_verifier();
        assert_eq!(verifier.len(), 43);
    }

    #[test]
    fn test_generate_code_verifier_is_hex() {
        let verifier = generate_code_verifier();
        assert!(verifier.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
