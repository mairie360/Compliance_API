//! Where the erasure of a user is propagated outside the database (MAIR-498). Each connector
//! erases what its service holds about the user and is idempotent: erasing twice, or erasing what
//! is already gone, is a success. A connector without configuration on this instance answers
//! `NotConfigured`: the step is recorded as done with that reason (there is nothing to erase there).

pub mod backup_key;
pub mod keycloak;
pub mod redis_keys;
pub mod resend;
pub mod s3;

use crate::store::ErasureTargets;
use async_trait::async_trait;

/// What a connector receives: the user id, and the targets read before the anonymization (absent
/// for the steps that run after it).
#[derive(Debug, Clone)]
pub struct ErasureContext {
    pub user_id: i32,
    pub targets: Option<ErasureTargets>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Erased, or nothing was left to erase.
    Erased,
    /// The service is not configured on this instance.
    NotConfigured,
}

// async_trait marks the boxed futures `#[must_use]` again.
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait Connector: Send + Sync {
    /// Erases what the service holds about the user.
    ///
    /// # Errors
    ///
    /// A short reason (status code, kind of failure), never a value of the user.
    async fn erase(&self, context: &ErasureContext) -> Result<Outcome, String>;
}

/// The connectors of the external steps (the database step is the store's `anonymize`).
pub struct Connectors {
    pub keycloak: Box<dyn Connector>,
    pub resend: Box<dyn Connector>,
    pub s3: Box<dyn Connector>,
    pub redis: Box<dyn Connector>,
    pub backup_key: Box<dyn Connector>,
}

impl Connectors {
    /// The connectors of this instance, from the environment (see `CLAUDE.md`).
    #[must_use]
    pub fn from_env() -> Self {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
        Self {
            keycloak: Box::new(keycloak::KeycloakConnector::from_env(&var)),
            resend: Box::new(resend::ResendConnector::from_env(&var)),
            s3: Box::new(s3::S3Connector::from_env(&var)),
            redis: Box::new(redis_keys::RedisConnector::from_env(&var)),
            backup_key: Box::new(backup_key::BackupKeyConnector),
        }
    }
}
