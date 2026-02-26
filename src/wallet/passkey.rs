//! Browser-based wallet authentication (device code + PKCE flow).

use std::time::{Duration, Instant};

use alloy::signers::local::PrivateKeySigner;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use url::Url;

use zeroize::Zeroizing;

use crate::analytics::{
    Analytics, CallbackReceivedPayload, CallbackWindowOpenedPayload, Event, KeyCreatedPayload,
};
use crate::error::PrestoError;
use crate::network::networks::network_or_default;
use crate::wallet::credentials::{WalletCredentials, WalletType};

const CALLBACK_TIMEOUT_SECS: u64 = 900; // 15 minutes
const POLL_INTERVAL_SECS: u64 = 2;

#[derive(Debug, Clone)]
struct AuthCallback {
    pub account_address: String,
    pub key_authorization: Option<String>,
}

/// Orchestrates browser-based wallet authentication.
pub struct WalletManager {
    auth_server_url: String,
    network: String,
    analytics: Option<Analytics>,
}

impl WalletManager {
    /// Create a new wallet manager for a specific network.
    pub fn new(network: Option<&str>, analytics: Option<Analytics>) -> Self {
        let network = network_or_default(network).to_string();

        let auth_server_url = std::env::var("PRESTO_AUTH_URL")
            .ok()
            .unwrap_or_else(|| Self::auth_url_for_network(&network));

        Self {
            auth_server_url,
            network,
            analytics,
        }
    }

    /// Get the auth server URL for a given network.
    fn auth_url_for_network(network: &str) -> String {
        match network {
            "tempo" => "https://app.tempo.xyz/cli-auth".to_string(),
            _ => "https://app.moderato.tempo.xyz/cli-auth".to_string(),
        }
    }

    /// Get the base URL (origin) from the auth server URL.
    fn get_auth_base_url(&self) -> String {
        Url::parse(&self.auth_server_url)
            .map(|u| u.origin().ascii_serialization())
            .unwrap_or_else(|_| self.auth_server_url.clone())
    }

    /// Open browser for wallet authentication.
    pub async fn setup_wallet(&self) -> Result<(), PrestoError> {
        let local_signer = PrivateKeySigner::random();
        let uncompressed = local_signer
            .credential()
            .verifying_key()
            .to_encoded_point(false);
        let pub_key = format!("0x{}", hex::encode(uncompressed.as_bytes()));

        let auth_base_url = self.get_auth_base_url();

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

        let mut auth_url = Url::parse(&self.auth_server_url)
            .map_err(|e| PrestoError::Http(format!("Invalid auth server URL: {}", e)))?;

        auth_url.query_pairs_mut().append_pair("code", code);

        let url_str = auth_url.to_string();

        eprintln!();
        let display_code = if code.len() == 8 {
            format!("{}-{}", &code[..4], &code[4..])
        } else {
            code.to_string()
        };
        eprintln!("Verification code: \x1b[1m{}\x1b[0m", display_code);

        use std::io::IsTerminal;
        if std::io::stdin().is_terminal() {
            eprint!(
                "\x1b[1mPress Enter\x1b[0m to open your browser to {}... ",
                url_str
            );
            std::io::Write::flush(&mut std::io::stderr()).ok();
            tokio::task::spawn_blocking(|| {
                let _ = std::io::stdin().read_line(&mut String::new());
            })
            .await
            .ok();
        } else {
            eprintln!("Opening browser to {}...", url_str);
        }

        if let Err(e) = webbrowser::open(&url_str) {
            eprintln!("Failed to open browser: {}", e);
            eprintln!("Please open this URL manually: {}", url_str);
        }

        if let Some(ref a) = self.analytics {
            a.track(
                Event::CallbackWindowOpened,
                CallbackWindowOpenedPayload {
                    network: self.network.clone(),
                },
            );
        }

        eprintln!("Waiting for authentication...");
        let wait_start = Instant::now();
        let timeout = Duration::from_secs(CALLBACK_TIMEOUT_SECS);

        let callback = loop {
            if wait_start.elapsed() >= timeout {
                return Err(PrestoError::LoginExpired);
            }

            let poll_resp = poll_device_code(&client, &auth_base_url, code, &code_verifier).await?;

            if let Some(err) = &poll_resp.error {
                if err.to_lowercase().contains("expired") {
                    return Err(PrestoError::LoginExpired);
                }
                return Err(PrestoError::Http(err.clone()));
            }

            if poll_resp.status == "authorized" {
                break AuthCallback {
                    account_address: poll_resp.account_address.ok_or_else(|| {
                        PrestoError::Http(
                            "Missing account_address in authorized response".to_string(),
                        )
                    })?,
                    key_authorization: poll_resp.key_authorization,
                };
            }

            tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
        };

        if let Some(ref a) = self.analytics {
            a.track(
                Event::CallbackReceived,
                CallbackReceivedPayload {
                    network: self.network.clone(),
                    duration_secs: wait_start.elapsed().as_secs(),
                },
            );
        }

        self.save_credentials(callback, local_signer).await?;

        Ok(())
    }

    /// Save authentication credentials.
    ///
    /// Stores the key inline in keys.toml (NOT in the OS keychain).
    async fn save_credentials(
        &self,
        callback: AuthCallback,
        local_signer: PrivateKeySigner,
    ) -> Result<(), PrestoError> {
        let validated = super::key_authorization::validate(
            callback.key_authorization.as_deref(),
            local_signer.address(),
        )?;
        let key_auth_hex = validated.as_ref().map(|v| v.hex.clone());

        let access_key_hex = format!("0x{}", hex::encode(local_signer.to_bytes()));
        let access_key_address = local_signer.address().to_string();

        // Load existing credentials to preserve other accounts.
        // If the file is corrupt, surface the error instead of silently resetting.
        let mut creds = WalletCredentials::load()?;

        let entry = creds.upsert_by_wallet_address(&callback.account_address);

        // Only preserve provisioned state when key and chain are unchanged.
        let keep_provisioned = {
            let same_key = entry
                .key_address
                .as_deref()
                .is_some_and(|a| a == access_key_address);
            let same_chain = validated
                .as_ref()
                .is_none_or(|v| v.chain_id == entry.chain_id);
            same_key && same_chain && entry.provisioned
        };

        // When no new authorization was received, preserve existing chain metadata.
        let (chain_id, key_type, expiry, token_limits) = if let Some(ref v) = validated {
            (
                v.chain_id,
                v.key_type.clone(),
                Some(v.expiry),
                v.token_limits.clone(),
            )
        } else {
            (
                entry.chain_id,
                entry.key_type.clone(),
                entry.expiry,
                entry.token_limits.clone(),
            )
        };

        entry.wallet_type = WalletType::Passkey;
        entry.wallet_address = callback.account_address;
        entry.chain_id = chain_id;
        entry.key_type = key_type;
        entry.key_address = Some(access_key_address);
        entry.key = Some(Zeroizing::new(access_key_hex));
        entry.key_authorization = key_auth_hex;
        entry.expiry = expiry;
        entry.token_limits = token_limits;
        entry.provisioned = keep_provisioned;

        creds.save()?;

        if let Some(ref a) = self.analytics {
            a.track(
                Event::KeyCreated,
                KeyCreatedPayload {
                    network: self.network.clone(),
                },
            );
            a.identify();
        }

        Ok(())
    }
}

// ==================== Device Code ====================

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
) -> Result<DeviceCodeResponse, PrestoError> {
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
        .map_err(|e| PrestoError::Http(format!("Failed to create device code: {}", e)))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(PrestoError::Http(format!(
            "Device code request failed ({}): {}",
            status, body
        )));
    }

    resp.json::<DeviceCodeResponse>()
        .await
        .map_err(|e| PrestoError::Http(format!("Failed to parse device code response: {}", e)))
}

async fn poll_device_code(
    client: &reqwest::Client,
    base_url: &str,
    code: &str,
    code_verifier: &str,
) -> Result<PollResponse, PrestoError> {
    let url = format!("{}/cli-auth/poll/{}", base_url, code);
    let resp = client
        .post(&url)
        .json(&serde_json::json!({
            "code_verifier": code_verifier,
        }))
        .send()
        .await
        .map_err(|e| PrestoError::Http(format!("Failed to poll device code: {}", e)))?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(PrestoError::LoginExpired);
    }

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(PrestoError::Http(format!(
            "Poll request failed ({}): {}",
            status, body
        )));
    }

    resp.json::<PollResponse>()
        .await
        .map_err(|e| PrestoError::Http(format!("Failed to parse poll response: {}", e)))
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
