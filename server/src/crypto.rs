//! AES-256-GCM protection for secrets stored at rest.
//!
//! The key is derived from a server secret. Each record's UUID is bound as
//! associated data, so a ciphertext cannot be decrypted under a different row id.

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Clone)]
pub struct SecretCipher {
    key: [u8; 32],
}

impl SecretCipher {
    pub fn new(secret: &str) -> Self {
        Self {
            key: Sha256::digest(secret.as_bytes()).into(),
        }
    }

    pub fn encrypt(&self, id: Uuid, plaintext: &str) -> Result<String, String> {
        let cipher = Aes256Gcm::new_from_slice(&self.key).map_err(|_| "invalid key".to_string())?;
        let mut nonce_bytes = [0_u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce_bytes),
                aes_gcm::aead::Payload {
                    msg: plaintext.as_bytes(),
                    aad: id.as_bytes(),
                },
            )
            .map_err(|_| "failed to encrypt secret".to_string())?;
        let mut encoded = nonce_bytes.to_vec();
        encoded.extend(ciphertext);
        Ok(BASE64.encode(encoded))
    }

    pub fn decrypt(&self, id: Uuid, encoded: &str) -> Result<String, String> {
        let bytes = BASE64
            .decode(encoded)
            .map_err(|_| "invalid encrypted secret".to_string())?;
        let (nonce, ciphertext) = bytes
            .split_at_checked(12)
            .ok_or_else(|| "invalid encrypted secret".to_string())?;
        let cipher = Aes256Gcm::new_from_slice(&self.key).map_err(|_| "invalid key".to_string())?;
        let plaintext = cipher
            .decrypt(
                Nonce::from_slice(nonce),
                aes_gcm::aead::Payload {
                    msg: ciphertext,
                    aad: id.as_bytes(),
                },
            )
            .map_err(|_| "failed to decrypt secret".to_string())?;
        String::from_utf8(plaintext).map_err(|_| "invalid decrypted secret".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_binds_to_id() {
        let cipher = SecretCipher::new("secret");
        let id = Uuid::now_v7();
        let other = Uuid::now_v7();

        let encrypted = cipher.encrypt(id, "token").unwrap();
        assert_ne!(encrypted, "token");
        assert_eq!(cipher.decrypt(id, &encrypted).unwrap(), "token");
        // The row id is bound as associated data, so a different id cannot decrypt.
        assert!(cipher.decrypt(other, &encrypted).is_err());
    }
}
