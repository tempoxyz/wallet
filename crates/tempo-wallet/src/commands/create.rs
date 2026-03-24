//! Create command — OWS-native wallet (fully headless, no passkey).
//!
//! Creates a wallet where OWS IS the root key. No browser flow, no
//! passkey, no key authorization. The OWS wallet's EVM address becomes
//! the on-chain wallet address. Signing uses `TempoSigningMode::Direct`.

use tempo_common::{
    cli::{context::Context, output::OutputFormat},
    error::TempoError,
    keys::WalletType,
};

pub(crate) async fn run(ctx: &Context) -> Result<(), TempoError> {
    // Check if wallet already exists.
    if ctx.keys.has_wallet() {
        let msg = if ctx.output_format == OutputFormat::Text {
            format!(
                "Wallet already configured: {}. Use `tempo wallet logout --yes` first to reset.",
                ctx.keys.wallet_address()
            )
        } else {
            format!(
                "Wallet already configured: {}",
                ctx.keys.wallet_address()
            )
        };
        return Err(TempoError::Config(
            tempo_common::error::ConfigError::Missing(msg),
        ));
    }

    // Create an OWS wallet — key generated and encrypted in vault.
    let mut nonce = [0u8; 4];
    getrandom::fill(&mut nonce).map_err(|source| {
        tempo_common::error::KeyError::SigningOperationSource {
            operation: "generate wallet nonce",
            source: Box::new(source),
        }
    })?;
    let ows_wallet_name = format!("{}-{}", ctx.network.as_str(), hex::encode(nonce));
    let ows_id = tempo_common::keys::ows::create_wallet(&ows_wallet_name)?;

    // Export briefly to derive the address, then wipe.
    let exported_key = tempo_common::keys::ows::export_private_key(&ows_id)?;
    let signer = tempo_common::keys::parse_private_key_signer(&exported_key)?;
    let address = signer.address();
    drop(exported_key);

    // Save to keys.toml — wallet_type Local, wallet == key (Direct mode).
    let mut keys = ctx.keys.clone();
    let entry = keys.upsert_by_wallet_address_and_chain(address, ctx.network.chain_id());
    entry.wallet_type = WalletType::Local;
    entry.set_wallet_address(address);
    entry.set_key_address(Some(address));
    entry.ows_id = Some(ows_id);
    keys.save()?;

    match ctx.output_format {
        OutputFormat::Text => {
            eprintln!("Wallet created!");
            eprintln!("Address:  {address:#x}");
            eprintln!("Network:  {}", ctx.network.as_str());
            eprintln!();
            eprintln!("Private key encrypted in OWS vault.");
            eprintln!("No spending limits — this key has full control of the wallet.");
            eprintln!();
            eprintln!("Fund your wallet: tempo wallet fund -n {}", ctx.network.as_str());
        }
        _ => {
            println!(
                "{}",
                serde_json::json!({
                    "wallet": format!("{address:#x}"),
                    "network": ctx.network.as_str(),
                    "type": "ows",
                })
            );
        }
    }

    Ok(())
}
