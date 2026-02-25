//! Wallet management commands — create and delete wallets.

use alloy::rlp::Encodable;
use alloy::signers::local::PrivateKeySigner;
use alloy::signers::SignerSync;
use anyhow::Result;
use std::time::{SystemTime, UNIX_EPOCH};
use tempo_primitives::transaction::{
    KeyAuthorization, PrimitiveSignature, SignatureType, TokenLimit,
};
use zeroize::Zeroizing;

use crate::wallet::credentials::{self, keychain, KeyEntry, WalletCredentials, WalletType};

/// Create a local EOA wallet with an access key.
///
/// 1. Generate random EOA key → store in OS keychain (wallet owner key)
/// 2. Generate random access key → store inline in keys.toml
/// 3. Sign key_authorization with chain_id=0 using the local EOA
/// 4. Do not provision; auto-provisions on first payment
/// 5. Print the fundable wallet address
pub fn create_local_wallet(name: &str) -> Result<()> {
    if credentials::has_credentials_override() {
        anyhow::bail!("Cannot create wallets with --private-key flag");
    }

    let mut creds = WalletCredentials::load()?;
    if creds.keys.contains_key(name) {
        anyhow::bail!("Key '{name}' already exists. Use a different name.");
    }

    // Generate wallet EOA key and store in OS keychain
    let wallet_signer = PrivateKeySigner::random();
    let wallet_key_hex = Zeroizing::new(format!("0x{}", hex::encode(wallet_signer.to_bytes())));
    let wallet_address = format!("{}", wallet_signer.address());

    keychain().set(name, &wallet_key_hex).map_err(|e| {
        crate::error::PrestoError::Keychain(format!("Failed to store wallet key: {e}"))
    })?;

    // Generate access key
    let access_signer = PrivateKeySigner::random();
    let access_key_hex = Zeroizing::new(format!("0x{}", hex::encode(access_signer.to_bytes())));
    let access_key_address = format!("{}", access_signer.address());

    // Sign key_authorization with chain_id=0 (all chains)
    let (key_auth_hex, _) = sign_key_authorization(&wallet_signer, &access_signer)?;

    let key_entry = KeyEntry {
        wallet_type: WalletType::Local,
        wallet_address: wallet_address.clone(),
        access_key_address: Some(access_key_address),
        access_key: Some(access_key_hex),
        key_authorization: Some(key_auth_hex),
        ..Default::default()
    };
    creds.keys.insert(name.to_string(), key_entry);
    if let Err(e) = creds.save() {
        let _ = keychain().delete(name);
        return Err(e.into());
    }

    Ok(())
}

/// Renew the access key for an existing local wallet.
///
/// 1. Load the wallet EOA key from the OS keychain
/// 2. Generate a new random access key → store inline in keys.toml
/// 3. Sign a fresh key_authorization (30-day expiry, $100 limit)
/// 4. Clear provisioned_chain_ids (new key must re-provision)
pub fn create_access_key(name: &str) -> Result<()> {
    if credentials::has_credentials_override() {
        anyhow::bail!("Cannot renew wallets with --private-key flag");
    }

    let mut creds = WalletCredentials::load()?;
    let key_entry = creds
        .keys
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("Wallet '{name}' not found."))?;

    if key_entry.wallet_type != WalletType::Local {
        anyhow::bail!("Only local wallets can be renewed. Use ' tempo-walletlogin' for passkey wallets.");
    }

    // Load wallet EOA key from OS keychain
    let wallet_key_hex = keychain()
        .get(name)
        .map_err(|e| {
            crate::error::PrestoError::Keychain(format!("Failed to load wallet key: {e}"))
        })?
        .ok_or_else(|| {
            anyhow::anyhow!(
            "Wallet key not found in keychain for '{name}'. The wallet may need to be re-created."
        )
        })?;
    let wallet_signer: PrivateKeySigner =
        crate::wallet::credentials::parse_private_key_signer(&wallet_key_hex)
            .map_err(|e| anyhow::anyhow!("Invalid wallet key in keychain: {e}"))?;

    // Generate new access key
    let access_signer = PrivateKeySigner::random();
    let access_key_hex = Zeroizing::new(format!("0x{}", hex::encode(access_signer.to_bytes())));
    let access_key_address = format!("{}", access_signer.address());

    // Sign key_authorization with fresh expiry
    let (key_auth_hex, _) = sign_key_authorization(&wallet_signer, &access_signer)?;

    // Update the key entry in-place
    let entry = creds.keys.get_mut(name).unwrap();
    entry.access_key_address = Some(access_key_address);
    entry.access_key = Some(access_key_hex);
    entry.key_authorization = Some(key_auth_hex);
    entry.provisioned_chain_ids.clear();

    creds.save()?;
    Ok(())
}

/// Sign a key authorization for an access key using the wallet EOA.
///
/// Returns `(key_auth_hex, expiry_secs)`. Uses chain_id=0 (all chains),
/// $100 USDC limit, and 30-day expiry.
fn sign_key_authorization(
    wallet_signer: &PrivateKeySigner,
    access_signer: &PrivateKeySigner,
) -> Result<(String, u64)> {
    let expiry_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 30 * 24 * 60 * 60;
    let limit = alloy::primitives::U256::from(100_000_000u64); // $100 with 6 decimals
    let token_limits: Vec<TokenLimit> = [
        crate::network::tempo_tokens::USDCE,
        crate::network::tempo_tokens::PATH_USD,
    ]
    .iter()
    .map(|addr| TokenLimit {
        token: addr.parse().unwrap(),
        limit,
    })
    .collect();
    let auth = KeyAuthorization {
        chain_id: 0,
        key_type: SignatureType::Secp256k1,
        key_id: access_signer.address(),
        expiry: Some(expiry_secs),
        limits: Some(token_limits),
    };
    let sig = wallet_signer
        .sign_hash_sync(&auth.signature_hash())
        .map_err(|e| anyhow::anyhow!("Failed to sign key authorization: {e}"))?;
    let signed = auth.into_signed(PrimitiveSignature::Secp256k1(sig));
    let mut buf = Vec::new();
    signed.encode(&mut buf);
    Ok((format!("0x{}", hex::encode(&buf)), expiry_secs))
}

/// Delete a wallet by name.
pub fn delete_wallet(name: &str, yes: bool) -> Result<()> {
    let creds = WalletCredentials::load()?;

    if !creds.keys.contains_key(name) {
        anyhow::bail!("Wallet '{name}' not found.");
    }

    if !yes {
        use std::io::IsTerminal;
        if !std::io::stdin().is_terminal() {
            anyhow::bail!("Use --yes for non-interactive delete");
        }

        print!("Delete wallet '{name}'? [y/N] ");
        use std::io::{self, Write};
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Cancelled.");
            return Ok(());
        }
    }

    let mut creds = creds;
    creds.delete_key(name)?;
    creds.save()?;

    if creds.keys.is_empty() {
        println!("Deleted wallet '{name}'. No wallets configured.");
    } else {
        println!("Deleted wallet '{name}'.");
    }
    Ok(())
}

/// Import an existing EOA private key as a local wallet.
///
/// Reads the private key from `--private-key`, `--stdin-key`, or prompts interactively
/// (masked) on a TTY. Stores the key in the OS keychain and records the wallet
/// address in keys.toml. Does not create an access key; run ` tempo-walletlogin` to connect
/// and provision when ready.
pub fn import_wallet(name: &str, private_key_arg: Option<String>, stdin_key: bool) -> Result<()> {
    if credentials::has_credentials_override() {
        anyhow::bail!("Cannot import wallets with --private-key flag");
    }

    let mut creds = WalletCredentials::load()?;
    if creds.keys.contains_key(name) {
        anyhow::bail!("Wallet '{name}' already exists. Use a different name.");
    }

    let key_hex: Zeroizing<String> = if let Some(pk) = private_key_arg {
        Zeroizing::new(pk)
    } else if stdin_key {
        read_private_key_noninteractive()? // single-line from stdin
    } else {
        read_private_key()? // interactive masked prompt or pipe
    };

    // Parse and validate the key using shared helper
    let signer: PrivateKeySigner = crate::wallet::credentials::parse_private_key_signer(&key_hex)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    let private_key_hex = Zeroizing::new(format!("0x{}", hex::encode(signer.to_bytes())));
    let address = format!("{}", signer.address());

    // Store wallet EOA key in OS keychain
    keychain()
        .set(name, &private_key_hex)
        .map_err(|e| crate::error::PrestoError::Keychain(format!("Failed to store key: {e}")))?;

    let key = KeyEntry {
        wallet_type: WalletType::Local,
        wallet_address: address.clone(),
        ..Default::default()
    };
    creds.keys.insert(name.to_string(), key);
    if let Err(e) = creds.save() {
        let _ = keychain().delete(name);
        return Err(e.into());
    }

    println!("Imported wallet '{name}'.");
    println!("  Address: {address}");
    println!("\nRun ' tempo-walletlogin' to connect and authorize payments.");
    Ok(())
}

/// Read a private key from stdin.
///
/// Disables terminal echo on Unix to avoid leaking the key,
/// or reads silently from a pipe.
fn read_private_key() -> Result<Zeroizing<String>> {
    use std::io::{self, BufRead, IsTerminal, Write};
    use zeroize::Zeroize;

    struct EchoGuard;
    impl Drop for EchoGuard {
        fn drop(&mut self) {
            let _ = std::process::Command::new("stty").args(["echo"]).status();
            // Print a newline after restoring echo (matches prior behavior)
            let _ = writeln!(std::io::stdout());
        }
    }

    if io::stdin().is_terminal() {
        print!("Enter private key: ");
        io::stdout().flush()?;

        // Disable echo with a guard to ensure restoration on all paths
        let _echo_guard = EchoGuard;
        let _ = std::process::Command::new("stty").args(["-echo"]).status();

        let mut input = String::new();
        io::stdin().lock().read_line(&mut input)?;

        let key = Zeroizing::new(input.trim().to_string());
        input.zeroize();
        if key.is_empty() {
            anyhow::bail!("No private key provided");
        }
        Ok(key)
    } else {
        // Reading from pipe
        let mut input = String::new();
        io::stdin().lock().read_line(&mut input)?;
        let key = Zeroizing::new(input.trim().to_string());
        input.zeroize();
        if key.is_empty() {
            anyhow::bail!("No private key provided on stdin");
        }
        Ok(key)
    }
}

/// Read a private key from stdin non-interactively (one line, no prompts).
fn read_private_key_noninteractive() -> Result<Zeroizing<String>> {
    use std::io::{self, BufRead};
    use zeroize::Zeroize;
    let mut input = String::new();
    io::stdin().lock().read_line(&mut input)?;
    let key = Zeroizing::new(input.trim().to_string());
    input.zeroize();
    if key.is_empty() {
        anyhow::bail!("No private key provided on stdin");
    }
    Ok(key)
}
