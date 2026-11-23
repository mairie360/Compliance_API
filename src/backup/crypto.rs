//! AES-256-GCM of the backups (MAIR-500): a random 96-bit nonce per message, stored in front of the
//! ciphertext; `aad` binds a sealed blob to its user (a blob moved to another user fails to open).

use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng, Payload};
use aes_gcm::{Aes256Gcm, Nonce};

const NONCE_LEN: usize = 12;

/// A random 256-bit key.
#[must_use]
pub fn random_key() -> [u8; 32] {
    Aes256Gcm::generate_key(OsRng).into()
}

/// `nonce || ciphertext` of `plaintext`.
///
/// # Errors
///
/// The cipher failed (never with a valid key).
pub fn seal(key: &[u8; 32], plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>, String> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| "invalid key".to_owned())?;
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let mut out = nonce.to_vec();
    out.extend(
        cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext,
                    aad,
                },
            )
            .map_err(|_| "encryption failed".to_owned())?,
    );
    Ok(out)
}

/// The plaintext of `seal`'s output.
///
/// # Errors
///
/// Wrong key, wrong `aad`, or tampered data.
pub fn open(key: &[u8; 32], sealed: &[u8], aad: &[u8]) -> Result<Vec<u8>, String> {
    if sealed.len() < NONCE_LEN {
        return Err("sealed data too short".to_owned());
    }
    let (nonce, ciphertext) = sealed.split_at(NONCE_LEN);
    // `from_slice` is deprecated by generic-array 0.14, the version aes-gcm 0.10 exposes.
    #[allow(deprecated)]
    let nonce = Nonce::from_slice(nonce);
    Aes256Gcm::new_from_slice(key)
        .map_err(|_| "invalid key".to_owned())?
        .decrypt(
            nonce,
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| "decryption failed".to_owned())
}

#[cfg(test)]
mod tests {
    use super::{open, random_key, seal};

    #[test]
    fn sealed_data_opens_only_with_its_key_and_aad() {
        let key = random_key();
        let sealed = seal(&key, b"jean.dupont@example.com", b"user-7").unwrap();
        assert!(
            !sealed.windows(4).any(|w| w == b"jean"),
            "no plaintext in the output"
        );
        assert_eq!(
            open(&key, &sealed, b"user-7").unwrap(),
            b"jean.dupont@example.com"
        );
        assert!(open(&key, &sealed, b"user-8").is_err());
        assert!(open(&random_key(), &sealed, b"user-7").is_err());
        assert_ne!(
            seal(&key, b"x", b"").unwrap(),
            seal(&key, b"x", b"").unwrap(),
            "fresh nonce"
        );
    }
}
