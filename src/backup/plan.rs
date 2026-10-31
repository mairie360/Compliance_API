//! What a backup seals (MAIR-500), derived from the personal data inventory (Devops/Database
//! `gdpr/inventory.yaml`, the same file as the erasure of MAIR-289):
//!
//! - a table whose user column is erased by deletion (`erasure: delete`, e.g. `group_members`): the
//!   user's rows move whole into the user's sealed blob and leave the backup;
//! - otherwise, the user's values of the personal columns the erasure clears (`anonymize` or
//!   `delete`, identifiers excluded: ids keep the references working) move into the blob and are
//!   replaced by a placeholder;
//! - `users_audit_log.previous_data` / `new_data` too: kept by decision, but MAIR-289 hashes the
//!   identity they hold at erasure, so a backup must not keep it readable.
//!
//! A table referenced by a foreign key is never emptied (a restore would fail on the constraint):
//! only its columns are sealed. The user of a row is the table's identifier column (`user_id` when
//! present, else the first one); rows reached only through another identifier are not sealed by
//! this table and keep the instance key only (listed in the module documentation of `seal`).

use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};

/// Columns kept by the inventory that a backup still seals (see the module documentation).
pub const ALSO_SEALED: &[(&str, &str)] = &[
    ("users_audit_log", "previous_data"),
    ("users_audit_log", "new_data"),
];

#[derive(Debug, Deserialize)]
struct Inventory {
    version: u32,
    tables: BTreeMap<String, TableSpec>,
}

#[derive(Debug, Deserialize)]
struct TableSpec {
    #[serde(default)]
    personal: BTreeMap<String, ColumnSpec>,
}

#[derive(Debug, Deserialize)]
struct ColumnSpec {
    category: String,
    erasure: String,
}

/// How one table is sealed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TablePlan {
    pub table: String,
    /// The column that names the user (`id` for `users`).
    pub user_column: String,
    /// The user's rows move whole into the blob.
    pub whole_rows: bool,
    /// Otherwise, the columns moved into the blob and replaced by a placeholder.
    pub columns: Vec<String>,
}

/// The plan of the inventory. `referenced`: the tables other tables reference by a foreign key
/// (never emptied).
///
/// # Errors
///
/// The inventory is not a version 1 inventory.
pub fn plan(inventory: &str, referenced: &HashSet<String>) -> Result<Vec<TablePlan>, String> {
    let inventory: Inventory =
        yaml_serde::from_str(inventory).map_err(|e| format!("invalid inventory: {e}"))?;
    if inventory.version != 1 {
        return Err("the inventory must be version 1".to_owned());
    }
    let mut plans = Vec::new();
    for (table, spec) in &inventory.tables {
        let erased = |c: &ColumnSpec| c.erasure == "anonymize" || c.erasure == "delete";
        let user_column = if table == "users" {
            Some("id".to_owned())
        } else if spec
            .personal
            .get("user_id")
            .is_some_and(|c| c.category == "identifier")
        {
            Some("user_id".to_owned())
        } else {
            spec.personal
                .iter()
                .find(|(_, c)| c.category == "identifier")
                .map(|(n, _)| n.clone())
        };
        let Some(user_column) = user_column else {
            continue;
        };
        let whole_rows = table != "users"
            && spec
                .personal
                .get(&user_column)
                .is_some_and(|c| c.erasure == "delete")
            && !referenced.contains(table);
        let mut columns: Vec<String> = spec
            .personal
            .iter()
            .filter(|(_, c)| c.category != "identifier" && erased(c))
            .map(|(n, _)| n.clone())
            .collect();
        for (t, c) in ALSO_SEALED {
            if t == table && spec.personal.contains_key(*c) && !columns.iter().any(|x| x == c) {
                columns.push((*c).to_owned());
            }
        }
        columns.sort();
        if whole_rows || !columns.is_empty() {
            plans.push(TablePlan {
                table: table.clone(),
                user_column,
                whole_rows,
                columns: if whole_rows { Vec::new() } else { columns },
            });
        }
    }
    Ok(plans)
}

#[cfg(test)]
mod tests {
    use super::{plan, TablePlan};
    use std::collections::HashSet;

    const INVENTORY: &str = r#"version: 1
tables:
  users:
    personal:
      id: {category: identifier, erasure: keep, visibility: directory}
      email: {category: contact, erasure: anonymize, visibility: directory}
      status: {category: account, erasure: keep, visibility: directory}
  group_members:
    personal:
      user_id: {category: identifier, erasure: delete, visibility: members}
    not_personal: [group_id]
  sessions:
    personal:
      user_id: {category: identifier, erasure: delete, visibility: self}
      ip_address: {category: connection, erasure: delete, visibility: self}
  messages:
    personal:
      owner_id: {category: identifier, erasure: anonymize, visibility: members}
      content: {category: content, erasure: keep, visibility: members}
  users_audit_log:
    personal:
      user_id: {category: identifier, erasure: keep, visibility: internal}
      previous_data: {category: identity, erasure: keep, visibility: internal}
"#;

    #[test]
    fn the_plan_follows_the_erasure_of_the_inventory() {
        let referenced: HashSet<String> = ["sessions".to_owned()].into();
        let plans = plan(INVENTORY, &referenced).unwrap();
        let by = |t: &str| plans.iter().find(|p| p.table == t).cloned();
        assert_eq!(
            by("users"),
            Some(TablePlan {
                table: "users".into(),
                user_column: "id".into(),
                whole_rows: false,
                columns: vec!["email".into()]
            })
        );
        assert!(by("group_members").unwrap().whole_rows);
        let sessions = by("sessions").unwrap();
        assert!(!sessions.whole_rows, "a referenced table is never emptied");
        assert_eq!(sessions.columns, vec!["ip_address".to_owned()]);
        assert_eq!(
            by("messages"),
            None,
            "content is kept by decision, owner_id is an id"
        );
        assert_eq!(
            by("users_audit_log").unwrap().columns,
            vec!["previous_data".to_owned()]
        );
        assert!(plan("version: 2\ntables: {}\n", &referenced).is_err());
    }
}
