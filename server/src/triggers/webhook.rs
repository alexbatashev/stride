//! Helpers for webhook-triggered automations. Each webhook automation has an
//! opaque secret; callers authenticate by presenting it. No routing here — the
//! API layer owns the endpoint.

use rand_core::{OsRng, RngCore};

/// Generate a 256-bit hex secret for a new webhook automation.
pub fn generate_secret() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let mut out = String::with_capacity(64);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Constant-time comparison so a mismatch does not leak position via timing.
pub fn verify_secret(expected: &str, provided: &str) -> bool {
    let expected = expected.as_bytes();
    let provided = provided.as_bytes();
    if expected.is_empty() || expected.len() != provided.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in expected.iter().zip(provided.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_is_unique_and_hex() {
        let a = generate_secret();
        let b = generate_secret();
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn verify_matches_only_exact() {
        let secret = generate_secret();
        assert!(verify_secret(&secret, &secret));
        assert!(!verify_secret(&secret, "wrong"));
        assert!(!verify_secret(&secret, ""));
        assert!(!verify_secret("", ""));
    }
}
