//! What the compliance service reads and writes in Postgres (MAIR-498), behind a trait so that the
//! scan and the erasure are tested without a database. `pg::PgStore` is the real one: the role
//! `compliance_api` reads no personal table, only the `SECURITY DEFINER` functions of
//! Devops/Database (`repeatable/security/compliance.sql`), its journal and the erasure steps.

pub mod pg;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// A finding of the database scan: counts and locations, never a value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Finding {
    /// `retention_overdue`, `archived_overdue` or `erased_user_data`.
    pub kind: String,
    /// Table, or `table.column`.
    pub location: String,
    pub rows: i64,
    /// The rule that was broken (a period, a reason), never a value.
    pub detail: String,
}

/// Where a journal entry happened (enum `compliance_storage` of the database).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Storage {
    Database,
    Logs,
    S3,
    Redis,
    Keycloak,
    Resend,
    Backup,
}

/// What the service did (enum `compliance_action`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Detected,
    Masked,
    Erased,
    Failed,
}

/// One row of `compliance_journal`: never the value itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalEntry {
    pub kind: String,
    pub storage: Storage,
    pub location: String,
    pub action: Action,
    pub rows: Option<i64>,
    pub masked_excerpt: Option<String>,
    pub cause_hint: Option<String>,
    pub user_id: Option<i32>,
}

impl JournalEntry {
    #[must_use]
    pub fn finding(finding: &Finding) -> Self {
        Self {
            kind: finding.kind.clone(),
            storage: Storage::Database,
            location: finding.location.clone(),
            action: Action::Detected,
            rows: Some(finding.rows),
            masked_excerpt: None,
            cause_hint: Some(finding.detail.clone()),
            user_id: None,
        }
    }
}

/// The store the scan and the erasure use.
// async_trait marks the boxed futures `#[must_use]` again.
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait ComplianceStore: Send + Sync {
    /// `fn_compliance_scan()`.
    async fn scan(&self) -> Result<Vec<Finding>, StoreError>;
    /// Appends to `compliance_journal`.
    async fn journal(&self, entry: &JournalEntry) -> Result<(), StoreError>;
}

/// A store failure, described without the data (the lib's errors are already value-free).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreError(pub String);

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
