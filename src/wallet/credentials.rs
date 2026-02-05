//! Tempo wallet credentials stored in wallet.toml
//!
//! Separate from config.toml to keep passkey wallet credentials isolated.

use crate::error::{PgetError, Result};
use crate::wallet::access_key::AccessKey;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const WALLET_FILE_NAME: &str = "wallet.toml";
const DEFAULT_NETWORK: &str = "tempo-moderato";

/// Credentials for a Tempo network wallet.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NetworkWallet {
    #[serde(default)]
    pub account_address: String,

    #[serde(default)]
    pub access_keys: Vec<AccessKey>,

    #[serde(default)]
    pub active_key_index: usize,
}

impl NetworkWallet {
    /// Get the currently active access key.
    pub fn active_access_key(&self) -> Option<&AccessKey> {
        self.access_keys.get(self.active_key_index)
    }

    /// Add a new access key to the wallet.
    pub fn add_key(&mut self, key: AccessKey, make_active: bool) {
        self.access_keys.push(key);
        if make_active {
            self.active_key_index = self.access_keys.len() - 1;
        }
    }

    /// Remove an access key by index.
    pub fn remove_key(&mut self, index: usize) -> Option<AccessKey> {
        if index >= self.access_keys.len() {
            return None;
        }

        let key = self.access_keys.remove(index);

        if self.access_keys.is_empty() {
            self.active_key_index = 0;
        } else if self.active_key_index >= self.access_keys.len() {
            self.active_key_index = self.access_keys.len() - 1;
        } else if index < self.active_key_index {
            self.active_key_index -= 1;
        }

        Some(key)
    }

    /// Switch to a different access key by index.
    pub fn switch_key(&mut self, index: usize) -> bool {
        if index < self.access_keys.len() {
            self.active_key_index = index;
            true
        } else {
            false
        }
    }
}

/// Wallet credentials stored in wallet.toml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletCredentials {
    /// Active network for Tempo wallet
    #[serde(default = "default_network")]
    pub network: String,

    /// Tempo mainnet wallet
    #[serde(default)]
    pub tempo: Option<NetworkWallet>,

    /// Tempo Moderato testnet wallet
    #[serde(default, rename = "tempo-moderato")]
    pub tempo_moderato: Option<NetworkWallet>,
}

fn default_network() -> String {
    DEFAULT_NETWORK.to_string()
}

impl Default for WalletCredentials {
    fn default() -> Self {
        Self {
            network: default_network(),
            tempo: None,
            tempo_moderato: None,
        }
    }
}

impl WalletCredentials {
    /// Get the data directory path.
    pub fn data_dir() -> Result<PathBuf> {
        let data_dir = dirs::data_dir().ok_or(PgetError::NoConfigDir)?.join("pget");

        if !data_dir.exists() {
            fs::create_dir_all(&data_dir)?;
        }

        Ok(data_dir)
    }

    /// Get the wallet.toml file path.
    pub fn wallet_path() -> Result<PathBuf> {
        Ok(Self::data_dir()?.join(WALLET_FILE_NAME))
    }

    /// Load wallet credentials from disk.
    pub fn load() -> Result<Self> {
        let path = Self::wallet_path()?;

        if !path.exists() {
            return Ok(Self::default());
        }

        let contents = fs::read_to_string(&path)?;
        let creds: WalletCredentials = toml::from_str(&contents)?;
        Ok(creds)
    }

    /// Save wallet credentials atomically.
    pub fn save(&self) -> Result<()> {
        let path = Self::wallet_path()?;
        let dir = path.parent().ok_or(PgetError::NoConfigDir)?;

        let contents = toml::to_string_pretty(self)?;

        let temp_path = dir.join(format!(".wallet.{}.tmp", std::process::id()));
        fs::write(&temp_path, &contents)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = fs::Permissions::from_mode(0o600);
            fs::set_permissions(&temp_path, perms)?;
        }

        fs::rename(&temp_path, &path)?;
        Ok(())
    }

    /// Get the wallet for the currently active network.
    pub fn active_wallet(&self) -> Option<&NetworkWallet> {
        match self.network.as_str() {
            "tempo" => self.tempo.as_ref(),
            "tempo-moderato" => self.tempo_moderato.as_ref(),
            _ => None,
        }
        .filter(|w| !w.account_address.is_empty())
    }

    /// Get a mutable reference to the wallet for the currently active network.
    pub fn active_wallet_mut(&mut self) -> Option<&mut NetworkWallet> {
        match self.network.as_str() {
            "tempo" => self.tempo.as_mut(),
            "tempo-moderato" => self.tempo_moderato.as_mut(),
            _ => None,
        }
        .filter(|w| !w.account_address.is_empty())
    }

    /// Set the wallet for the currently active network.
    pub fn set_wallet(&mut self, wallet: NetworkWallet) {
        match self.network.as_str() {
            "tempo" => self.tempo = Some(wallet),
            "tempo-moderato" => self.tempo_moderato = Some(wallet),
            _ => {}
        }
    }

    /// Clear the wallet for the currently active network.
    pub fn clear_wallet(&mut self) {
        match self.network.as_str() {
            "tempo" => self.tempo = None,
            "tempo-moderato" => self.tempo_moderato = None,
            _ => {}
        }
    }

    /// Check if there's a valid wallet for the active network.
    #[allow(dead_code)]
    pub fn has_wallet(&self) -> bool {
        self.active_wallet().is_some()
    }

    /// Get auth server URL for the active network.
    #[allow(dead_code)]
    pub fn auth_server_url(&self) -> &'static str {
        match self.network.as_str() {
            "tempo" => "https://app.tempo.xyz/cli-auth",
            _ => "https://app.moderato.tempo.xyz/cli-auth",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_credentials() {
        let creds = WalletCredentials::default();
        assert_eq!(creds.network, "tempo-moderato");
        assert!(creds.tempo.is_none());
        assert!(creds.tempo_moderato.is_none());
    }

    #[test]
    fn test_network_wallet_add_key() {
        let mut wallet = NetworkWallet::default();
        let key = AccessKey::new("0x1234".to_string());
        wallet.add_key(key, true);
        assert_eq!(wallet.active_key_index, 0);
        assert_eq!(wallet.access_keys.len(), 1);
    }

    #[test]
    fn test_network_wallet_switch_key() {
        let mut wallet = NetworkWallet::default();
        wallet.add_key(AccessKey::new("0x1111".to_string()), true);
        wallet.add_key(AccessKey::new("0x2222".to_string()), false);

        assert_eq!(wallet.active_key_index, 0);
        assert!(wallet.switch_key(1));
        assert_eq!(wallet.active_key_index, 1);
        assert!(!wallet.switch_key(5));
    }

    #[test]
    fn test_network_wallet_remove_key() {
        let mut wallet = NetworkWallet::default();
        wallet.add_key(AccessKey::new("0x1111".to_string()), true);
        wallet.add_key(AccessKey::new("0x2222".to_string()), true);

        assert_eq!(wallet.active_key_index, 1);
        wallet.remove_key(0);
        assert_eq!(wallet.active_key_index, 0);
        assert_eq!(wallet.access_keys.len(), 1);
    }

    #[test]
    fn test_credentials_serialization() {
        let mut creds = WalletCredentials {
            network: "tempo".to_string(),
            ..Default::default()
        };

        let wallet = NetworkWallet {
            account_address: "0xtest".to_string(),
            ..Default::default()
        };
        creds.tempo = Some(wallet);

        let toml_str = toml::to_string_pretty(&creds).unwrap();
        assert!(toml_str.contains("network = \"tempo\""));
        assert!(toml_str.contains("account_address = \"0xtest\""));
    }

    #[test]
    fn test_active_wallet() {
        let mut creds = WalletCredentials {
            network: "tempo-moderato".to_string(),
            ..Default::default()
        };

        let wallet = NetworkWallet {
            account_address: "0xtest".to_string(),
            ..Default::default()
        };
        creds.tempo_moderato = Some(wallet);

        assert!(creds.active_wallet().is_some());
        assert_eq!(creds.active_wallet().unwrap().account_address, "0xtest");
    }

    #[test]
    fn test_active_wallet_empty_address() {
        let mut creds = WalletCredentials::default();
        let wallet = NetworkWallet::default();
        creds.tempo_moderato = Some(wallet);

        assert!(creds.active_wallet().is_none());
    }
}
