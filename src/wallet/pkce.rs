use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use sha2::{Digest, Sha256};

pub fn generate_code_verifier() -> String {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).expect("failed to generate random bytes");
    hex::encode(bytes)[..43].to_string()
}

pub fn compute_code_challenge(code_verifier: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(code_verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_code_challenge_produces_43_char_base64url() {
        let verifier = "test-code-verifier-12345678901234567890";
        let challenge = compute_code_challenge(verifier);
        assert_eq!(challenge.len(), 43);
        assert!(challenge
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn test_code_challenge_is_deterministic() {
        let verifier = "deterministic-verifier";
        let c1 = compute_code_challenge(verifier);
        let c2 = compute_code_challenge(verifier);
        assert_eq!(c1, c2);
    }

    #[test]
    fn test_different_inputs_produce_different_outputs() {
        let c1 = compute_code_challenge("input-a");
        let c2 = compute_code_challenge("input-b");
        assert_ne!(c1, c2);
    }

    #[test]
    fn test_generate_code_verifier_length() {
        let verifier = generate_code_verifier();
        assert_eq!(verifier.len(), 43);
    }

    #[test]
    fn test_generate_code_verifier_is_hex() {
        let verifier = generate_code_verifier();
        assert!(verifier.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
