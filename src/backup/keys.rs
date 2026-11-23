//! Per-user keys of the backups (MAIR-500), kept in the instance's key manager, never in a backup.
//!
//! Envelope encryption: for each user the key manager holds a key of its own (`mairie360-user-<id>`)
//! that never leaves it; a backup asks it for a fresh data key, encrypts the user's data with the
//! plaintext data key and stores only the data key *wrapped* by the user's key. Destroying the
//! user's key in the key manager (erasure) makes every wrapped data key, so every old backup of the
//! user, impossible to decrypt, without touching the backups.

use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Mutex;

/// A data key: `plaintext` encrypts the user's data and is dropped right after, `wrapped` and
/// `key_id` go into the backup.
pub struct DataKey {
    pub key_id: String,
    pub plaintext: [u8; 32],
    pub wrapped: String,
}

impl std::fmt::Debug for DataKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DataKey({})", self.key_id)
    }
}

// async_trait marks the boxed futures `#[must_use]` again.
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait KeyManager: Send + Sync {
    /// A fresh data key wrapped by the user's key, created on first use.
    ///
    /// # Errors
    ///
    /// A short reason, never a key.
    async fn data_key(&self, user_id: i32) -> Result<DataKey, String>;

    /// The plaintext of a wrapped data key, or `None` when the user's key no longer exists (the
    /// user was erased): their data stays sealed.
    ///
    /// # Errors
    ///
    /// The key manager failed for another reason.
    async fn unwrap(&self, key_id: &str, wrapped: &str) -> Result<Option<[u8; 32]>, String>;

    /// Destroys the user's key. Idempotent: `false` when there was none.
    ///
    /// # Errors
    ///
    /// The key manager failed.
    async fn destroy(&self, user_id: i32) -> Result<bool, String>;
}

/// The name of a user's key in the key manager.
#[must_use]
pub fn key_name(user_id: i32) -> String {
    format!("mairie360-user-{user_id}")
}

fn key32(bytes: &[u8]) -> Result<[u8; 32], String> {
    bytes
        .try_into()
        .map_err(|_| "data key is not 32 bytes".to_owned())
}

/// In-memory key manager for the tests and the local stacks: the "user keys" are random 32-byte
/// keys held in this process, the data keys are wrapped with them (AES-256-GCM).
#[derive(Default)]
pub struct InMemoryKeyManager {
    keys: Mutex<HashMap<String, [u8; 32]>>,
}

impl InMemoryKeyManager {
    /// Whether the user's key exists.
    #[must_use]
    pub fn has_key(&self, user_id: i32) -> bool {
        self.keys
            .lock()
            .map(|k| k.contains_key(&key_name(user_id)))
            .unwrap_or(false)
    }

    /// Every key held (tests check that none of them is in a backup).
    #[must_use]
    pub fn all_keys(&self) -> Vec<[u8; 32]> {
        self.keys
            .lock()
            .map(|k| k.values().copied().collect())
            .unwrap_or_default()
    }
}

#[async_trait]
impl KeyManager for InMemoryKeyManager {
    async fn data_key(&self, user_id: i32) -> Result<DataKey, String> {
        let name = key_name(user_id);
        let master = {
            let mut keys = self
                .keys
                .lock()
                .map_err(|_| "key store poisoned".to_owned())?;
            *keys
                .entry(name.clone())
                .or_insert_with(super::crypto::random_key)
        };
        let plaintext = super::crypto::random_key();
        let wrapped = STANDARD.encode(super::crypto::seal(&master, &plaintext, name.as_bytes())?);
        Ok(DataKey {
            key_id: name,
            plaintext,
            wrapped,
        })
    }

    async fn unwrap(&self, key_id: &str, wrapped: &str) -> Result<Option<[u8; 32]>, String> {
        let master = self
            .keys
            .lock()
            .map_err(|_| "key store poisoned".to_owned())?
            .get(key_id)
            .copied();
        let Some(master) = master else {
            return Ok(None);
        };
        let sealed = STANDARD
            .decode(wrapped)
            .map_err(|_| "wrapped key is not base64".to_owned())?;
        key32(&super::crypto::open(&master, &sealed, key_id.as_bytes())?).map(Some)
    }

    async fn destroy(&self, user_id: i32) -> Result<bool, String> {
        Ok(self
            .keys
            .lock()
            .map_err(|_| "key store poisoned".to_owned())?
            .remove(&key_name(user_id))
            .is_some())
    }
}

/// Scaleway Key Manager (REST API `key-manager/v1alpha1`), one symmetric AES-256-GCM key per user.
/// Configured with `SCW_SECRET_KEY` (an API key of the instance's project, Key Manager rights only),
/// `SCW_DEFAULT_PROJECT_ID`, `SCW_REGION` (`fr-par` by default) and, for tests, `KEY_MANAGER_URL`.
pub struct ScalewayKeyManager {
    http: reqwest::Client,
    base: String,
    secret_key: String,
    project_id: String,
}

#[derive(Deserialize)]
struct KeyList {
    keys: Vec<KeyRef>,
}

#[derive(Deserialize)]
struct KeyRef {
    id: String,
    #[serde(default)]
    name: String,
}

#[derive(Deserialize)]
struct GeneratedDataKey {
    ciphertext: String,
    plaintext: String,
}

#[derive(Deserialize)]
struct Decrypted {
    plaintext: String,
}

impl ScalewayKeyManager {
    /// The key manager of this instance, or `None` when it is not configured.
    #[must_use]
    pub fn from_env(var: &dyn Fn(&str) -> Option<String>) -> Option<Self> {
        let region = var("SCW_REGION").unwrap_or_else(|| "fr-par".to_owned());
        Some(Self {
            http: reqwest::Client::new(),
            base: var("KEY_MANAGER_URL").unwrap_or_else(|| {
                format!("https://api.scaleway.com/key-manager/v1alpha1/regions/{region}")
            }),
            secret_key: var("SCW_SECRET_KEY")?,
            project_id: var("SCW_DEFAULT_PROJECT_ID")?,
        })
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{}{path}", self.base))
            .header("X-Auth-Token", &self.secret_key)
    }

    async fn find(&self, name: &str) -> Result<Option<String>, String> {
        let list: KeyList = self
            .request(reqwest::Method::GET, "/keys")
            .query(&[("project_id", self.project_id.as_str()), ("name", name)])
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| {
                format!(
                    "key manager listing failed ({})",
                    e.status().map_or(0, |s| s.as_u16())
                )
            })?
            .json()
            .await
            .map_err(|_| "unreadable key manager listing".to_owned())?;
        // The API filters by name prefix: keep the exact name only.
        Ok(list.keys.into_iter().find(|k| k.name == name).map(|k| k.id))
    }

    async fn find_or_create(&self, name: &str) -> Result<String, String> {
        if let Some(id) = self.find(name).await? {
            return Ok(id);
        }
        let created: KeyRef = self
            .request(reqwest::Method::POST, "/keys")
            .json(&serde_json::json!({
                "project_id": self.project_id,
                "name": name,
                "description": "Mairie 360 per-user backup key (MAIR-500), destroyed at erasure",
                "usage": { "symmetric_encryption": "aes_256_gcm" },
                "unprotected": true,
            }))
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| {
                format!(
                    "key creation failed ({})",
                    e.status().map_or(0, |s| s.as_u16())
                )
            })?
            .json()
            .await
            .map_err(|_| "unreadable key creation answer".to_owned())?;
        Ok(created.id)
    }
}

#[async_trait]
impl KeyManager for ScalewayKeyManager {
    async fn data_key(&self, user_id: i32) -> Result<DataKey, String> {
        let key_id = self.find_or_create(&key_name(user_id)).await?;
        let generated: GeneratedDataKey = self
            .request(
                reqwest::Method::POST,
                &format!("/keys/{key_id}/generate-data-key"),
            )
            .json(&serde_json::json!({ "algorithm": "aes_256_gcm", "without_plaintext": false }))
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| {
                format!(
                    "data key generation failed ({})",
                    e.status().map_or(0, |s| s.as_u16())
                )
            })?
            .json()
            .await
            .map_err(|_| "unreadable data key".to_owned())?;
        let plaintext = key32(
            &STANDARD
                .decode(generated.plaintext)
                .map_err(|_| "data key is not base64".to_owned())?,
        )?;
        Ok(DataKey {
            key_id,
            plaintext,
            wrapped: generated.ciphertext,
        })
    }

    async fn unwrap(&self, key_id: &str, wrapped: &str) -> Result<Option<[u8; 32]>, String> {
        let response = self
            .request(reqwest::Method::POST, &format!("/keys/{key_id}/decrypt"))
            .json(&serde_json::json!({ "ciphertext": wrapped }))
            .send()
            .await
            .map_err(|_| "key manager unreachable".to_owned())?;
        // A destroyed key: the user was erased, their data stays sealed.
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let decrypted: Decrypted = response
            .error_for_status()
            .map_err(|e| {
                format!(
                    "data key decryption failed ({})",
                    e.status().map_or(0, |s| s.as_u16())
                )
            })?
            .json()
            .await
            .map_err(|_| "unreadable decryption answer".to_owned())?;
        key32(
            &STANDARD
                .decode(decrypted.plaintext)
                .map_err(|_| "data key is not base64".to_owned())?,
        )
        .map(Some)
    }

    async fn destroy(&self, user_id: i32) -> Result<bool, String> {
        let Some(key_id) = self.find(&key_name(user_id)).await? else {
            return Ok(false);
        };
        let response = self
            .request(reqwest::Method::DELETE, &format!("/keys/{key_id}"))
            .send()
            .await
            .map_err(|_| "key manager unreachable".to_owned())?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(false);
        }
        response.error_for_status().map_err(|e| {
            format!(
                "key deletion failed ({})",
                e.status().map_or(0, |s| s.as_u16())
            )
        })?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::{InMemoryKeyManager, KeyManager, ScalewayKeyManager};
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[actix_web::test]
    async fn a_destroyed_key_no_longer_unwraps() {
        let km = InMemoryKeyManager::default();
        let key = km.data_key(7).await.unwrap();
        assert_eq!(
            km.unwrap(&key.key_id, &key.wrapped).await.unwrap(),
            Some(key.plaintext)
        );
        assert!(km.destroy(7).await.unwrap());
        assert!(!km.destroy(7).await.unwrap(), "idempotent");
        assert_eq!(km.unwrap(&key.key_id, &key.wrapped).await.unwrap(), None);
    }

    #[actix_web::test]
    async fn scaleway_requests_carry_the_token_and_find_keys_by_exact_name() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/keys"))
            .and(query_param("name", "mairie360-user-7"))
            .and(header("X-Auth-Token", "secret"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "keys": [{ "id": "k-70", "name": "mairie360-user-70" }, { "id": "k-7", "name": "mairie360-user-7" }]
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/keys/k-7/generate-data-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "key_id": "k-7", "algorithm": "aes_256_gcm", "ciphertext": "wrapped", "plaintext": STANDARD.encode([1u8; 32])
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/keys/k-7/decrypt"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/keys/k-7"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        let uri = server.uri();
        let km = ScalewayKeyManager::from_env(&|name: &str| match name {
            "KEY_MANAGER_URL" => Some(uri.clone()),
            "SCW_SECRET_KEY" => Some("secret".to_owned()),
            "SCW_DEFAULT_PROJECT_ID" => Some("project".to_owned()),
            _ => None,
        })
        .unwrap();
        let key = km.data_key(7).await.unwrap();
        assert_eq!(
            (key.key_id.as_str(), key.wrapped.as_str(), key.plaintext),
            ("k-7", "wrapped", [1u8; 32])
        );
        assert_eq!(
            km.unwrap("k-7", "wrapped").await.unwrap(),
            None,
            "404: the key was destroyed"
        );
        assert!(km.destroy(7).await.unwrap());
        assert!(ScalewayKeyManager::from_env(&|_| None).is_none());
    }
}
