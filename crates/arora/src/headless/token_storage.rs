//! File-based refresh-token encryption.
//!
//! Ported ~verbatim from `studio-bridge/headless/src/token_storage.rs`. The
//! refresh token is encrypted with XSalsa20Poly1305 (`crypto_secretbox`) under a
//! 32-byte key stored next to it; [`save_token`] generates the key on first use.

use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;

use crypto_secretbox::aead::generic_array::GenericArray;
use crypto_secretbox::Key;
use crypto_secretbox::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Nonce, XSalsa20Poly1305,
};
use log::{debug, error, info, trace, warn};

const KEY_SIZE: usize = 32;
const NONCE_SIZE: usize = 24;

fn generate_key(key_path: &PathBuf) -> Result<Key, std::io::Error> {
    trace!("Generating new encryption key at {:?}", key_path);
    let key: Key = XSalsa20Poly1305::generate_key(&mut OsRng);
    let mut file = File::create(key_path)?;
    file.write_all(key.as_slice())?;
    info!("Generated new encryption key at {:?}", key_path);
    Ok(key)
}

fn load_key(key_path: &PathBuf) -> Result<Key, std::io::Error> {
    trace!("Attempting to load encryption key from {:?}", key_path);
    let mut file = File::open(key_path)?;
    let mut key_bytes = [0u8; KEY_SIZE];
    file.read_exact(&mut key_bytes)?;
    let key: Key = *GenericArray::from_slice(&key_bytes);
    info!("Loaded encryption key from {:?}", key_path);
    Ok(key)
}

pub fn save_token(
    key_path: &PathBuf,
    token_path: &PathBuf,
    token: &str,
) -> Result<(), std::io::Error> {
    trace!(
        "Saving token to {:?} using key from {:?}",
        token_path,
        key_path
    );
    let key = if let Ok(key) = load_key(key_path) {
        key
    } else {
        warn!(
            "Encryption key not found at {:?}, generating a new one.",
            key_path
        );
        generate_key(key_path)?
    };
    let cipher = XSalsa20Poly1305::new(&key);
    let nonce: Nonce = XSalsa20Poly1305::generate_nonce(&mut OsRng); // unique per message
    assert_eq!(nonce.len(), NONCE_SIZE);
    let ciphertext = cipher
        .encrypt(&nonce, token.as_bytes())
        .expect("failed to encrypt token");
    trace!("Token encrypted successfully.");

    if let Some(parent) = token_path.parent() {
        if !parent.exists() {
            trace!("Creating parent directories for token file: {:?}", parent);
            std::fs::create_dir_all(parent)?;
            debug!("Created parent directories for token file: {:?}", parent);
        }
    }
    let mut file = File::create(token_path)?;
    file.write_all(nonce.as_slice())?;
    file.write_all(&ciphertext)?;
    info!("Token saved successfully to {:?}", token_path);
    Ok(())
}

pub fn load_token(
    key_path: &PathBuf,
    token_path: &PathBuf,
) -> Result<Option<String>, std::io::Error> {
    trace!(
        "Attempting to load token from {:?} using key from {:?}",
        token_path,
        key_path
    );
    if !token_path.exists() {
        info!("Token file not found at {:?}", token_path);
        return Ok(None);
    }
    let load_key_result = load_key(key_path);
    let key = match load_key_result {
        Ok(key) => key,
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                warn!(
                    "Encryption key not found at {:?}. Cannot load token.",
                    key_path
                );
                return Ok(None);
            } else {
                error!("Failed to load encryption key from {:?}: {:?}", key_path, e);
                return Err(e);
            }
        }
    };
    let cipher = XSalsa20Poly1305::new(&key);
    trace!("Encryption key loaded.");

    let mut file: File = File::open(token_path).expect("Could not open token file");
    let mut nonce_bytes = [0u8; NONCE_SIZE];
    file.read_exact(&mut nonce_bytes)
        .expect("Could not read nonce from file");
    let nonce: Nonce = *GenericArray::from_slice(&nonce_bytes);
    trace!("Nonce read from token file.");

    let mut ciphertext = Vec::new();
    file.read_to_end(&mut ciphertext)
        .expect("Could not read ciphertext from file");
    trace!("Ciphertext read from token file.");

    let plaintext = cipher.decrypt(&nonce, ciphertext.as_ref()).map_err(|_| {
        error!("Failed to decrypt token from {:?}", token_path);
        std::io::Error::other("Could not decrypt token".to_string())
    })?;
    info!("Token decrypted successfully from {:?}", token_path);
    Ok(Some(String::from_utf8(plaintext).map_err(|e| {
        error!("Failed to convert decrypted token to string: {:?}", e);
        std::io::Error::other(format!("Could not convert token to string: {}", e))
    })?))
}
