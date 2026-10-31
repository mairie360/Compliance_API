//! The per-user backup key (MAIR-500): the backups encrypt each user's personal data with a data
//! key wrapped by a key of the user in the instance's key manager. Destroying that key makes the
//! user unreadable in every old backup without touching them (crypto-shredding). Without a key
//! manager on this instance the step is recorded as not configured.

use super::{Connector, ErasureContext, Outcome};
use crate::backup::keys::KeyManager;
use async_trait::async_trait;

pub struct BackupKeyConnector {
    keys: Option<Box<dyn KeyManager>>,
}

impl BackupKeyConnector {
    #[must_use]
    pub fn new(keys: Option<Box<dyn KeyManager>>) -> Self {
        Self { keys }
    }
}

#[async_trait]
impl Connector for BackupKeyConnector {
    async fn erase(&self, context: &ErasureContext) -> Result<Outcome, String> {
        let Some(keys) = &self.keys else {
            return Ok(Outcome::NotConfigured);
        };
        // Idempotent: a key already destroyed (or never created) is a success.
        keys.destroy(context.user_id).await?;
        Ok(Outcome::Erased)
    }
}

#[cfg(test)]
mod tests {
    use super::BackupKeyConnector;
    use crate::backup::keys::{InMemoryKeyManager, KeyManager};
    use crate::erasure::connectors::{Connector, ErasureContext, Outcome};
    use std::sync::Arc;

    struct Shared(Arc<InMemoryKeyManager>);

    #[async_trait::async_trait]
    impl KeyManager for Shared {
        async fn data_key(&self, user_id: i32) -> Result<crate::backup::keys::DataKey, String> {
            self.0.data_key(user_id).await
        }
        async fn unwrap(&self, key_id: &str, wrapped: &str) -> Result<Option<[u8; 32]>, String> {
            self.0.unwrap(key_id, wrapped).await
        }
        async fn destroy(&self, user_id: i32) -> Result<bool, String> {
            self.0.destroy(user_id).await
        }
    }

    #[actix_web::test]
    async fn the_erasure_destroys_the_users_key_and_is_idempotent() {
        let km = Arc::new(InMemoryKeyManager::default());
        km.data_key(42).await.unwrap();
        let connector = BackupKeyConnector::new(Some(Box::new(Shared(km.clone()))));
        let context = ErasureContext {
            user_id: 42,
            targets: None,
        };
        assert_eq!(connector.erase(&context).await, Ok(Outcome::Erased));
        assert!(!km.has_key(42));
        assert_eq!(
            connector.erase(&context).await,
            Ok(Outcome::Erased),
            "again: nothing left"
        );
        assert_eq!(
            BackupKeyConnector::new(None).erase(&context).await,
            Ok(Outcome::NotConfigured)
        );
    }
}
