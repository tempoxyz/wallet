//! Wallet credentials: key entries, selection logic, and persistence.

mod io;
mod model;
pub(crate) mod overrides;

pub(crate) use model::parse_private_key_signer;
pub(crate) use model::{
    keychain, KeyEntry, KeyType, StoredTokenLimit, WalletCredentials, WalletType,
};
pub(crate) use overrides::{
    has_credentials_override, set_credentials_override, set_key_name_override,
};

#[cfg(test)]
mod tests {
    use super::*;
    use zeroize::Zeroizing;

    const TEST_PRIVATE_KEY: &str =
        "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
    const TEST_ADDRESS: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";

    /// Helper to create a WalletCredentials with a single key.
    /// Uses `WalletType::Passkey` by default to avoid keychain interactions in tests.
    fn make_creds_with_profile(
        profile: &str,
        address: &str,
        access_key: Option<&str>,
    ) -> WalletCredentials {
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
        creds.keys.insert(profile.to_string(), key_entry);
        creds
    }

    /// Helper to create a WalletCredentials with a single "default" key.
    fn make_creds(address: &str, access_key: Option<&str>) -> WalletCredentials {
        make_creds_with_profile("default", address, access_key)
    }

    #[test]
    fn test_default_credentials() {
        let creds = WalletCredentials::default();
        assert!(!creds.has_wallet());
        assert!(creds.primary_key_name().is_none());
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
        // Use a unique profile to avoid keychain entries from other tests
        let creds = make_creds_with_profile("no-key-profile", "0xtest", None);
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
        let mut creds = make_creds("0xtest", Some(TEST_PRIVATE_KEY));
        {
            let entry = creds.keys.get_mut("default").unwrap();
            entry.chain_id = 4217;
            entry.provisioned = true;
        }
        assert!(creds.is_provisioned("tempo"));
        assert!(!creds.is_provisioned("tempo-moderato"));
        assert!(!creds.is_provisioned("nonexistent"));
    }

    // Tests for current wallet format only
    #[test]
    fn test_credentials_serialization_with_key() {
        // New format: key inline
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
        creds.keys.insert("default".to_string(), key_entry);

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
        creds.keys.insert("default".to_string(), key_entry);
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
        creds.keys.insert("default".to_string(), key_entry);

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
        // New format with key inline
        let toml_str = r#"
active = "default"

[keys.default]
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
active = "default"

[keys.default]
wallet_address = "0xtest"
"#;
        let creds: WalletCredentials = toml::from_str(toml_str).unwrap();
        assert_eq!(creds.wallet_address(), "0xtest");
        assert!(!creds.has_wallet());
    }

    #[test]
    fn test_insert_passkey_entry() {
        let mut creds = WalletCredentials::default();
        creds.keys.insert(
            "passkey-default".to_string(),
            KeyEntry {
                wallet_type: WalletType::Passkey,
                wallet_address: "0xABC".to_string(),
                key_address: Some("0xsigner1".to_string()),
                key: Some(Zeroizing::new("0xaccesskey1".to_string())),
                key_authorization: Some("auth".to_string()),
                ..Default::default()
            },
        );
        assert_eq!(creds.primary_key_name().unwrap(), "passkey-default");
        assert_eq!(creds.wallet_address(), "0xABC");
        assert!(creds.has_wallet());
        let key_entry = creds.primary_key().unwrap();
        assert_eq!(key_entry.key_address, Some("0xsigner1".to_string()));
    }

    #[test]
    fn test_multiple_keys() {
        let toml_str = r#"
active = "work"

[keys.default]
wallet_address = "0xAAA"
chain_id = 4217
key_address = "0xsigner1"
provisioned = true

[keys.work]
wallet_address = "0xBBB"
chain_id = 42431
key_address = "0xsigner2"
provisioned = true
"#;
        let creds: WalletCredentials = toml::from_str(toml_str).unwrap();
        // primary_key_name() picks "default" (first in BTreeMap order)
        assert_eq!(creds.primary_key_name().unwrap(), "default");
        assert_eq!(creds.wallet_address(), "0xAAA");
        assert!(creds.is_provisioned("tempo"));
        // "work" key is provisioned on moderato (42431), found via key_for_network
        assert!(creds.is_provisioned("tempo-moderato"));
    }

    #[test]
    fn test_delete_key() {
        let mut creds = make_creds("0xAAA", Some(TEST_PRIVATE_KEY));
        creds.keys.insert(
            "work".to_string(),
            KeyEntry {
                wallet_address: "0xBBB".to_string(),
                key: Some(Zeroizing::new("0xaccess".to_string())),
                ..Default::default()
            },
        );

        creds.delete_key("work").unwrap();
        assert_eq!(creds.keys.len(), 1);
        assert_eq!(creds.primary_key_name().unwrap(), "default");
    }

    #[test]
    fn test_delete_primary_key_switches() {
        let mut creds = make_creds("0xAAA", Some(TEST_PRIVATE_KEY));
        creds.keys.insert(
            "work".to_string(),
            KeyEntry {
                wallet_address: "0xBBB".to_string(),
                key: Some(Zeroizing::new("0xaccess".to_string())),
                ..Default::default()
            },
        );

        creds.delete_key("default").unwrap();
        assert_eq!(creds.primary_key_name().unwrap(), "work");
        assert_eq!(creds.primary_key().unwrap().wallet_address, "0xBBB");
    }

    #[test]
    fn test_delete_last_key() {
        let mut creds = make_creds("0xAAA", Some(TEST_PRIVATE_KEY));
        creds.delete_key("default").unwrap();
        assert!(creds.primary_key_name().is_none());
        assert!(creds.keys.is_empty());
    }

    #[test]
    fn test_delete_nonexistent() {
        let mut creds = make_creds("0xAAA", Some(TEST_PRIVATE_KEY));
        assert!(creds.delete_key("nonexistent").is_err());
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
        creds.keys.insert("test-profile".to_string(), key_entry);

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
        creds.keys.insert("default".to_string(), key_entry);

        assert_eq!(creds.key_address(), Some("0xsigneraddr".to_string()));
    }

    #[test]
    fn test_delete_removes_keychain_entry() {
        let profile = "delete-kc-test";
        keychain().set(profile, "0xsecret").unwrap();

        let mut creds = WalletCredentials::default();
        let key_entry = KeyEntry {
            wallet_address: "0xtest".to_string(),
            wallet_type: WalletType::Local,
            ..Default::default()
        };
        creds.keys.insert(profile.to_string(), key_entry);

        creds.delete_key(profile).unwrap();
        assert!(keychain().get(profile).unwrap().is_none());
    }

    #[test]
    fn test_delete_primary_key_switches_deterministic() {
        let mut creds = WalletCredentials::default();
        for name in ["zebra", "alpha", "middle"] {
            creds.keys.insert(
                name.to_string(),
                KeyEntry {
                    wallet_address: format!("0x{name}"),
                    key: Some(Zeroizing::new("0xaccess".to_string())),
                    ..Default::default()
                },
            );
        }

        creds.delete_key("zebra").unwrap();
        assert_eq!(creds.primary_key_name().unwrap(), "alpha");
    }

    #[test]
    fn test_from_private_key() {
        let creds = WalletCredentials::from_private_key(TEST_PRIVATE_KEY).unwrap();
        assert_eq!(creds.primary_key_name().unwrap(), "default");
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
    fn test_resolve_key_name_for_login_matches_active_wallet_address() {
        let mut creds = WalletCredentials::default();
        creds.keys.insert(
            "work".to_string(),
            KeyEntry {
                wallet_address: "0xWALLET".to_string(),
                ..Default::default()
            },
        );

        let name = creds.resolve_key_name_for_login("0xWALLET", "0xSIGNER");
        assert_eq!(name, "work");
    }

    #[test]
    fn test_resolve_key_name_for_login_matches_signer_address() {
        let mut creds = WalletCredentials::default();
        creds.keys.insert(
            "work".to_string(),
            KeyEntry {
                wallet_address: "0xOTHER".to_string(),
                key_address: Some("0xSIGNER".to_string()),
                ..Default::default()
            },
        );

        let name = creds.resolve_key_name_for_login("0xDIFFERENT", "0xSIGNER");
        assert_eq!(name, "work");
    }

    #[test]
    fn test_resolve_key_name_for_login_fallback_to_passkey() {
        let mut creds = WalletCredentials::default();
        creds.keys.insert(
            "work".to_string(),
            KeyEntry {
                wallet_address: "0xOTHER".to_string(),
                key_address: Some("0xOTHER_SIGNER".to_string()),
                ..Default::default()
            },
        );

        let name = creds.resolve_key_name_for_login("0xNEW", "0xNEW2");
        assert_eq!(name, "passkey-default");
    }

    #[test]
    fn test_primary_key_resolves_first() {
        let mut creds = WalletCredentials::default();
        creds.keys.insert(
            "somekey".to_string(),
            KeyEntry {
                wallet_address: "0xtest".to_string(),
                ..Default::default()
            },
        );
        // No passkey type or key, but it's the only key so primary_key_name() finds it
        assert_eq!(creds.primary_key_name(), Some("somekey"));
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
    fn test_relogin_existing_entry_clears_provisioned() {
        // Simulates the login re-login code path where an existing entry
        // is updated in-place with a new key — provisioned must be cleared
        // because the new key hasn't been provisioned yet.
        let mut creds = WalletCredentials::default();
        creds.keys.insert(
            "passkey-default".to_string(),
            KeyEntry {
                wallet_type: WalletType::Passkey,
                wallet_address: "0xABC".to_string(),
                key_address: Some("0xsigner1".to_string()),
                key: Some(Zeroizing::new("0xaccesskey1".to_string())),
                key_authorization: Some("auth".to_string()),
                provisioned: true,
                ..Default::default()
            },
        );

        // Simulate the re-login path: update existing entry in-place
        let profile = creds.resolve_key_name_for_login("0xABC", "0xsigner2");
        let key = creds.keys.get_mut(&profile).unwrap();
        key.key_address = Some("0xsigner2".to_string());
        key.key = Some(Zeroizing::new("0xaccesskey2".to_string()));
        key.key_authorization = None;
        key.provisioned = false;

        let key_entry = creds.primary_key().unwrap();
        assert!(!key_entry.provisioned);
        assert_eq!(key_entry.key_address, Some("0xsigner2".to_string()));
    }
}
