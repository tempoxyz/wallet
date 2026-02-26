//! Wallet credentials: key entries, selection logic, and persistence.

mod io;
mod model;
pub(crate) mod overrides;

pub(crate) use model::parse_private_key_signer;
pub(crate) use model::{
    keychain, KeyEntry, KeyType, StoredTokenLimit, WalletCredentials, WalletType,
};
pub(crate) use overrides::{has_credentials_override, set_credentials_override};

#[cfg(test)]
mod tests {
    use super::*;
    use zeroize::Zeroizing;

    const TEST_PRIVATE_KEY: &str =
        "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
    const TEST_ADDRESS: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";

    /// Helper to create a WalletCredentials with a single key entry.
    /// Uses `WalletType::Passkey` by default to avoid keychain interactions in tests.
    fn make_creds(address: &str, access_key: Option<&str>) -> WalletCredentials {
        let mut creds = WalletCredentials::default();
        let mut key_entry = KeyEntry {
            wallet_type: WalletType::Passkey,
            wallet_address: address.to_string(),
            ..Default::default()
        };
        if let Some(pk) = access_key {
            let trimmed = pk.trim();
            if !trimmed.is_empty() {
                if let Ok(signer) = parse_private_key_signer(trimmed) {
                    key_entry.key = Some(Zeroizing::new(trimmed.to_string()));
                    key_entry.key_address = Some(format!("{}", signer.address()));
                }
            }
        }
        creds.keys.push(key_entry);
        creds
    }

    #[test]
    fn test_default_credentials() {
        let creds = WalletCredentials::default();
        assert!(!creds.has_wallet());
        assert!(creds.primary_key().is_none());
        assert!(creds.keys.is_empty());
    }

    #[test]
    fn test_has_wallet() {
        // No keys at all
        let creds = WalletCredentials::default();
        assert!(!creds.has_wallet());

        // wallet_address alone is not enough
        let creds = make_creds("0xtest", None);
        assert!(!creds.has_wallet());

        // needs wallet_address + key
        let creds = make_creds("0xtest", Some(TEST_PRIVATE_KEY));
        assert!(creds.has_wallet());

        // empty key doesn't count
        let creds = make_creds("0xtest", Some(""));
        assert!(!creds.has_wallet());
    }

    #[test]
    fn test_signer() {
        let creds = make_creds("0xtest", Some(TEST_PRIVATE_KEY));
        let signer = creds.signer().unwrap();
        assert_eq!(
            format!("{}", signer.address()).to_lowercase(),
            TEST_ADDRESS.to_lowercase()
        );
    }

    #[test]
    fn test_signer_no_key() {
        let creds = make_creds("0xtest", None);
        assert!(creds.signer().is_err());
    }

    #[test]
    fn test_key_address() {
        let creds = make_creds("0xtest", Some(TEST_PRIVATE_KEY));
        let addr = creds.key_address().unwrap();
        assert_eq!(addr.to_lowercase(), TEST_ADDRESS.to_lowercase());
    }

    #[test]
    fn test_is_provisioned() {
        let mut creds = WalletCredentials::default();
        creds.keys.push(KeyEntry {
            wallet_type: WalletType::Local,
            wallet_address: "0xtest".to_string(),
            chain_id: 4217,
            provisioned: true,
            ..Default::default()
        });
        assert!(creds.is_provisioned("tempo"));
        assert!(!creds.is_provisioned("tempo-moderato"));
        assert!(!creds.is_provisioned("nonexistent"));
    }

    // Tests for current wallet format only
    #[test]
    fn test_credentials_serialization_with_key() {
        let mut creds = WalletCredentials::default();
        let key_entry = KeyEntry {
            wallet_address: "0xwallet".to_string(),
            key_address: Some("0xsigner".to_string()),
            key: Some(Zeroizing::new("0xaccesskey".to_string())),
            key_authorization: Some("auth123".to_string()),
            chain_id: 4217,
            provisioned: true,
            ..Default::default()
        };
        creds.keys.push(key_entry);

        let toml_str = toml::to_string_pretty(&creds).unwrap();
        assert!(toml_str.contains("key_address = \"0xsigner\""));
        assert!(toml_str.contains("key = \"0xaccesskey\""));
        assert!(!toml_str.contains("private_key"));

        let parsed: WalletCredentials = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.wallet_address(), "0xwallet");
        assert!(parsed.has_wallet());
    }

    #[test]
    fn test_not_ready_when_no_signing_key() {
        // wallet_address alone (no key) → not ready
        let mut creds = WalletCredentials::default();
        let key_entry = KeyEntry {
            wallet_address: "0xtest".to_string(),
            ..Default::default()
        };
        creds.keys.push(key_entry);
        assert!(!creds.has_wallet());
    }

    #[test]
    fn test_round_trip_via_atomic_write() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("keys.toml");

        let mut creds = WalletCredentials::default();
        let key_entry = KeyEntry {
            wallet_address: "0xdeadbeef".to_string(),
            key_address: Some("0xsigneraddr".to_string()),
            key: Some(Zeroizing::new("0xaccesskey".to_string())),
            key_authorization: Some("pending123".to_string()),
            chain_id: 4217,
            provisioned: true,
            ..Default::default()
        };
        creds.keys.push(key_entry);

        let contents = toml::to_string_pretty(&creds).expect("serialize");
        crate::util::atomic_write(&path, &contents, 0o600).expect("write");

        let loaded: WalletCredentials =
            toml::from_str(&std::fs::read_to_string(&path).expect("read")).expect("deserialize");
        assert_eq!(loaded.wallet_address(), "0xdeadbeef");
        assert!(loaded.is_provisioned("tempo"));
        assert!(!loaded.is_provisioned("tempo-moderato"));
    }

    #[cfg(unix)]
    #[test]
    fn test_wallet_save_permissions_via_atomic_write() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("keys.toml");

        let creds = WalletCredentials::default();
        let contents = toml::to_string_pretty(&creds).expect("serialize");
        crate::util::atomic_write(&path, &contents, 0o600).expect("write");

        let mode = std::fs::metadata(&path)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn test_new_format_loads_correctly() {
        // New format with key inline using [[keys]] array
        let toml_str = r#"
[[keys]]
wallet_address = "0xtest"
chain_id = 4217
key_address = "0xsigner"
key = "0xaccesskey"
key_authorization = "auth123"
provisioned = true
"#;
        let creds: WalletCredentials = toml::from_str(toml_str).unwrap();
        assert_eq!(creds.wallet_address(), "0xtest");
        assert!(creds.has_wallet());
        assert!(creds.is_provisioned("tempo"));
    }

    #[test]
    fn test_wallet_address_only_not_enough() {
        // wallet_address alone without key is not enough
        let toml_str = r#"
[[keys]]
wallet_address = "0xtest"
"#;
        let creds: WalletCredentials = toml::from_str(toml_str).unwrap();
        assert_eq!(creds.wallet_address(), "0xtest");
        assert!(!creds.has_wallet());
    }

    #[test]
    fn test_insert_passkey_entry() {
        let mut creds = WalletCredentials::default();
        creds.keys.push(KeyEntry {
            wallet_type: WalletType::Passkey,
            wallet_address: "0xABC".to_string(),
            key_address: Some("0xsigner1".to_string()),
            key: Some(Zeroizing::new("0xaccesskey1".to_string())),
            key_authorization: Some("auth".to_string()),
            ..Default::default()
        });
        assert!(creds.primary_key().is_some());
        assert_eq!(creds.wallet_address(), "0xABC");
        assert!(creds.has_wallet());
        let key_entry = creds.primary_key().unwrap();
        assert_eq!(key_entry.key_address, Some("0xsigner1".to_string()));
    }

    #[test]
    fn test_multiple_keys() {
        let toml_str = r#"
[[keys]]
wallet_address = "0xAAA"
chain_id = 4217
key_address = "0xsigner1"
provisioned = true

[[keys]]
wallet_address = "0xBBB"
chain_id = 42431
key_address = "0xsigner2"
provisioned = true
"#;
        let creds: WalletCredentials = toml::from_str(toml_str).unwrap();
        // primary_key() picks first entry (no passkey, no key → first)
        assert_eq!(creds.wallet_address(), "0xAAA");
        assert!(creds.is_provisioned("tempo"));
        // second key is provisioned on moderato (42431), found via key_for_network
        assert!(creds.is_provisioned("tempo-moderato"));
    }

    #[test]
    fn test_delete_by_address() {
        let mut creds = make_creds("0xAAA", Some(TEST_PRIVATE_KEY));
        creds.keys.push(KeyEntry {
            wallet_type: WalletType::Passkey,
            wallet_address: "0xBBB".to_string(),
            key: Some(Zeroizing::new("0xaccess".to_string())),
            ..Default::default()
        });

        creds.delete_by_address("0xBBB").unwrap();
        assert_eq!(creds.keys.len(), 1);
        assert_eq!(creds.primary_key().unwrap().wallet_address, "0xAAA");
    }

    #[test]
    fn test_delete_passkey() {
        let mut creds = WalletCredentials::default();
        // Local wallet entry
        creds.keys.push(KeyEntry {
            wallet_type: WalletType::Local,
            wallet_address: "0xAAA".to_string(),
            key: Some(Zeroizing::new("0xaccess".to_string())),
            ..Default::default()
        });
        // Passkey entry
        creds.keys.push(KeyEntry {
            wallet_type: WalletType::Passkey,
            wallet_address: "0xBBB".to_string(),
            key: Some(Zeroizing::new("0xaccess".to_string())),
            ..Default::default()
        });

        creds.delete_passkey().unwrap();
        assert_eq!(creds.keys.len(), 1);
        assert_eq!(creds.primary_key().unwrap().wallet_address, "0xAAA");
    }

    #[test]
    fn test_delete_primary_key_switches() {
        let mut creds = make_creds("0xAAA", Some(TEST_PRIVATE_KEY));
        creds.keys.push(KeyEntry {
            wallet_address: "0xBBB".to_string(),
            key: Some(Zeroizing::new("0xaccess".to_string())),
            ..Default::default()
        });

        creds.delete_by_address("0xAAA").unwrap();
        assert_eq!(creds.primary_key().unwrap().wallet_address, "0xBBB");
    }

    #[test]
    fn test_delete_last_key() {
        let mut creds = make_creds("0xAAA", Some(TEST_PRIVATE_KEY));
        creds.delete_by_address("0xAAA").unwrap();
        assert!(creds.primary_key().is_none());
        assert!(creds.keys.is_empty());
    }

    #[test]
    fn test_delete_nonexistent() {
        let mut creds = make_creds("0xAAA", Some(TEST_PRIVATE_KEY));
        assert!(creds.delete_by_address("0xnonexistent").is_err());
    }

    // ==================== Keychain Integration Tests ====================

    #[test]
    fn test_signer_uses_inline_key() {
        let mut creds = WalletCredentials::default();
        let key_entry = KeyEntry {
            wallet_address: TEST_ADDRESS.to_string(),
            key: Some(Zeroizing::new(TEST_PRIVATE_KEY.to_string())),
            key_address: Some(TEST_ADDRESS.to_string()),
            ..Default::default()
        };
        creds.keys.push(key_entry);

        let signer = creds.signer().unwrap();
        assert_eq!(
            format!("{}", signer.address()).to_lowercase(),
            TEST_ADDRESS.to_lowercase()
        );
    }

    #[test]
    fn test_key_address_from_field() {
        let mut creds = WalletCredentials::default();
        let key_entry = KeyEntry {
            wallet_address: "0xtest".to_string(),
            key_address: Some("0xsigneraddr".to_string()),
            ..Default::default()
        };
        creds.keys.push(key_entry);

        assert_eq!(creds.key_address(), Some("0xsigneraddr".to_string()));
    }

    #[test]
    fn test_delete_removes_keychain_entry() {
        let wallet_addr = "0xdelete-kc-test-addr";
        keychain().set(wallet_addr, "0xsecret").unwrap();

        let mut creds = WalletCredentials::default();
        let key_entry = KeyEntry {
            wallet_address: wallet_addr.to_string(),
            wallet_type: WalletType::Local,
            ..Default::default()
        };
        creds.keys.push(key_entry);

        creds.delete_by_address(wallet_addr).unwrap();
        assert!(keychain().get(wallet_addr).unwrap().is_none());
    }

    #[test]
    fn test_from_private_key() {
        let creds = WalletCredentials::from_private_key(TEST_PRIVATE_KEY).unwrap();
        assert_eq!(
            creds.wallet_address().to_lowercase(),
            TEST_ADDRESS.to_lowercase()
        );
        assert!(creds.has_wallet());
        let signer = creds.signer().unwrap();
        assert_eq!(
            format!("{}", signer.address()).to_lowercase(),
            TEST_ADDRESS.to_lowercase()
        );
    }

    #[test]
    fn test_from_private_key_invalid() {
        assert!(WalletCredentials::from_private_key("not-a-key").is_err());
    }

    #[test]
    fn test_primary_key_resolves_first() {
        let mut creds = WalletCredentials::default();
        creds.keys.push(KeyEntry {
            wallet_address: "0xtest".to_string(),
            ..Default::default()
        });
        // No passkey type or key, but it's the only key so primary_key() finds it
        assert_eq!(creds.primary_key().unwrap().wallet_address, "0xtest");
    }

    #[test]
    fn test_parse_private_key_signer_valid() {
        let signer = parse_private_key_signer(TEST_PRIVATE_KEY).unwrap();
        assert_eq!(
            format!("{}", signer.address()).to_lowercase(),
            TEST_ADDRESS.to_lowercase()
        );
    }

    #[test]
    fn test_parse_private_key_signer_no_prefix() {
        let no_prefix = TEST_PRIVATE_KEY.strip_prefix("0x").unwrap();
        let signer = parse_private_key_signer(no_prefix).unwrap();
        assert_eq!(
            format!("{}", signer.address()).to_lowercase(),
            TEST_ADDRESS.to_lowercase()
        );
    }

    #[test]
    fn test_parse_private_key_signer_invalid_hex() {
        assert!(parse_private_key_signer("not-hex").is_err());
    }

    #[test]
    fn test_parse_private_key_signer_wrong_length() {
        assert!(parse_private_key_signer("0xdeadbeef").is_err());
    }

    #[test]
    fn test_upsert_by_wallet_address_creates_new() {
        let mut creds = WalletCredentials::default();
        let entry = creds.upsert_by_wallet_address("0xABC");
        entry.wallet_type = WalletType::Passkey;
        entry.key_address = Some("0xsigner1".to_string());
        entry.key = Some(Zeroizing::new("0xaccesskey1".to_string()));
        entry.provisioned = true;

        assert_eq!(creds.keys.len(), 1);
        assert_eq!(creds.wallet_address(), "0xABC");
    }

    #[test]
    fn test_upsert_by_wallet_address_updates_existing() {
        let mut creds = WalletCredentials::default();
        creds.keys.push(KeyEntry {
            wallet_type: WalletType::Passkey,
            wallet_address: "0xABC".to_string(),
            key_address: Some("0xsigner1".to_string()),
            key: Some(Zeroizing::new("0xaccesskey1".to_string())),
            provisioned: true,
            ..Default::default()
        });

        // Upsert same address — should update in-place
        let entry = creds.upsert_by_wallet_address("0xABC");
        entry.key_address = Some("0xsigner2".to_string());
        entry.key = Some(Zeroizing::new("0xaccesskey2".to_string()));
        entry.provisioned = false;

        assert_eq!(creds.keys.len(), 1);
        let key_entry = creds.primary_key().unwrap();
        assert!(!key_entry.provisioned);
        assert_eq!(key_entry.key_address, Some("0xsigner2".to_string()));
    }

    #[test]
    fn test_find_passkey() {
        let mut creds = WalletCredentials::default();
        assert!(creds.find_passkey().is_none());

        creds.keys.push(KeyEntry {
            wallet_type: WalletType::Passkey,
            wallet_address: "0xABC".to_string(),
            ..Default::default()
        });
        assert!(creds.find_passkey().is_some());
        assert_eq!(creds.find_passkey().unwrap().wallet_address, "0xABC");
    }

    #[test]
    fn test_key_for_network_passkey_fallback() {
        let mut creds = WalletCredentials::default();
        creds.keys.push(KeyEntry {
            wallet_type: WalletType::Passkey,
            wallet_address: "0xABC".to_string(),
            key: Some(Zeroizing::new("0xaccess".to_string())),
            chain_id: 4217,
            ..Default::default()
        });
        // Exact chain_id match
        assert!(creds.key_for_network("tempo").is_some());
        // No chain_id match, but passkey fallback kicks in
        assert!(creds.key_for_network("tempo-moderato").is_some());
    }
}
