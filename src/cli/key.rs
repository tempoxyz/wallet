//! Key management commands — list, switch, rename, delete, create, import keys.

use alloy::signers::local::PrivateKeySigner;
use anyhow::Result;

use crate::wallet::credentials::{self, keychain, WalletCredentials};

/// List all access keys.
pub fn list_keys() -> Result<()> {
    let creds = WalletCredentials::load()?;

    if creds.keys.is_empty() {
        println!("No keys. Run 'presto login' to add one.");
        return Ok(());
    }

    // Print active first, then remaining keys (BTreeMap iterates sorted)
    if let Some(key) = creds.keys.get(&creds.active) {
        let name = &creds.active;
        let addr = if key.account_address.is_empty() {
            "(no address)"
        } else {
            &key.account_address
        };
        println!("  {name} *  {addr}");
    }

    for (name, key) in &creds.keys {
        if *name == creds.active {
            continue;
        }
        let addr = if key.account_address.is_empty() {
            "(no address)"
        } else {
            &key.account_address
        };
        println!("  {name}    {addr}");
    }

    Ok(())
}

/// Switch the active key.
pub fn switch_key(profile: &str) -> Result<()> {
    let mut creds = WalletCredentials::load()?;
    creds.switch(profile)?;
    creds.save()?;
    println!("Switched to key '{profile}'.");
    Ok(())
}

/// Rename a key.
pub fn rename_key(old: &str, new: &str) -> Result<()> {
    let mut creds = WalletCredentials::load()?;
    creds.rename_key(old, new)?;
    if let Err(e) = creds.save() {
        // Rollback keychain rename to avoid desync with wallet.toml
        if !credentials::has_credentials_override() {
            let _ = keychain().rename(new, old);
        }
        return Err(e.into());
    }
    println!("Renamed key '{old}' to '{new}'.");
    Ok(())
}

/// Delete a key.
pub fn delete_key(profile: &str, yes: bool) -> Result<()> {
    let creds = WalletCredentials::load()?;

    if !creds.keys.contains_key(profile) {
        anyhow::bail!("Key '{profile}' not found.");
    }

    if !yes {
        use std::io::IsTerminal;
        if !std::io::stdin().is_terminal() {
            anyhow::bail!("Use --yes for non-interactive delete");
        }

        print!("Delete key '{profile}'? [y/N] ");
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
    creds.delete_key(profile)?;
    creds.save()?;
    println!("Deleted key '{profile}'.");
    Ok(())
}

/// Create a new key with a generated wallet EOA key.
///
/// Generates a random EOA key, stores it in the OS keychain,
/// and creates the account metadata in wallet.toml.
/// The key is not provisioned until `presto login` is run.
pub fn create_key(profile: &str, force: bool) -> Result<()> {
    if credentials::has_credentials_override() {
        anyhow::bail!("Cannot create keys with --private-key flag");
    }

    let mut creds = WalletCredentials::load()?;
    if creds.keys.contains_key(profile) && !force {
        anyhow::bail!(
            "Key '{profile}' already exists. Use --force to overwrite or choose a different name."
        );
    }

    let signer = PrivateKeySigner::random();
    let private_key_hex = format!("0x{}", hex::encode(signer.to_bytes()));
    let address = format!("{}", signer.address());

    // Store wallet EOA key in OS keychain
    keychain()
        .set(profile, &private_key_hex)
        .map_err(|e| anyhow::anyhow!("Failed to store key in keychain: {e}"))?;

    let key = crate::wallet::credentials::Key {
        account_address: address.clone(),
        wallet_key_address: Some(address.clone()),
        ..Default::default()
    };
    creds.keys.insert(profile.to_string(), key);
    creds.active = profile.to_string();
    if let Err(e) = creds.save() {
        let _ = keychain().delete(profile);
        return Err(e.into());
    }

    println!("Created key '{profile}'.");
    println!("  Address: {address}");
    println!("\nNot provisioned until 'presto login'.");
    Ok(())
}

/// Import an existing private key as a wallet EOA for a key.
///
/// Reads the private key from stdin (interactive prompt or pipe),
/// stores it in the OS keychain as the wallet key, and creates
/// key metadata in wallet.toml.
pub fn import_key(
    profile: &str,
    force: bool,
    private_key_arg: Option<String>,
    stdin_key: bool,
) -> Result<()> {
    if credentials::has_credentials_override() {
        anyhow::bail!("Cannot import keys with --private-key flag");
    }

    let mut creds = WalletCredentials::load()?;
    if creds.keys.contains_key(profile) && !force {
        anyhow::bail!(
            "Key '{profile}' already exists. Use --force to overwrite or choose a different name."
        );
    }

    let key_hex = if let Some(pk) = private_key_arg {
        pk
    } else if stdin_key {
        read_private_key_noninteractive()? // single-line from stdin
    } else {
        read_private_key()? // interactive masked prompt or pipe
    };

    // Parse and validate the key using shared helper
    let signer: PrivateKeySigner = crate::wallet::credentials::parse_private_key_signer(&key_hex)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    let private_key_hex = format!("0x{}", hex::encode(signer.to_bytes()));
    let address = format!("{}", signer.address());

    // Store wallet EOA key in OS keychain
    keychain()
        .set(profile, &private_key_hex)
        .map_err(|e| anyhow::anyhow!("Failed to store key in keychain: {e}"))?;

    let key = crate::wallet::credentials::Key {
        account_address: address.clone(),
        wallet_key_address: Some(address.clone()),
        ..Default::default()
    };
    creds.keys.insert(profile.to_string(), key);
    creds.active = profile.to_string();
    if let Err(e) = creds.save() {
        let _ = keychain().delete(profile);
        return Err(e.into());
    }

    println!("Imported key '{profile}'.");
    println!("  Address: {address}");
    println!("\nNot provisioned until 'presto login'.");
    Ok(())
}

/// Read a private key from stdin.
///
/// Disables terminal echo on Unix to avoid leaking the key,
/// or reads silently from a pipe.
fn read_private_key() -> Result<String> {
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

        let key = input.trim().to_string();
        input.zeroize();
        if key.is_empty() {
            anyhow::bail!("No private key provided");
        }
        Ok(key)
    } else {
        // Reading from pipe
        let mut input = String::new();
        io::stdin().lock().read_line(&mut input)?;
        let key = input.trim().to_string();
        input.zeroize();
        if key.is_empty() {
            anyhow::bail!("No private key provided on stdin");
        }
        Ok(key)
    }
}

/// Read a private key from stdin non-interactively (one line, no prompts).
fn read_private_key_noninteractive() -> Result<String> {
    use std::io::{self, BufRead};
    let mut input = String::new();
    io::stdin().lock().read_line(&mut input)?;
    let key = input.trim().to_string();
    if key.is_empty() {
        anyhow::bail!("No private key provided on stdin");
    }
    Ok(key)
}
