//! `ComplianceStore` on Postgres, as the `compliance_api` role (MAIR-498).

use super::{
    ComplianceStore, ErasureTargets, Finding, JournalEntry, Step, StepState, StepStatus, StoreError,
};
use async_trait::async_trait;
use mairie360_api_lib::database::db_interface::{ApiRequestDto, QueryParam};
use mairie360_api_lib::smart_db::SmartDatabase;
use serde::Deserialize;

pub struct PgStore {
    db: SmartDatabase,
}

impl PgStore {
    #[must_use]
    pub const fn new(db: SmartDatabase) -> Self {
        Self { db }
    }
}

#[derive(Debug, Deserialize)]
struct ScanQueryView;

impl ApiRequestDto for ScanQueryView {
    fn query_sql(&self) -> &'static str {
        "SELECT row_to_json(t) FROM (SELECT kind, location, rows, detail FROM fn_compliance_scan()) t"
    }

    fn query_params(&self) -> &[QueryParam] {
        &[]
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct JournalInsertView {
    params: Vec<QueryParam>,
}

impl JournalInsertView {
    pub(crate) fn new(entry: &JournalEntry) -> Self {
        let text = |value: &Option<String>| QueryParam::Text(value.clone().unwrap_or_default());
        Self {
            params: vec![
                QueryParam::Text(entry.kind.clone()),
                QueryParam::Text(enum_text(&entry.storage)),
                QueryParam::Text(entry.location.clone()),
                QueryParam::Text(enum_text(&entry.action)),
                QueryParam::I64(entry.rows.unwrap_or(-1)),
                text(&entry.masked_excerpt),
                text(&entry.cause_hint),
                QueryParam::OptionI32(entry.user_id),
            ],
        }
    }
}

fn enum_text<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

impl ApiRequestDto for JournalInsertView {
    fn query_sql(&self) -> &'static str {
        "INSERT INTO compliance_journal (kind, storage, location, action, rows, masked_excerpt, cause_hint, user_id) \
         VALUES ($1, $2::compliance_storage, $3, $4::compliance_action, NULLIF($5, -1), NULLIF($6, ''), NULLIF($7, ''), $8)"
    }

    fn query_params(&self) -> &[QueryParam] {
        &self.params
    }
}

macro_rules! view {
    ($name:ident, $sql:expr) => {
        #[derive(Debug, Deserialize)]
        struct $name {
            params: Vec<QueryParam>,
        }

        impl ApiRequestDto for $name {
            fn query_sql(&self) -> &'static str {
                $sql
            }

            fn query_params(&self) -> &[QueryParam] {
                &self.params
            }
        }
    };
}

view!(TargetsView, "SELECT row_to_json(t) FROM (SELECT (fn_erasure_targets($1) ->> 'email') AS email, (fn_erasure_targets($1) ->> 'keycloak_subject') AS keycloak_subject) t WHERE t.email IS NOT NULL");
view!(EnsureStepsView, "INSERT INTO erasure_steps (user_id, step) SELECT $1, s FROM unnest(ARRAY['keycloak', 'resend', 'database', 's3', 'redis', 'backup_key']) AS s ON CONFLICT DO NOTHING");
view!(StepsView, "SELECT row_to_json(t) FROM (SELECT step, status, attempts, last_error FROM erasure_steps WHERE user_id = $1) t");
view!(RecordStepView, "UPDATE erasure_steps SET status = $3, attempts = attempts + 1, last_error = NULLIF($4, ''), updated_at = now() WHERE user_id = $1 AND step = $2");
view!(UnfinishedView, "SELECT row_to_json(t) FROM (SELECT DISTINCT user_id AS id FROM erasure_steps WHERE status <> 'done' ORDER BY 1) t");
view!(
    UserExistsView,
    "SELECT EXISTS (SELECT 1 FROM users WHERE id = $1)"
);
view!(AnonymizeView, "SELECT anonymize_user($1) IS NOT NULL");

#[derive(Debug, serde::Serialize, Deserialize)]
struct UserIdRow {
    id: i32,
}

fn status_text(status: StepStatus) -> &'static str {
    match status {
        StepStatus::Pending => "pending",
        StepStatus::Done => "done",
        StepStatus::Failed => "failed",
    }
}

#[async_trait]
impl ComplianceStore for PgStore {
    async fn scan(&self) -> Result<Vec<Finding>, StoreError> {
        self.db
            .fetch_all::<Finding, _>(&ScanQueryView)
            .await
            .map_err(err)
    }

    async fn journal(&self, entry: &JournalEntry) -> Result<(), StoreError> {
        self.db
            .execute(JournalInsertView::new(entry))
            .await
            .map_err(err)
    }

    async fn user_exists(&self, user_id: i32) -> Result<bool, StoreError> {
        self.db
            .fetch_scalar::<bool, _>(&UserExistsView {
                params: vec![QueryParam::I32(user_id)],
            })
            .await
            .map_err(err)
    }

    async fn erasure_targets(&self, user_id: i32) -> Result<Option<ErasureTargets>, StoreError> {
        let rows = self
            .db
            .fetch_all::<TargetsRow, _>(&TargetsView {
                params: vec![QueryParam::I32(user_id)],
            })
            .await
            .map_err(err)?;
        Ok(rows.into_iter().next().map(|r| ErasureTargets {
            email: r.email,
            keycloak_subject: r.keycloak_subject,
        }))
    }

    async fn ensure_steps(&self, user_id: i32) -> Result<(), StoreError> {
        self.db
            .execute(EnsureStepsView {
                params: vec![QueryParam::I32(user_id)],
            })
            .await
            .map_err(err)
    }

    async fn steps(&self, user_id: i32) -> Result<Vec<StepState>, StoreError> {
        self.db
            .fetch_all::<StepState, _>(&StepsView {
                params: vec![QueryParam::I32(user_id)],
            })
            .await
            .map_err(err)
    }

    async fn record_step(
        &self,
        user_id: i32,
        step: Step,
        status: StepStatus,
        error: Option<&str>,
    ) -> Result<(), StoreError> {
        let params = vec![
            QueryParam::I32(user_id),
            QueryParam::Text(step.as_str().to_owned()),
            QueryParam::Text(status_text(status).to_owned()),
            QueryParam::Text(error.unwrap_or_default().chars().take(255).collect()),
        ];
        self.db
            .execute(RecordStepView { params })
            .await
            .map_err(err)
    }

    async fn unfinished_erasures(&self) -> Result<Vec<i32>, StoreError> {
        let rows = self
            .db
            .fetch_all::<UserIdRow, _>(&UnfinishedView { params: vec![] })
            .await
            .map_err(err)?;
        Ok(rows.into_iter().map(|r| r.id).collect())
    }

    async fn anonymize(&self, user_id: i32) -> Result<(), StoreError> {
        self.db
            .fetch_scalar::<bool, _>(&AnonymizeView {
                params: vec![QueryParam::I32(user_id)],
            })
            .await
            .map(|_| ())
            .map_err(err)
    }
}

#[derive(Debug, serde::Serialize, Deserialize)]
struct TargetsRow {
    email: String,
    keycloak_subject: Option<String>,
}

// The lib's errors carry no value (mairie360_api_lib 3.0.1).
#[allow(clippy::needless_pass_by_value)]
fn err(error: mairie360_api_lib::error::ApiLibError) -> StoreError {
    StoreError(error.to_string())
}
