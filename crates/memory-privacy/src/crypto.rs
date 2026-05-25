use serde::{Deserialize, Serialize};

use crate::error::{PrivacyError, PrivacyResult};
use crate::keys::KeyProvider;

/// Encrypted payload produced by Stream D before Stream A writes ciphertext.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EncryptedPayload {
    /// Ciphertext bytes passed to Stream A.
    pub ciphertext: Vec<u8>,
    /// Envelope metadata safe to persist in frontmatter extras.
    pub envelope: serde_json::Value,
}

/// Privacy encryption boundary backed by the age file-encryption format.
pub struct PrivacyEncryptor<P> {
    key_provider: P,
}

impl<P: KeyProvider> PrivacyEncryptor<P> {
    /// Create an encryptor from a key provider.
    pub fn new(key_provider: P) -> Self {
        Self { key_provider }
    }

    /// Encrypt plaintext for Stream A's encrypted write path.
    pub fn encrypt(&self, plaintext: &str) -> PrivacyResult<EncryptedPayload> {
        let key = self.key_provider.load_key()?;
        let recipient = key.recipient()?;
        let ciphertext =
            age::encrypt(&recipient, plaintext.as_bytes()).map_err(|err| PrivacyError::Crypto(err.to_string()))?;
        Ok(EncryptedPayload {
            ciphertext,
            envelope: serde_json::json!({
                "scheme": "age-x25519",
                "recipient": key.recipient,
            }),
        })
    }

    /// Decrypt an encrypted payload. Used for rotation tests and local repair tooling.
    pub fn decrypt(&self, payload: &EncryptedPayload) -> PrivacyResult<String> {
        validate_envelope_metadata(&payload.envelope)?;
        let mut last_error = None;
        for key in self.key_provider.load_decryption_keys()? {
            let identity = key.identity()?;
            match age::decrypt(&identity, &payload.ciphertext) {
                Ok(plaintext) => {
                    return String::from_utf8(plaintext).map_err(|err| PrivacyError::Crypto(err.to_string()));
                }
                Err(error) => last_error = Some(error.to_string()),
            }
        }
        Err(PrivacyError::Crypto(last_error.unwrap_or_else(|| "no decryption keys available".to_string())))
    }
}

fn validate_envelope_metadata(envelope: &serde_json::Value) -> PrivacyResult<()> {
    if envelope.is_null() {
        return Ok(());
    }
    let Some(object) = envelope.as_object() else {
        return Err(PrivacyError::Crypto("invalid encryption envelope metadata".to_string()));
    };
    match object.get("scheme").and_then(serde_json::Value::as_str) {
        Some("age-x25519") => Ok(()),
        Some(scheme) => Err(PrivacyError::Crypto(format!("unsupported encryption envelope scheme `{scheme}`"))),
        None => Err(PrivacyError::Crypto("encryption envelope missing scheme".to_string())),
    }
}
