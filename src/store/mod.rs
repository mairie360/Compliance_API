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

/// What the erasure needs outside the database, read before the account is anonymized
/// (`fn_erasure_targets`): `None` once the account is anonymized.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ErasureTargets {
    pub email: String,
    pub keycloak_subject: Option<String>,
}

/// A step of the erasure of a user (`erasure_steps.step`), in the order the orchestrator runs
/// them: the steps that need the e-mail or the Keycloak subject come before the database step,
/// which clears them; the ones keyed by the user id come after.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    Keycloak,
    Resend,
    Database,
    S3,
    Redis,
    BackupKey,
}

impl Step {
    pub const ORDER: [Self; 6] = [
        Self::Keycloak,
        Self::Resend,
        Self::Database,
        Self::S3,
        Self::Redis,
        Self::BackupKey,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Keycloak => "keycloak",
            Self::Resend => "resend",
            Self::Database => "database",
            Self::S3 => "s3",
            Self::Redis => "redis",
            Self::BackupKey => "backup_key",
        }
    }

    #[must_use]
    pub const fn storage(self) -> Storage {
        match self {
            Self::Keycloak => Storage::Keycloak,
            Self::Resend => Storage::Resend,
            Self::Database => Storage::Database,
            Self::S3 => Storage::S3,
            Self::Redis => Storage::Redis,
            Self::BackupKey => Storage::Backup,
        }
    }

    /// Whether the step needs the e-mail or the Keycloak subject (so runs before the database step).
    #[must_use]
    pub const fn needs_targets(self) -> bool {
        matches!(self, Self::Keycloak | Self::Resend)
    }
}

/// `erasure_steps.status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Done,
    Failed,
}

/// A row of `erasure_steps`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct StepState {
    pub step: Step,
    pub status: StepStatus,
    pub attempts: i32,
    /// A short reason, never a value of the user.
    pub last_error: Option<String>,
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
    /// Whether the account exists (archived or anonymized ones included).
    async fn user_exists(&self, user_id: i32) -> Result<bool, StoreError>;
    /// `fn_erasure_targets(user_id)`.
    async fn erasure_targets(&self, user_id: i32) -> Result<Option<ErasureTargets>, StoreError>;
    /// Creates the missing `erasure_steps` rows of `user_id` (pending), keeps the others.
    async fn ensure_steps(&self, user_id: i32) -> Result<(), StoreError>;
    /// The steps of `user_id`.
    async fn steps(&self, user_id: i32) -> Result<Vec<StepState>, StoreError>;
    /// Records the outcome of one attempt of a step.
    async fn record_step(
        &self,
        user_id: i32,
        step: Step,
        status: StepStatus,
        error: Option<&str>,
    ) -> Result<(), StoreError>;
    /// The users whose erasure is not finished.
    async fn unfinished_erasures(&self) -> Result<Vec<i32>, StoreError>;
    /// `anonymize_user(user_id)` (MAIR-289).
    async fn anonymize(&self, user_id: i32) -> Result<(), StoreError>;
}

/// A store failure, described without the data (the lib's errors are already value-free).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreError(pub String);

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
