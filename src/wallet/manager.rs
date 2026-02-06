//! Wallet manager for orchestrating browser-based authentication.

use std::time::Duration;

use alloy::signers::local::PrivateKeySigner;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use url::Url;

use crate::error::{PgetError, Result};
use crate::wallet::auth_server::{run_callback_server, AuthCallback};
use crate::wallet::credentials::{NetworkWallet, WalletCredentials};
use crate::wallet::AccessKey;

const CALLBACK_TIMEOUT_SECS: u64 = 300; // 5 minutes

/// Orchestrates browser-based wallet authentication.
pub struct WalletManager {
    auth_server_url: String,
    network: String,
}

impl WalletManager {
    /// Create a new wallet manager for a specific network.
    pub fn new(network: Option<&str>) -> Self {
        let creds = WalletCredentials::load().unwrap_or_default();
        let network = network.unwrap_or(&creds.network).to_string();

        let auth_server_url = std::env::var("PGET_AUTH_URL")
            .ok()
            .unwrap_or_else(|| Self::auth_url_for_network(&network));

        Self {
            auth_server_url,
            network,
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

        let state: String = thread_rng()
            .sample_iter(&Alphanumeric)
            .take(32)
            .map(char::from)
            .collect();

        let auth_base_url = self.get_auth_base_url();

        println!("Starting authentication server...");
        let (port, rx) = run_callback_server(state.clone(), auth_base_url).await?;

        let callback_url = format!("http://127.0.0.1:{}/callback", port);

        let mut auth_url = Url::parse(&self.auth_server_url)
            .map_err(|e| PgetError::Http(format!("Invalid auth server URL: {}", e)))?;

        auth_url
            .query_pairs_mut()
            .append_pair("callback_url", &callback_url)
            .append_pair("state", &state)
            .append_pair("pub_key", &pub_key);

        let url_str = auth_url.to_string();

        println!("Opening browser for authentication...");
        println!("If the browser doesn't open, visit: {}", url_str);

        if let Err(e) = webbrowser::open(&url_str) {
            eprintln!("Failed to open browser: {}", e);
            println!("Please open this URL manually: {}", url_str);
        }

        println!("\nWaiting for authentication...");

        let callback = tokio::time::timeout(Duration::from_secs(CALLBACK_TIMEOUT_SECS), rx)
            .await
            .map_err(|_| PgetError::Http("Authentication timed out".to_string()))?
            .map_err(|_| {
                PgetError::Http("Failed to receive authentication callback".to_string())
            })?;

        self.save_credentials(callback, local_signer).await?;

        println!("Wallet connected successfully!");
        Ok(())
    }

    /// Refresh access key for an existing wallet.
    pub async fn refresh_access_key(&self, account_address: &str) -> Result<()> {
        let local_signer = PrivateKeySigner::random();
        let pub_key = format!("0x{}", hex::encode(local_signer.address()));

        let state: String = thread_rng()
            .sample_iter(&Alphanumeric)
            .take(32)
            .map(char::from)
            .collect();

        let auth_base_url = self.get_auth_base_url();

        println!("Starting authentication server...");
        let (port, rx) = run_callback_server(state.clone(), auth_base_url).await?;

        let callback_url = format!("http://127.0.0.1:{}/callback", port);

        let mut auth_url = Url::parse(&self.auth_server_url)
            .map_err(|e| PgetError::Http(format!("Invalid auth server URL: {}", e)))?;

        auth_url
            .query_pairs_mut()
            .append_pair("callback_url", &callback_url)
            .append_pair("state", &state)
            .append_pair("account", account_address)
            .append_pair("pub_key", &pub_key);

        let url_str = auth_url.to_string();

        println!("Opening browser to refresh access key...");
        println!("If the browser doesn't open, visit: {}", url_str);

        if let Err(e) = webbrowser::open(&url_str) {
            eprintln!("Failed to open browser: {}", e);
            println!("Please open this URL manually: {}", url_str);
        }

        println!("\nWaiting for authentication...");

        let callback = tokio::time::timeout(Duration::from_secs(CALLBACK_TIMEOUT_SECS), rx)
            .await
            .map_err(|_| PgetError::Http("Authentication timed out".to_string()))?
            .map_err(|_| {
                PgetError::Http("Failed to receive authentication callback".to_string())
            })?;

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
        let mut creds = WalletCredentials::load()?;
        creds.network = self.network.clone();

        let private_key_hex = format!("0x{}", hex::encode(local_signer.to_bytes()));
        let access_key = AccessKey::new(private_key_hex)
            .with_expiry(callback.expiry)
            .with_label("Default".to_string());

        let mut wallet = NetworkWallet {
            account_address: callback.account_address,
            access_keys: vec![],
            active_key_index: 0,
            pending_key_authorization: callback.key_authorization,
        };

        wallet.add_key(access_key, true);
        creds.set_wallet(wallet);
        creds.save()?;

        Ok(())
    }

    /// Save a new access key to an existing wallet.
    async fn save_access_key(
        &self,
        callback: AuthCallback,
        local_signer: PrivateKeySigner,
    ) -> Result<()> {
        let mut creds = WalletCredentials::load()?;
        creds.network = self.network.clone();

        let private_key_hex = format!("0x{}", hex::encode(local_signer.to_bytes()));
        let label = format!("Key {}", chrono_label());
        let access_key = AccessKey::new(private_key_hex)
            .with_expiry(callback.expiry)
            .with_label(label);

        if let Some(wallet) = creds.active_wallet_mut() {
            wallet.add_key(access_key, true);
            wallet.pending_key_authorization = callback.key_authorization;
        } else {
            let mut wallet = NetworkWallet {
                account_address: callback.account_address,
                access_keys: vec![],
                active_key_index: 0,
                pending_key_authorization: callback.key_authorization,
            };
            wallet.add_key(access_key, true);
            creds.set_wallet(wallet);
        }

        creds.save()?;
        Ok(())
    }
}

fn chrono_label() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .to_string()
}

impl Default for WalletManager {
    fn default() -> Self {
        Self::new(None)
    }
}
