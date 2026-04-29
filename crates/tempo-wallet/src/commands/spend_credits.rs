//! Spend credits via Coinflow redeem flow.

use std::{io::Write, time::Duration};

use alloy::primitives::{keccak256, Address, B256};
use mpp::client::tempo::signing::{KeychainVersion, TempoSigningMode};
use serde::{Deserialize, Serialize};
use tempo_primitives::transaction::{KeychainSignature, TempoSignature};

use crate::commands::fund;
use tempo_common::{
    cli::{context::Context, output, output::OutputFormat},
    error::{ConfigError, InputError, NetworkError, TempoError},
    keys::Signer,
};

const COINFLOW_BLOCKCHAIN: &str = "tempo";
const COINFLOW_AUTH_SUBTOTAL_RETRY_BUFFER_CENTS: u64 = 1;

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

#[derive(Debug, Serialize)]
struct SignatureDebugInfo {
    eip712_digest: String,
    effective_signing_hash: String,
    effective_signing_hash_kind: &'static str,
    signature_length_bytes: usize,
    keychain_embedded_wallet_address: Option<String>,
    keychain_inner_signer_from_eip712_digest: Option<String>,
    recovered_from_eip712_digest: Option<String>,
    recovered_from_effective_signing_hash: Option<String>,
    expected_signer_address: String,
    expected_wallet_address: String,
    matches_keychain_wallet_address: bool,
    matches_keychain_inner_signer_on_eip712_digest: bool,
    matches_signer_on_eip712_digest: bool,
    matches_signer_on_effective_signing_hash: bool,
    matches_wallet_on_eip712_digest: bool,
    matches_wallet_on_effective_signing_hash: bool,
}

struct SubmitRedeemParams<'a> {
    base_url: &'a str,
    wallet: &'a str,
    amount_cents: u64,
    transaction_data: &'a serde_json::Value,
    auth_resp: &'a AuthMsgResponse,
    signature: &'a str,
    signature_debug: &'a SignatureDebugInfo,
    signer_info: &'a Signer,
    output_format: OutputFormat,
}

pub(crate) async fn run(
    ctx: &Context,
    amount_cents: u64,
    to: String,
    data: String,
    value: String,
    address: Option<String>,
) -> Result<(), TempoError> {
    let auth_server_url =
        std::env::var("TEMPO_AUTH_URL").unwrap_or_else(|_| ctx.network.auth_url().to_string());
    let wallet = fund::resolve_credit_address(address, &ctx.keys)?;
    let wallet_address = tempo_common::security::parse_address_input(&wallet, "wallet address")?;
    let transaction_data = build_transaction_data(&to, &data, &value)?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(format!("tempo-wallet/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(NetworkError::Reqwest)?;

    let base_url = build_api_base_url(&auth_server_url)?;

    let mut auth_subtotal_cents = amount_cents;
    let redeem_resp = loop {
        let signer_info = ctx
            .keys
            .signer_for_identity_address(wallet_address, ctx.network)?;

        let auth_resp = request_credits_auth_message(
            &client,
            &base_url,
            &wallet,
            auth_subtotal_cents,
            &transaction_data,
            ctx.output_format,
        )
        .await?;

        if ctx.output_format == OutputFormat::Text {
            eprintln!("Signing authorization...");
        }

        let eip712_digest = compute_eip712_signing_hash(&auth_resp.message)?;
        let signature =
            signer_info.sign_hash_hex(&eip712_digest, "sign EIP-712 credits authorization")?;
        let signature_debug =
            build_signature_debug_info(&signer_info, &wallet, eip712_digest, &signature);

        match submit_redeem_transaction(
            &client,
            SubmitRedeemParams {
                base_url: &base_url,
                wallet: &wallet,
                amount_cents,
                transaction_data: &transaction_data,
                auth_resp: &auth_resp,
                signature: &signature,
                signature_debug: &signature_debug,
                signer_info: &signer_info,
                output_format: ctx.output_format,
            },
        )
        .await
        {
            Ok(response) => break response,
            Err(NetworkError::HttpStatus { body, .. })
                if auth_subtotal_cents == amount_cents
                    && body
                        .as_deref()
                        .is_some_and(is_max_credits_authorized_mismatch) =>
            {
                auth_subtotal_cents =
                    amount_cents.saturating_add(COINFLOW_AUTH_SUBTOTAL_RETRY_BUFFER_CENTS);
                if ctx.output_format == OutputFormat::Text {
                    eprintln!(
                        "Coinflow fee estimate changed between authorization and submit; retrying with refreshed authorization..."
                    );
                }
            }
            Err(error) => return Err(error.into()),
        }
    };

    let result = SpendCreditsResult {
        wallet,
        amount_cents,
        tx_hash: redeem_resp.hash,
    };

    result.render(ctx.output_format)
}

async fn request_credits_auth_message(
    client: &reqwest::Client,
    base_url: &str,
    wallet: &str,
    auth_subtotal_cents: u64,
    transaction_data: &serde_json::Value,
    output_format: OutputFormat,
) -> Result<AuthMsgResponse, TempoError> {
    if output_format == OutputFormat::Text {
        eprintln!("Requesting credits authorization...");
    }

    let auth_msg_url = format!("{base_url}/api/coinflow/redeem/auth-msg");
    let auth_msg_body = serde_json::json!({
        "wallet": wallet,
        "subtotal": {
            "cents": auth_subtotal_cents,
            "currency": "USD"
        },
        "transactionData": transaction_data
    });

    let resp = client
        .post(&auth_msg_url)
        .json(&auth_msg_body)
        .send()
        .await
        .map_err(NetworkError::Reqwest)?;

    let resp_status = resp.status();
    let resp_text = resp.text().await.map_err(NetworkError::Reqwest)?;

    {
        let coinflow_sent = serde_json::json!({
            "coinflow_endpoint": "POST https://api-sandbox.coinflow.cash/api/redeem/evm/creditsAuthMsg",
            "headers": {
                "x-coinflow-auth-wallet": wallet,
                "x-coinflow-auth-blockchain": COINFLOW_BLOCKCHAIN,
            },
            "body": {
                "merchantId": "(from server config)",
                "subtotal": { "cents": auth_subtotal_cents, "currency": "USD" },
                "transactionData": transaction_data,
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

    serde_json::from_str(&resp_text)
        .map_err(|source| NetworkError::ResponseParse {
            context: "auth msg",
            source,
        })
        .map_err(Into::into)
}

async fn submit_redeem_transaction(
    client: &reqwest::Client,
    params: SubmitRedeemParams<'_>,
) -> Result<RedeemResponse, NetworkError> {
    if params.output_format == OutputFormat::Text {
        eprintln!("Submitting redeem transaction...");
    }

    let redeem_url = format!("{}/api/coinflow/redeem/send", params.base_url);
    let redeem_body = serde_json::json!({
        "wallet": params.wallet,
        "subtotal": {
            "cents": params.amount_cents,
            "currency": "USD"
        },
        "transactionData": params.transaction_data,
        "permitCreditsSignature": params.signature,
        "validBefore": params.auth_resp.valid_before,
        "nonce": params.auth_resp.nonce,
        "creditsRawAmount": params.auth_resp.credits_raw_amount
    });

    let resp = client
        .post(&redeem_url)
        .json(&redeem_body)
        .send()
        .await
        .map_err(NetworkError::Reqwest)?;

    let resp_status = resp.status();
    let resp_text = resp.text().await.map_err(NetworkError::Reqwest)?;

    {
        let coinflow_sent = serde_json::json!({
            "coinflow_endpoint": "POST https://api-sandbox.coinflow.cash/api/redeem/evm/sendGaslessTx",
            "headers": {
                "x-coinflow-auth-wallet": params.wallet,
                "x-coinflow-auth-blockchain": COINFLOW_BLOCKCHAIN,
            },
            "body": {
                "merchantId": "(from server config)",
                "subtotal": { "cents": params.amount_cents, "currency": "USD" },
                "transactionData": params.transaction_data,
                "signedMessages": { "permitCredits": params.signature },
                "validBefore": &params.auth_resp.valid_before,
                "nonce": &params.auth_resp.nonce,
                "creditsRawAmount": params.auth_resp.credits_raw_amount,
            },
            "context": {
                "wallet_address": params.wallet,
                "signer_address": format!("{:#x}", params.signer_info.signer.address()),
                "signing_mode": format!("{:?}", params.signer_info.signing_mode),
                "note": "permitCredits is sent using the same format as viem/tempo signTypedData. Direct signers return a raw 65-byte secp256k1 signature. V2 access keys return a 0x04 keychain envelope whose inner signature is over keccak256(0x04 || eip712_digest || wallet_address).",
                "signature_debug": params.signature_debug,
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
        });
    }

    serde_json::from_str(&resp_text).map_err(|source| NetworkError::ResponseParse {
        context: "redeem response",
        source,
    })
}

fn is_max_credits_authorized_mismatch(body: &str) -> bool {
    body.contains("exceeds max credits authorized")
}

fn compute_eip712_signing_hash(message_json: &str) -> Result<B256, TempoError> {
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
    Ok(keccak256(&digest_input))
}

fn effective_signing_hash(signer: &Signer, digest: B256) -> (B256, &'static str) {
    match &signer.signing_mode {
        TempoSigningMode::Direct => (digest, "raw-eip712-digest"),
        TempoSigningMode::Keychain {
            wallet, version, ..
        } => match version {
            KeychainVersion::V1 => (digest, "keychain-v1-raw-eip712-digest"),
            KeychainVersion::V2 => (
                KeychainSignature::signing_hash(digest, *wallet),
                "keychain-v2-wallet-bound-hash",
            ),
        },
    }
}

fn build_signature_debug_info(
    signer: &Signer,
    wallet: &str,
    eip712_digest: B256,
    signature_hex: &str,
) -> SignatureDebugInfo {
    let (effective_hash, effective_hash_kind) = effective_signing_hash(signer, eip712_digest);
    let signature_bytes = hex::decode(signature_hex.trim_start_matches("0x")).unwrap_or_default();
    let parsed_signature = TempoSignature::from_bytes(&signature_bytes).ok();
    let parsed_keychain = parsed_signature
        .as_ref()
        .and_then(TempoSignature::as_keychain);
    let keychain_embedded_wallet_address =
        parsed_keychain.map(|keychain| format!("{:#x}", keychain.user_address));
    let keychain_inner_signer_from_eip712_digest = parsed_keychain
        .and_then(|keychain| keychain.key_id(&eip712_digest).ok())
        .map(|address| format!("{address:#x}"));
    let recovered_from_eip712_digest = parsed_signature
        .as_ref()
        .and_then(|signature| signature.recover_signer(&eip712_digest).ok())
        .map(|address| format!("{address:#x}"));
    let recovered_from_effective_signing_hash = parsed_signature
        .as_ref()
        .and_then(|signature| signature.recover_signer(&effective_hash).ok())
        .map(|address| format!("{address:#x}"));
    let expected_signer_address = format!("{:#x}", signer.signer.address());
    let expected_wallet_address = wallet.to_string();

    SignatureDebugInfo {
        eip712_digest: format!("{eip712_digest:#x}"),
        effective_signing_hash: format!("{effective_hash:#x}"),
        effective_signing_hash_kind: effective_hash_kind,
        signature_length_bytes: signature_bytes.len(),
        keychain_embedded_wallet_address: keychain_embedded_wallet_address.clone(),
        keychain_inner_signer_from_eip712_digest: keychain_inner_signer_from_eip712_digest.clone(),
        recovered_from_eip712_digest: recovered_from_eip712_digest.clone(),
        recovered_from_effective_signing_hash: recovered_from_effective_signing_hash.clone(),
        expected_signer_address: expected_signer_address.clone(),
        expected_wallet_address: expected_wallet_address.clone(),
        matches_keychain_wallet_address: keychain_embedded_wallet_address
            .as_deref()
            .is_some_and(|address| address == expected_wallet_address),
        matches_keychain_inner_signer_on_eip712_digest: keychain_inner_signer_from_eip712_digest
            .as_deref()
            .is_some_and(|address| address == expected_signer_address),
        matches_signer_on_eip712_digest: recovered_from_eip712_digest
            .as_deref()
            .is_some_and(|address| address == expected_signer_address),
        matches_signer_on_effective_signing_hash: recovered_from_effective_signing_hash
            .as_deref()
            .is_some_and(|address| address == expected_signer_address),
        matches_wallet_on_eip712_digest: recovered_from_eip712_digest
            .as_deref()
            .is_some_and(|address| address == expected_wallet_address),
        matches_wallet_on_effective_signing_hash: recovered_from_effective_signing_hash
            .as_deref()
            .is_some_and(|address| address == expected_wallet_address),
    }
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

fn build_transaction_data(
    to: &str,
    data: &str,
    value: &str,
) -> Result<serde_json::Value, TempoError> {
    if !is_zero_value(value)? {
        return Err(ConfigError::Invalid(
            "Coinflow credits redeem does not support non-zero ETH value".to_string(),
        )
        .into());
    }

    if data == "0x" {
        return Ok(serde_json::json!({
            "type": "token",
            "destination": to,
        }));
    }

    Ok(serde_json::json!({
        "transaction": {
            "to": to,
            "data": data,
        },
    }))
}

fn is_zero_value(value: &str) -> Result<bool, TempoError> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(true);
    }

    if let Some(hex_value) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        if !hex_value.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return Err(InputError::InvalidHexInput(format!("invalid ETH value: {value}")).into());
        }
        return Ok(hex_value.is_empty() || hex_value.bytes().all(|byte| byte == b'0'));
    }

    if !value.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(InputError::InvalidHexInput(format!("invalid ETH value: {value}")).into());
    }

    Ok(value.bytes().all(|byte| byte == b'0'))
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

#[cfg(test)]
mod tests {
    use super::*;

    use alloy::primitives::Address;
    use tempo_common::{keys::Keystore, network::NetworkId};
    use zeroize::Zeroizing;

    const TEST_PRIVATE_KEY: &str =
        "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
    const TEST_ADDRESS: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
    const COINFLOW_AUTH_MSG: &str = r#"{"domain":{"name":"Coinflow Credits Contract","version":"1","chainId":42431,"verifyingContract":"0x02af2603e2A7d891684854CBC4aaeBa310bf7C1c"},"message":{"customerWallet":"0x480F8659821A7a5f6209cDA338A53E9Dea09DB46","creditSeed":"tempo-sandbox","amount":1030000,"validBefore":"1777483839","nonce":"0x7968399a1307417362f545e43d5a12eb942562dd8c181d41f68fd881f56ba23d"},"primaryType":"CreditsAuthorization","types":{"EIP712Domain":[{"name":"name","type":"string"},{"name":"version","type":"string"},{"name":"chainId","type":"uint256"},{"name":"verifyingContract","type":"address"}],"CreditsAuthorization":[{"name":"customerWallet","type":"address"},{"name":"creditSeed","type":"string"},{"name":"amount","type":"uint256"},{"name":"validBefore","type":"uint256"},{"name":"nonce","type":"bytes32"}]}}"#;

    #[test]
    fn compute_coinflow_auth_message_matches_reference_hash() {
        let digest = compute_eip712_signing_hash(COINFLOW_AUTH_MSG).unwrap();

        assert_eq!(
            format!("{digest:#x}"),
            "0x3caf17f85e96e489a081eab08cdc14794d8725d4356ef252fc98de3c65e03225"
        );
    }

    #[test]
    fn sign_coinflow_auth_message_with_access_key_matches_viem_keychain_envelope() {
        let mut keys = Keystore::default();
        let wallet: Address = "0x480f8659821a7a5f6209cda338a53e9dea09db46"
            .parse()
            .unwrap();
        let entry = keys.upsert_by_wallet_address_and_chain(wallet, 4217);
        entry.key_address = Some(TEST_ADDRESS.to_string());
        entry.key = Some(Zeroizing::new(TEST_PRIVATE_KEY.to_string()));
        let signer = keys.signer(NetworkId::Tempo).unwrap();
        let digest = compute_eip712_signing_hash(COINFLOW_AUTH_MSG).unwrap();
        let signature = signer
            .sign_hash_hex(&digest, "sign EIP-712 credits authorization")
            .unwrap();
        let signature_bytes = hex::decode(signature.trim_start_matches("0x")).unwrap();
        let parsed = TempoSignature::from_bytes(&signature_bytes).unwrap();
        let keychain = parsed.as_keychain().expect("expected keychain envelope");

        assert_eq!(
            signature,
            "0x04480f8659821a7a5f6209cda338a53e9dea09db46c940b8c39d08d4a737ed58543b2cd922debb9881fb6efc05ac4fa3269b0c28c13e04a2abe52b1aad61b0d5e1b79fe85fcef6f093878d237a7be2e12b1cb7c6c01b"
        );
        assert_eq!(signature.len(), 174, "0x + 86 byte keychain envelope");
        assert!(signature.starts_with("0x04"));
        assert_eq!(parsed.recover_signer(&digest).unwrap(), wallet);
        assert_eq!(keychain.key_id(&digest).unwrap(), signer.signer.address());
    }

    #[test]
    fn build_transaction_data_uses_token_redeem_shape_without_calldata() {
        let transaction_data = build_transaction_data(TEST_ADDRESS, "0x", "0").unwrap();

        assert_eq!(
            transaction_data,
            serde_json::json!({
                "type": "token",
                "destination": TEST_ADDRESS,
            })
        );
    }

    #[test]
    fn build_transaction_data_uses_normal_redeem_shape_with_calldata() {
        let transaction_data = build_transaction_data(TEST_ADDRESS, "0xdeadbeef", "0").unwrap();

        assert_eq!(
            transaction_data,
            serde_json::json!({
                "transaction": {
                    "to": TEST_ADDRESS,
                    "data": "0xdeadbeef",
                },
            })
        );
    }

    #[test]
    fn build_transaction_data_rejects_non_zero_eth_value() {
        let err = build_transaction_data(TEST_ADDRESS, "0xdeadbeef", "1").unwrap_err();

        assert!(err
            .to_string()
            .contains("Coinflow credits redeem does not support non-zero ETH value"));
    }

    #[test]
    fn detects_max_credits_authorized_mismatch() {
        assert!(is_max_credits_authorized_mismatch(
            r#"{"error":"Failed to send redeem transaction","detail":"HTTP 412: {\"message\":\"Error Processing your request\",\"details\":\"Total 1.04 exceeds max credits authorized 1.03\"}"}"#
        ));
    }

    #[test]
    fn ignores_other_coinflow_failures() {
        assert!(!is_max_credits_authorized_mismatch(
            r#"{"error":"Failed to send redeem transaction","detail":"HTTP 412: {\"message\":\"Error Processing your request\",\"details\":\"Wallet does not have enough credits to complete redeem request\"}"}"#
        ));
    }
}
