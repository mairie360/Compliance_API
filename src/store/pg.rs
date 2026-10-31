//! `ComplianceStore` on Postgres, as the `compliance_api` role (MAIR-498).

use super::{ComplianceStore, Finding, JournalEntry, StoreError};
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

#[async_trait]
impl ComplianceStore for PgStore {
    async fn scan(&self) -> Result<Vec<Finding>, StoreError> {
        self.db
            .fetch_all::<Finding, _>(&ScanQueryView)
            .await
            .map_err(|e| StoreError(e.to_string()))
    }

    async fn journal(&self, entry: &JournalEntry) -> Result<(), StoreError> {
        self.db
            .execute(JournalInsertView::new(entry))
            .await
            .map_err(|e| StoreError(e.to_string()))
    }
}
