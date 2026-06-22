//! Cryptographic helpers.
//!
//! Two concerns live here:
//! - `password`: hashing web-ssh account passwords with Argon2id.
//! - `secret`: encrypting/decrypting SSH credentials (password or private key)
//!   at rest with AES-256-GCM, keyed from `Config::master_key`.

use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, OsRng},
};
use anyhow::{Result, anyhow, bail};
use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng as ArgonOsRng},
};

/// Encrypt a secret with AES-256-GCM. Returns (ciphertext, 12-byte nonce).
pub fn encrypt_secret(master_key: &[u8], plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    if master_key.len() != 32 {
        bail!("master key must be 32 bytes");
    }
    let cipher = Aes256Gcm::new_from_slice(master_key)?;
    // russh 0.49 -> aes-gcm 0.10 exposes OsRng via aead.
    let mut nonce_bytes = [0u8; 12];
    use aes_gcm::aead::rand_core::RngCore;
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| anyhow!("encrypt: {e}"))?;
    Ok((ct, nonce_bytes.to_vec()))
}

/// Decrypt a secret previously produced by [`encrypt_secret`].
pub fn decrypt_secret(master_key: &[u8], ciphertext: &[u8], nonce: &[u8]) -> Result<Vec<u8>> {
    if master_key.len() != 32 {
        bail!("master key must be 32 bytes");
    }
    if nonce.len() != 12 {
        bail!("nonce must be 12 bytes");
    }
    let cipher = Aes256Gcm::new_from_slice(master_key)?;
    let nonce = Nonce::from_slice(nonce);
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| anyhow!("decrypt (bad key or corrupted data): {e}"))
}

/// Hash a web-ssh account password with Argon2id.
pub fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut ArgonOsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow!("argon2 hash: {e}"))?
        .to_string();
    Ok(hash)
}

/// Verify a password against a stored Argon2 hash.
pub fn verify_password(password: &str, hash: &str) -> Result<()> {
    let parsed = PasswordHash::new(hash).map_err(|e| anyhow!("bad hash: {e}"))?;
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .map_err(|_| anyhow!("invalid credentials"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_secret() {
        let key = [7u8; 32];
        let msg = "super secret ssh password 🔑".as_bytes();
        let (ct, nonce) = encrypt_secret(&key, msg).unwrap();
        assert_ne!(ct.as_slice(), msg);
        let back = decrypt_secret(&key, &ct, &nonce).unwrap();
        assert_eq!(back, msg);
    }

    #[test]
    fn password_roundtrip() {
        let h = hash_password("hunter2").unwrap();
        assert!(verify_password("hunter2", &h).is_ok());
        assert!(verify_password("wrong", &h).is_err());
    }
}
