//! Spend credits via Coinflow redeem flow.

use std::{io::Write, time::Duration};

use alloy::primitives::{keccak256, Address, B256};
use serde::{Deserialize, Serialize};

use crate::commands::fund;
use tempo_common::{
    cli::{context::Context, output, output::OutputFormat},
    error::{ConfigError, InputError, NetworkError, TempoError},
    keys::Signer,
};

const COINFLOW_BLOCKCHAIN: &str = "tempo";

#[derive(Debug, Deserialize)]
struct AuthMsgResponse {
    message: String,
    #[serde(rename = "validBefore")]
    valid_before: String,
    nonce: String,
    #[serde(rename = "creditsRawAmount")]
    credits_raw_amount: u64,
}

#[derive(Debug, Deserialize)]
struct RedeemResponse {
    hash: String,
}

#[derive(Debug, Serialize)]
struct SpendCreditsResult {
    wallet: String,
    amount_cents: u64,
    tx_hash: String,
}

pub(crate) async fn run(
    ctx: &Context,
    amount_cents: u64,
    to: String,
    _data: String,
    _value: String,
    address: Option<String>,
) -> Result<(), TempoError> {
    let auth_server_url =
        std::env::var("TEMPO_AUTH_URL").unwrap_or_else(|_| ctx.network.auth_url().to_string());
    let wallet = fund::resolve_address(address, &ctx.keys)?;

    let signer_info = ctx.keys.signer(ctx.network)?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(format!("tempo-wallet/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(NetworkError::Reqwest)?;

    let base_url = build_api_base_url(&auth_server_url)?;

    // Step 1: Get credits auth message
    if ctx.output_format == OutputFormat::Text {
        eprintln!("Requesting credits authorization...");
    }

    let auth_msg_url = format!("{base_url}/api/coinflow/redeem/auth-msg");
    let auth_msg_body = serde_json::json!({
        "wallet": wallet,
        "subtotal": {
            "cents": amount_cents,
            "currency": "USD"
        },
        "transactionData": {
            "type": "token",
            "destination": to
        }
    });

    let resp = client
        .post(&auth_msg_url)
        .json(&auth_msg_body)
        .send()
        .await
        .map_err(NetworkError::Reqwest)?;

    let resp_status = resp.status();
    let resp_text = resp.text().await.map_err(NetworkError::Reqwest)?;

    // Write /tmp/coinflow-request.log — what was sent to Coinflow and what it returned
    {
        let coinflow_sent = serde_json::json!({
            "coinflow_endpoint": "POST https://api-sandbox.coinflow.cash/api/redeem/evm/creditsAuthMsg",
            "headers": {
                "x-coinflow-auth-wallet": wallet,
                "x-coinflow-auth-blockchain": COINFLOW_BLOCKCHAIN,
            },
            "body": {
                "merchantId": "(from server config)",
                "subtotal": { "cents": amount_cents, "currency": "USD" },
                "transactionData": { "type": "token", "destination": to },
            },
        });
        let coinflow_returned: serde_json::Value = serde_json::from_str(&resp_text)
            .unwrap_or_else(|_| serde_json::Value::String(resp_text.clone()));
        let log = format!(
            "=== COINFLOW creditsAuthMsg ===\n\n--- SENT ---\n{}\n\n--- RETURNED (HTTP {}) ---\n{}",
            serde_json::to_string_pretty(&coinflow_sent).unwrap_or_default(),
            resp_status.as_u16(),
            serde_json::to_string_pretty(&coinflow_returned).unwrap_or_default(),
        );
        std::fs::write("/tmp/coinflow-request.log", &log).ok();
    }

    if !resp_status.is_success() {
        return Err(NetworkError::HttpStatus {
            operation: "get credits auth message",
            status: resp_status.as_u16(),
            body: Some(resp_text),
        }
        .into());
    }

    let auth_resp: AuthMsgResponse =
        serde_json::from_str(&resp_text).map_err(|source| NetworkError::ResponseParse {
            context: "auth msg",
            source,
        })?;

    // Step 2: Sign the EIP-712 typed data
    if ctx.output_format == OutputFormat::Text {
        eprintln!("Signing authorization...");
    }

    let signature = sign_eip712_message(&signer_info, &auth_resp.message)?;

    // Step 3: Send the redeem transaction
    if ctx.output_format == OutputFormat::Text {
        eprintln!("Submitting redeem transaction...");
    }

    let redeem_url = format!("{base_url}/api/coinflow/redeem/send");
    let redeem_body = serde_json::json!({
        "wallet": wallet,
        "subtotal": {
            "cents": amount_cents,
            "currency": "USD"
        },
        "transactionData": {
            "type": "token",
            "destination": to
        },
        "permitCreditsSignature": signature,
        "validBefore": auth_resp.valid_before,
        "nonce": auth_resp.nonce,
        "creditsRawAmount": auth_resp.credits_raw_amount
    });

    let resp = client
        .post(&redeem_url)
        .json(&redeem_body)
        .send()
        .await
        .map_err(NetworkError::Reqwest)?;

    let resp_status = resp.status();
    let resp_text = resp.text().await.map_err(NetworkError::Reqwest)?;

    // Write /tmp/coinflow-response.log — what was sent to Coinflow sendGaslessTx and what it returned
    {
        let coinflow_sent = serde_json::json!({
            "coinflow_endpoint": "POST https://api-sandbox.coinflow.cash/api/redeem/evm/sendGaslessTx",
            "headers": {
                "x-coinflow-auth-wallet": wallet,
                "x-coinflow-auth-blockchain": COINFLOW_BLOCKCHAIN,
            },
            "body": {
                "merchantId": "(from server config)",
                "subtotal": { "cents": amount_cents, "currency": "USD" },
                "transactionData": { "type": "token", "destination": to },
                "signedMessages": { "permitCredits": &signature },
                "validBefore": &auth_resp.valid_before,
                "nonce": &auth_resp.nonce,
                "creditsRawAmount": auth_resp.credits_raw_amount,
            },
            "context": {
                "wallet_address": wallet,
                "signer_address": format!("{:#x}", signer_info.signer.address()),
                "signing_mode": format!("{:?}", signer_info.signing_mode),
                "note": "Access keys now emit a Tempo keychain signature envelope (0x04...) for ERC-1271 validation against wallet_address. If Coinflow still assumes 65-byte ECDSA and only ecrecovers, signature verification will fail on their side.",
            },
        });
        let coinflow_returned: serde_json::Value = serde_json::from_str(&resp_text)
            .unwrap_or_else(|_| serde_json::Value::String(resp_text.clone()));
        let log = format!(
            "=== COINFLOW sendGaslessTx ===\n\n--- SENT ---\n{}\n\n--- RETURNED (HTTP {}) ---\n{}",
            serde_json::to_string_pretty(&coinflow_sent).unwrap_or_default(),
            resp_status.as_u16(),
            serde_json::to_string_pretty(&coinflow_returned).unwrap_or_default(),
        );
        std::fs::write("/tmp/coinflow-response.log", &log).ok();
    }

    if !resp_status.is_success() {
        return Err(NetworkError::HttpStatus {
            operation: "send redeem transaction",
            status: resp_status.as_u16(),
            body: Some(resp_text),
        }
        .into());
    }

    let redeem_resp: RedeemResponse =
        serde_json::from_str(&resp_text).map_err(|source| NetworkError::ResponseParse {
            context: "redeem response",
            source,
        })?;

    let result = SpendCreditsResult {
        wallet,
        amount_cents,
        tx_hash: redeem_resp.hash,
    };

    result.render(ctx.output_format)
}

/// Compute the EIP-712 hash from the JSON typed data message and sign it.
fn sign_eip712_message(signer: &Signer, message_json: &str) -> Result<String, TempoError> {
    let typed_data: serde_json::Value =
        serde_json::from_str(message_json).map_err(|source| NetworkError::ResponseParse {
            context: "EIP-712 typed data",
            source,
        })?;

    let domain = &typed_data["domain"];
    let domain_separator = compute_domain_separator(domain)?;

    let primary_type = typed_data["primaryType"]
        .as_str()
        .ok_or_else(|| InputError::InvalidHexInput("missing primaryType".to_string()))?;
    let types = &typed_data["types"];
    let message = &typed_data["message"];
    let struct_hash = compute_struct_hash(primary_type, types, message)?;

    // EIP-712 digest: keccak256("\x19\x01" || domainSeparator || structHash)
    let mut digest_input = Vec::with_capacity(66);
    digest_input.extend_from_slice(&[0x19, 0x01]);
    digest_input.extend_from_slice(domain_separator.as_slice());
    digest_input.extend_from_slice(struct_hash.as_slice());
    let digest = keccak256(&digest_input);

    signer.sign_hash_hex(&digest, "sign EIP-712 credits authorization")
}

/// Compute the EIP-712 domain separator hash.
fn compute_domain_separator(domain: &serde_json::Value) -> Result<B256, TempoError> {
    let mut domain_type_parts = vec![];
    let mut domain_values: Vec<Vec<u8>> = vec![];

    if domain.get("name").is_some() {
        domain_type_parts.push("string name");
        let name = domain["name"].as_str().unwrap_or("");
        domain_values.push(keccak256(name.as_bytes()).to_vec());
    }
    if domain.get("version").is_some() {
        domain_type_parts.push("string version");
        let version = domain["version"].as_str().unwrap_or("");
        domain_values.push(keccak256(version.as_bytes()).to_vec());
    }
    if domain.get("chainId").is_some() {
        domain_type_parts.push("uint256 chainId");
        let chain_id = domain["chainId"]
            .as_u64()
            .ok_or_else(|| InputError::InvalidHexInput("invalid chainId".to_string()))?;
        let mut buf = [0u8; 32];
        buf[24..].copy_from_slice(&chain_id.to_be_bytes());
        domain_values.push(buf.to_vec());
    }
    if domain.get("verifyingContract").is_some() {
        domain_type_parts.push("address verifyingContract");
        let addr_str = domain["verifyingContract"]
            .as_str()
            .ok_or_else(|| InputError::InvalidHexInput("invalid verifyingContract".to_string()))?;
        let addr: Address = addr_str.parse().map_err(|_| ConfigError::InvalidAddress {
            context: "EIP-712 domain verifyingContract",
            value: addr_str.to_string(),
        })?;
        let mut buf = [0u8; 32];
        buf[12..].copy_from_slice(addr.as_slice());
        domain_values.push(buf.to_vec());
    }
    if domain.get("salt").is_some() {
        domain_type_parts.push("bytes32 salt");
        let salt_str = domain["salt"]
            .as_str()
            .ok_or_else(|| InputError::InvalidHexInput("invalid salt".to_string()))?;
        let salt_hex = salt_str.strip_prefix("0x").unwrap_or(salt_str);
        let salt_bytes = hex::decode(salt_hex)
            .map_err(|_| InputError::InvalidHexInput("invalid salt hex".to_string()))?;
        domain_values.push(salt_bytes);
    }

    let domain_type_str = format!("EIP712Domain({})", domain_type_parts.join(","));
    let type_hash = keccak256(domain_type_str.as_bytes());

    let mut encoded = Vec::new();
    encoded.extend_from_slice(type_hash.as_slice());
    for val in &domain_values {
        encoded.extend_from_slice(val);
    }

    Ok(keccak256(&encoded))
}

/// Compute the struct hash for a given type, following EIP-712 encoding rules.
fn compute_struct_hash(
    type_name: &str,
    types: &serde_json::Value,
    data: &serde_json::Value,
) -> Result<B256, TempoError> {
    let type_hash = compute_type_hash(type_name, types)?;
    let encoded_data = encode_data(type_name, types, data)?;

    let mut full = Vec::new();
    full.extend_from_slice(type_hash.as_slice());
    full.extend_from_slice(&encoded_data);

    Ok(keccak256(&full))
}

fn compute_type_hash(type_name: &str, types: &serde_json::Value) -> Result<B256, TempoError> {
    let type_str = encode_type(type_name, types)?;
    Ok(keccak256(type_str.as_bytes()))
}

/// Encode a type string including all referenced sub-types (sorted).
fn encode_type(type_name: &str, types: &serde_json::Value) -> Result<String, TempoError> {
    let fields = types[type_name].as_array().ok_or_else(|| {
        InputError::InvalidHexInput(format!("missing type definition for {type_name}"))
    })?;

    let mut params = Vec::new();
    let mut referenced_types = std::collections::BTreeSet::new();

    for field in fields {
        let field_type = field["type"]
            .as_str()
            .ok_or_else(|| InputError::InvalidHexInput("missing field type".to_string()))?;
        let field_name = field["name"]
            .as_str()
            .ok_or_else(|| InputError::InvalidHexInput("missing field name".to_string()))?;
        params.push(format!("{field_type} {field_name}"));

        let base_type = field_type.trim_end_matches("[]");
        if types.get(base_type).is_some() && base_type != type_name {
            collect_referenced_types(base_type, types, &mut referenced_types);
        }
    }

    let primary = format!("{type_name}({})", params.join(","));
    let mut result = primary;
    for ref_type in &referenced_types {
        result.push_str(&encode_type_single(ref_type, types)?);
    }
    Ok(result)
}

fn encode_type_single(type_name: &str, types: &serde_json::Value) -> Result<String, TempoError> {
    let fields = types[type_name].as_array().ok_or_else(|| {
        InputError::InvalidHexInput(format!("missing type definition for {type_name}"))
    })?;
    let mut params = Vec::new();
    for field in fields {
        let field_type = field["type"].as_str().unwrap_or("");
        let field_name = field["name"].as_str().unwrap_or("");
        params.push(format!("{field_type} {field_name}"));
    }
    Ok(format!("{type_name}({})", params.join(",")))
}

fn collect_referenced_types(
    type_name: &str,
    types: &serde_json::Value,
    collected: &mut std::collections::BTreeSet<String>,
) {
    if !collected.insert(type_name.to_string()) {
        return;
    }
    if let Some(fields) = types[type_name].as_array() {
        for field in fields {
            if let Some(field_type) = field["type"].as_str() {
                let base_type = field_type.trim_end_matches("[]");
                if types.get(base_type).is_some() && base_type != type_name {
                    collect_referenced_types(base_type, types, collected);
                }
            }
        }
    }
}

/// Encode the data values according to EIP-712 rules.
fn encode_data(
    type_name: &str,
    types: &serde_json::Value,
    data: &serde_json::Value,
) -> Result<Vec<u8>, TempoError> {
    let fields = types[type_name].as_array().ok_or_else(|| {
        InputError::InvalidHexInput(format!("missing type definition for {type_name}"))
    })?;

    let mut encoded = Vec::new();

    for field in fields {
        let field_type = field["type"]
            .as_str()
            .ok_or_else(|| InputError::InvalidHexInput("missing field type".to_string()))?;
        let field_name = field["name"]
            .as_str()
            .ok_or_else(|| InputError::InvalidHexInput("missing field name".to_string()))?;
        let value = &data[field_name];

        let encoded_value = encode_value(field_type, types, value)?;
        encoded.extend_from_slice(&encoded_value);
    }

    Ok(encoded)
}

/// Encode a single value according to its EIP-712 type.
fn encode_value(
    field_type: &str,
    types: &serde_json::Value,
    value: &serde_json::Value,
) -> Result<Vec<u8>, TempoError> {
    // Handle array types
    if let Some(base_type) = field_type.strip_suffix("[]") {
        let items = value
            .as_array()
            .ok_or_else(|| InputError::InvalidHexInput("expected array value".to_string()))?;
        let mut inner = Vec::new();
        for item in items {
            inner.extend_from_slice(&encode_value(base_type, types, item)?);
        }
        return Ok(keccak256(&inner).to_vec());
    }

    // Handle struct types (referenced custom types)
    if types.get(field_type).is_some() {
        let hash = compute_struct_hash(field_type, types, value)?;
        return Ok(hash.to_vec());
    }

    // Handle atomic types
    match field_type {
        "address" => {
            let addr_str = value.as_str().ok_or_else(|| {
                InputError::InvalidHexInput("expected address string".to_string())
            })?;
            let addr: Address = addr_str.parse().map_err(|_| ConfigError::InvalidAddress {
                context: "EIP-712 field",
                value: addr_str.to_string(),
            })?;
            let mut buf = [0u8; 32];
            buf[12..].copy_from_slice(addr.as_slice());
            Ok(buf.to_vec())
        }
        "bool" => {
            let b = value.as_bool().unwrap_or(false);
            let mut buf = [0u8; 32];
            if b {
                buf[31] = 1;
            }
            Ok(buf.to_vec())
        }
        "string" => {
            let s = value.as_str().unwrap_or("");
            Ok(keccak256(s.as_bytes()).to_vec())
        }
        "bytes" => {
            let hex_str = value.as_str().unwrap_or("0x");
            let hex_clean = hex_str.strip_prefix("0x").unwrap_or(hex_str);
            let bytes = hex::decode(hex_clean)
                .map_err(|_| InputError::InvalidHexInput("invalid bytes hex".to_string()))?;
            Ok(keccak256(&bytes).to_vec())
        }
        t if t.starts_with("bytes") => {
            // bytesN (fixed-size)
            let hex_str = value.as_str().unwrap_or("0x");
            let hex_clean = hex_str.strip_prefix("0x").unwrap_or(hex_str);
            let bytes = hex::decode(hex_clean)
                .map_err(|_| InputError::InvalidHexInput("invalid bytesN hex".to_string()))?;
            let mut buf = [0u8; 32];
            let len = bytes.len().min(32);
            buf[..len].copy_from_slice(&bytes[..len]);
            Ok(buf.to_vec())
        }
        t if t.starts_with("uint") || t.starts_with("int") => {
            let mut buf = [0u8; 32];
            if let Some(n) = value.as_u64() {
                buf[24..].copy_from_slice(&n.to_be_bytes());
            } else if let Some(s) = value.as_str() {
                if let Some(hex_val) = s.strip_prefix("0x") {
                    let bytes = hex::decode(hex_val)
                        .map_err(|_| InputError::InvalidHexInput("invalid uint hex".to_string()))?;
                    let start = 32 - bytes.len().min(32);
                    buf[start..start + bytes.len().min(32)]
                        .copy_from_slice(&bytes[..bytes.len().min(32)]);
                } else if let Ok(n) = s.parse::<u128>() {
                    buf[16..].copy_from_slice(&n.to_be_bytes());
                } else {
                    let n: u64 = s.parse().map_err(|_| {
                        InputError::InvalidHexInput(format!("invalid numeric value: {s}"))
                    })?;
                    buf[24..].copy_from_slice(&n.to_be_bytes());
                }
            } else if let Some(n) = value.as_i64() {
                buf[24..].copy_from_slice(&(n as u64).to_be_bytes());
            }
            Ok(buf.to_vec())
        }
        _ => {
            let hex_str = value.as_str().unwrap_or("0x");
            let hex_clean = hex_str.strip_prefix("0x").unwrap_or(hex_str);
            let bytes = hex::decode(hex_clean).unwrap_or_default();
            let mut buf = [0u8; 32];
            let len = bytes.len().min(32);
            buf[..len].copy_from_slice(&bytes[..len]);
            Ok(buf.to_vec())
        }
    }
}

fn build_api_base_url(auth_server_url: &str) -> Result<String, TempoError> {
    let url = url::Url::parse(auth_server_url).map_err(|source| InputError::UrlParseFor {
        context: "auth server",
        source,
    })?;
    Ok(url.origin().ascii_serialization())
}

impl SpendCreditsResult {
    fn render(&self, format: OutputFormat) -> Result<(), TempoError> {
        output::emit_by_format(format, self, || {
            let w = &mut std::io::stdout();
            writeln!(w, "{:>10}: {}", "Wallet", self.wallet)?;
            writeln!(
                w,
                "{:>10}: ${:.2}",
                "Amount",
                self.amount_cents as f64 / 100.0
            )?;
            writeln!(w, "{:>10}: {}", "TX Hash", self.tx_hash)?;
            Ok(())
        })
    }
}
