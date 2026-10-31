//! The per-user backup key (MAIR-500): the backups encrypt each user's data with a key kept in the
//! instance's key manager, destroyed at erasure. Not built yet: the step is recorded as not
//! configured until MAIR-500 replaces this connector.

use super::{Connector, ErasureContext, Outcome};
use async_trait::async_trait;

pub struct BackupKeyConnector;

#[async_trait]
impl Connector for BackupKeyConnector {
    async fn erase(&self, _context: &ErasureContext) -> Result<Outcome, String> {
        Ok(Outcome::NotConfigured)
    }
}
