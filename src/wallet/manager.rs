//! Wallet manager for orchestrating browser-based authentication.

use std::time::{Duration, Instant};

use alloy::primitives::Address;
use alloy::rlp::Decodable;
use alloy::signers::local::PrivateKeySigner;
use tempo_primitives::transaction::SignedKeyAuthorization;
use url::Url;

use crate::analytics::Analytics;
use crate::error::{PrestoError, Result};
use crate::wallet::auth_server::{run_callback_server, AuthCallback};
use crate::wallet::credentials::{NetworkWallet, WalletCredentials};
use crate::wallet::AccessKey;

const CALLBACK_TIMEOUT_SECS: u64 = 300; // 5 minutes

/// Orchestrates browser-based wallet authentication.
pub struct WalletManager {
    auth_server_url: String,
    network: String,
    analytics: Option<Analytics>,
}

impl WalletManager {
    /// Create a new wallet manager for a specific network.
    pub fn new(network: Option<&str>, analytics: Option<Analytics>) -> Self {
        let creds = WalletCredentials::load().unwrap_or_default();
        let network = network.unwrap_or(&creds.network).to_string();

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

    /// Get the base URL from the auth server URL.
    fn get_auth_base_url(&self) -> String {
        Url::parse(&self.auth_server_url)
            .map(|u| {
                let port = u.port().map(|p| format!(":{}", p)).unwrap_or_default();
                format!(
                    "{}://{}{}",
                    u.scheme(),
                    u.host_str().unwrap_or("localhost"),
                    port
                )
            })
            .unwrap_or_else(|_| self.auth_server_url.clone())
    }

    /// Open browser for wallet authentication.
    pub async fn setup_wallet(&self) -> Result<()> {
        let local_signer = PrivateKeySigner::random();
        let pub_key = format!("0x{}", hex::encode(local_signer.address()));

        let auth_base_url = self.get_auth_base_url();

        println!("Starting authentication server...");
        let (port, rx) = run_callback_server(auth_base_url).await?;

        let mut auth_url = Url::parse(&self.auth_server_url)
            .map_err(|e| PrestoError::Http(format!("Invalid auth server URL: {}", e)))?;

        auth_url
            .query_pairs_mut()
            .append_pair("port", &port.to_string())
            .append_pair("pub_key", &pub_key)
            .append_pair("key_type", "secp256k1");

        let url_str = auth_url.to_string();

        println!("Opening browser for authentication...");
        println!("If the browser doesn't open, visit: {}", url_str);

        if let Err(e) = webbrowser::open(&url_str) {
            eprintln!("Failed to open browser: {}", e);
            println!("Please open this URL manually: {}", url_str);
        }

        if let Some(ref a) = self.analytics {
            a.track(
                crate::analytics::Event::CallbackWindowOpened,
                crate::analytics::CallbackWindowOpenedPayload {
                    is_refresh: false,
                    network: self.network.clone(),
                },
            );
        }

        println!("\nWaiting for authentication...");
        let wait_start = Instant::now();

        let callback = tokio::time::timeout(Duration::from_secs(CALLBACK_TIMEOUT_SECS), rx)
            .await
            .map_err(|_| PrestoError::Http("Authentication timed out".to_string()))?
            .map_err(|_| {
                PrestoError::Http("Failed to receive authentication callback".to_string())
            })?;

        if let Some(ref a) = self.analytics {
            a.track(
                crate::analytics::Event::CallbackReceived,
                crate::analytics::CallbackReceivedPayload {
                    network: self.network.clone(),
                    duration_secs: wait_start.elapsed().as_secs(),
                },
            );
        }

        self.save_credentials(callback, local_signer).await?;

        println!("Wallet connected successfully!");
        Ok(())
    }

    /// Refresh access key for an existing wallet.
    pub async fn refresh_access_key(&self, account_address: &str) -> Result<()> {
        let local_signer = PrivateKeySigner::random();
        let pub_key = format!("0x{}", hex::encode(local_signer.address()));

        let auth_base_url = self.get_auth_base_url();

        println!("Starting authentication server...");
        let (port, rx) = run_callback_server(auth_base_url).await?;

        let mut auth_url = Url::parse(&self.auth_server_url)
            .map_err(|e| PrestoError::Http(format!("Invalid auth server URL: {}", e)))?;

        auth_url
            .query_pairs_mut()
            .append_pair("port", &port.to_string())
            .append_pair("account", account_address)
            .append_pair("pub_key", &pub_key)
            .append_pair("key_type", "secp256k1");

        let url_str = auth_url.to_string();

        println!("Opening browser to refresh access key...");
        println!("If the browser doesn't open, visit: {}", url_str);

        if let Err(e) = webbrowser::open(&url_str) {
            eprintln!("Failed to open browser: {}", e);
            println!("Please open this URL manually: {}", url_str);
        }

        if let Some(ref a) = self.analytics {
            a.track(
                crate::analytics::Event::CallbackWindowOpened,
                crate::analytics::CallbackWindowOpenedPayload {
                    is_refresh: true,
                    network: self.network.clone(),
                },
            );
        }

        println!("\nWaiting for authentication...");
        let wait_start = Instant::now();

        let callback = tokio::time::timeout(Duration::from_secs(CALLBACK_TIMEOUT_SECS), rx)
            .await
            .map_err(|_| PrestoError::Http("Authentication timed out".to_string()))?
            .map_err(|_| {
                PrestoError::Http("Failed to receive authentication callback".to_string())
            })?;

        if let Some(ref a) = self.analytics {
            a.track(
                crate::analytics::Event::CallbackReceived,
                crate::analytics::CallbackReceivedPayload {
                    network: self.network.clone(),
                    duration_secs: wait_start.elapsed().as_secs(),
                },
            );
        }

        self.save_access_key(callback, local_signer).await?;

        println!("Access key refreshed successfully!");
        Ok(())
    }

    /// Save authentication credentials.
    async fn save_credentials(
        &self,
        callback: AuthCallback,
        local_signer: PrivateKeySigner,
    ) -> Result<()> {
        let validated = validate_key_authorization(
            callback.key_authorization.as_deref(),
            local_signer.address(),
        )?;

        let mut creds = WalletCredentials::load()?;
        creds.network = self.network.clone();

        let private_key_hex = format!("0x{}", hex::encode(local_signer.to_bytes()));
        let mut access_key = AccessKey::new(private_key_hex).with_label("Default".to_string());
        let mut pending_hex = None;

        if let Some(v) = &validated {
            access_key = access_key.with_expiry(v.expiry);
            pending_hex = Some(v.hex.clone());
        }

        let mut wallet = NetworkWallet {
            account_address: callback.account_address,
            access_keys: vec![],
            active_key_index: 0,
            pending_key_authorization: pending_hex,
        };

        wallet.add_key(access_key, true);
        creds.set_wallet(wallet);
        creds.save()?;

        if let Some(ref a) = self.analytics {
            a.track(
                crate::analytics::Event::KeyCreated,
                crate::analytics::KeyCreatedPayload {
                    network: self.network.clone(),
                    label: "Default".to_string(),
                },
            );
            a.identify();
        }

        Ok(())
    }

    /// Save a new access key to an existing wallet.
    async fn save_access_key(
        &self,
        callback: AuthCallback,
        local_signer: PrivateKeySigner,
    ) -> Result<()> {
        let validated = validate_key_authorization(
            callback.key_authorization.as_deref(),
            local_signer.address(),
        )?;

        let mut creds = WalletCredentials::load()?;
        creds.network = self.network.clone();

        let private_key_hex = format!("0x{}", hex::encode(local_signer.to_bytes()));
        let key_label = format!("Key {}", chrono_label());
        let mut access_key = AccessKey::new(private_key_hex).with_label(key_label.clone());
        let mut pending_hex = None;

        if let Some(v) = &validated {
            access_key = access_key.with_expiry(v.expiry);
            pending_hex = Some(v.hex.clone());
        }

        if let Some(wallet) = creds.active_wallet_mut() {
            wallet.add_key(access_key, true);
            wallet.pending_key_authorization = pending_hex;
        } else {
            let mut wallet = NetworkWallet {
                account_address: callback.account_address,
                access_keys: vec![],
                active_key_index: 0,
                pending_key_authorization: pending_hex,
            };
            wallet.add_key(access_key, true);
            creds.set_wallet(wallet);
        }

        creds.save()?;

        if let Some(ref a) = self.analytics {
            a.track(
                crate::analytics::Event::KeyCreated,
                crate::analytics::KeyCreatedPayload {
                    network: self.network.clone(),
                    label: key_label,
                },
            );
            a.identify();
        }

        Ok(())
    }
}

#[derive(Debug, PartialEq)]
struct ValidatedKeyAuth {
    hex: String,
    expiry: u64,
}

fn validate_key_authorization(
    hex_str: Option<&str>,
    expected_key_id: Address,
) -> Result<Option<ValidatedKeyAuth>> {
    let hex_str = match hex_str {
        Some(s) => s,
        None => return Ok(None),
    };

    let raw = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    let bytes = hex::decode(raw)
        .map_err(|e| PrestoError::InvalidConfig(format!("Invalid key authorization hex: {}", e)))?;

    let mut slice = bytes.as_slice();
    let signed = SignedKeyAuthorization::decode(&mut slice)
        .map_err(|e| PrestoError::InvalidConfig(format!("Invalid key authorization RLP: {}", e)))?;

    if signed.authorization.key_id != expected_key_id {
        return Err(PrestoError::InvalidConfig(format!(
            "Key authorization targets {:#x}, expected {:#x}",
            signed.authorization.key_id, expected_key_id
        )));
    }

    let expiry = signed.authorization.expiry.unwrap_or(0);

    Ok(Some(ValidatedKeyAuth {
        hex: hex_str.to_string(),
        expiry,
    }))
}

fn chrono_label() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before UNIX epoch")
        .as_secs()
        .to_string()
}

impl Default for WalletManager {
    fn default() -> Self {
        Self::new(None, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::rlp::Encodable;
    use alloy::signers::SignerSync;
    use tempo_primitives::transaction::{KeyAuthorization, PrimitiveSignature, SignatureType};

    fn make_signed_auth_hex(key_id: Address) -> String {
        let signer: PrivateKeySigner =
            "0x1234567890123456789012345678901234567890123456789012345678901234"
                .parse()
                // ast-grep-ignore: no-unwrap-in-lib
                .unwrap();

        let auth = KeyAuthorization {
            chain_id: 42431,
            key_type: SignatureType::Secp256k1,
            key_id,
            expiry: Some(9999999999),
            limits: None,
        };

        // ast-grep-ignore: no-unwrap-in-lib
        let sig = signer.sign_hash_sync(&auth.signature_hash()).unwrap();
        let signed = auth.into_signed(PrimitiveSignature::Secp256k1(sig));

        let mut buf = Vec::new();
        signed.encode(&mut buf);
        format!("0x{}", hex::encode(&buf))
    }

    #[test]
    fn test_validate_key_authorization_matching_key_id() {
        let signer = PrivateKeySigner::random();
        let hex = make_signed_auth_hex(signer.address());
        let result = validate_key_authorization(Some(&hex), signer.address());
        assert!(result.is_ok());
        // ast-grep-ignore: no-unwrap-in-lib
        let validated = result.unwrap().unwrap();
        assert_eq!(validated.hex, hex);
        assert_eq!(validated.expiry, 9999999999);
    }

    #[test]
    fn test_validate_key_authorization_mismatched_key_id() {
        let signer = PrivateKeySigner::random();
        let wrong_address = Address::repeat_byte(0xFF);
        let hex = make_signed_auth_hex(wrong_address);
        let result = validate_key_authorization(Some(&hex), signer.address());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Key authorization targets"));
    }

    #[test]
    fn test_validate_key_authorization_none() {
        let result = validate_key_authorization(None, Address::ZERO);
        assert!(result.is_ok());
        // ast-grep-ignore: no-unwrap-in-lib
        assert_eq!(result.unwrap(), None);
    }

    #[test]
    fn test_validate_key_authorization_invalid_hex() {
        let result = validate_key_authorization(Some("not-hex"), Address::ZERO);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_key_authorization_invalid_rlp() {
        let result = validate_key_authorization(Some("0xdeadbeef"), Address::ZERO);
        assert!(result.is_err());
    }
}
