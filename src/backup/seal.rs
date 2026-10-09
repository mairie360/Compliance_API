//! Sealing and unsealing a backup copy of the database (MAIR-500).
//!
//! `seal` runs on a **throwaway copy** of the database (the backup Job restores the live dump into
//! a Postgres of its own first; the live database is never touched): for each user it moves the
//! values the plan (`plan.rs`) designates into a blob encrypted with a data key of the user
//! (`keys.rs`), then replaces them in the copy by placeholders, so the dump of the copy that goes to
//! restic (encrypted with the instance key) holds no personal value of the plan. The blobs go to
//! restic too, next to the dump: each holds the data key only wrapped by the user's key, which
//! stays in the key manager.
//!
//! `unseal` runs on a database restored from such a dump: it puts back the values of every user
//! whose key still exists; the others (erased: their key was destroyed) stay sealed, i.e. restored
//! anonymized. Nothing in a backup is ever modified: an erasure only destroys a key.
//!
//! Triggers are off on the copy while sealing and unsealing (`session_replication_role = replica`,
//! superuser): sealing is not a change of the users' data, the audit log must not record it.

use super::crypto;
use super::keys::KeyManager;
use super::plan::TablePlan;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

/// One sealed user in the backup: no key in clear, only the data key wrapped by the user's key.
#[derive(Debug, Serialize, Deserialize)]
pub struct SealedUser {
    pub user_id: i32,
    pub key_id: String,
    pub wrapped_key: String,
    /// base64 of `nonce || AES-256-GCM(blob)`, blob = the user's rows and values as JSON.
    pub data: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct SealReport {
    pub users: usize,
    pub sealed_users: usize,
    pub tables: Vec<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct UnsealReport {
    pub restored: usize,
    /// Users whose key no longer exists: restored anonymized.
    pub left_sealed: usize,
}

// Every dynamic statement below is built from identifiers that `ident` accepts (lowercase ASCII,
// digits, underscore: the names of the inventory and of the catalog) and placeholders this module
// writes; every value goes through a bind parameter. Hence the `AssertSqlSafe`.
fn ident(name: &str) -> Result<String, String> {
    if name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && !name.is_empty()
    {
        Ok(format!("\"{name}\""))
    } else {
        Err(format!("unexpected identifier {name}"))
    }
}

fn aad(user_id: i32) -> Vec<u8> {
    format!("mairie360-backup-user-{user_id}").into_bytes()
}

/// The tables other tables reference by a foreign key (never emptied by a plan).
///
/// # Errors
///
/// The query failed.
pub async fn referenced_tables(pool: &PgPool) -> Result<HashSet<String>, String> {
    let rows = sqlx::query(
        "SELECT DISTINCT c.confrelid::regclass::text AS t FROM pg_constraint c
          WHERE c.contype = 'f' AND c.connamespace = 'public'::regnamespace",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("cannot read the foreign keys: {}", db_reason(&e)))?;
    Ok(rows.iter().map(|r| r.get::<String, _>("t")).collect())
}

fn db_reason(error: &sqlx::Error) -> String {
    match error {
        sqlx::Error::Database(e) => format!("SQLSTATE {}", e.code().as_deref().unwrap_or("?")),
        other => other.to_string(),
    }
}

/// A table of the plan with what the database says about it.
struct Resolved {
    plan: TablePlan,
    pk: Vec<String>,
    /// Column → placeholder SQL (`NULL` for a nullable column).
    placeholders: BTreeMap<String, String>,
}

async fn resolve(pool: &PgPool, plans: &[TablePlan]) -> Result<Vec<Resolved>, String> {
    let mut resolved = Vec::new();
    for plan in plans {
        let columns = sqlx::query(
            "SELECT column_name::text AS name, is_nullable = 'YES' AS nullable, data_type::text AS type
               FROM information_schema.columns WHERE table_schema = 'public' AND table_name = $1",
        )
        .bind(&plan.table)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("cannot read {}: {}", plan.table, db_reason(&e)))?;
        if columns.is_empty() {
            // The inventory names a table this schema does not have (other version): nothing to seal.
            continue;
        }
        let pk: Vec<String> = sqlx::query(
            "SELECT a.attname::text AS name FROM pg_index i
               JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = ANY (i.indkey)
              WHERE i.indrelid = $1::regclass AND i.indisprimary ORDER BY a.attnum",
        )
        .bind(format!("public.{}", ident(&plan.table)?))
        .fetch_all(pool)
        .await
        .map_err(|e| format!("cannot read the key of {}: {}", plan.table, db_reason(&e)))?
        .iter()
        .map(|r| r.get("name"))
        .collect();
        let mut placeholders = BTreeMap::new();
        for column in &plan.columns {
            let Some(meta) = columns
                .iter()
                .find(|c| c.get::<String, _>("name") == *column)
            else {
                continue;
            };
            let placeholder = if meta.get::<bool, _>("nullable") {
                "NULL".to_owned()
            } else {
                match meta.get::<String, _>("type").as_str() {
                    "text" | "character varying" | "character" | "citext"
                        if column.contains("email") =>
                    {
                        let first = pk
                            .first()
                            .ok_or_else(|| format!("{} has no primary key", plan.table))?;
                        format!("'sealed-' || {}::text || '@sealed.invalid'", ident(first)?)
                    }
                    "text" | "character varying" | "character" | "citext" => "'sealed'".to_owned(),
                    "jsonb" => "'{}'::jsonb".to_owned(),
                    "json" => "'{}'::json".to_owned(),
                    other => {
                        return Err(format!(
                            "cannot seal {}.{column}: NOT NULL {other}",
                            plan.table
                        ))
                    }
                }
            };
            placeholders.insert(column.clone(), placeholder);
        }
        if !plan.whole_rows && (placeholders.is_empty() || pk.is_empty()) {
            continue;
        }
        resolved.push(Resolved {
            plan: plan.clone(),
            pk,
            placeholders,
        });
    }
    Ok(resolved)
}

/// Seals every user of the copy `pool` into `out_dir/users/<id>.json` and scrubs the copy.
///
/// # Errors
///
/// A database, key manager or file error: the backup must then fail (never ship a half-sealed copy).
pub async fn seal(
    pool: &PgPool,
    plans: &[TablePlan],
    keys: &dyn KeyManager,
    out_dir: &Path,
) -> Result<SealReport, String> {
    let resolved = resolve(pool, plans).await?;
    let users_dir = out_dir.join("users");
    std::fs::create_dir_all(&users_dir)
        .map_err(|e| format!("cannot create {}: {e}", users_dir.display()))?;
    let users: Vec<i32> = sqlx::query("SELECT id FROM users ORDER BY id")
        .fetch_all(pool)
        .await
        .map_err(|e| format!("cannot list the users: {}", db_reason(&e)))?
        .iter()
        .map(|r| r.get("id"))
        .collect();
    let mut report = SealReport {
        users: users.len(),
        tables: resolved.iter().map(|r| r.plan.table.clone()).collect(),
        ..SealReport::default()
    };
    for user_id in users {
        let mut tx = pool.begin().await.map_err(|e| db_reason(&e))?;
        sqlx::query("SET LOCAL session_replication_role = replica")
            .execute(&mut *tx)
            .await
            .map_err(|e| db_reason(&e))?;
        let mut tables = serde_json::Map::new();
        for r in &resolved {
            let table = ident(&r.plan.table)?;
            let user_column = ident(&r.plan.user_column)?;
            let filter = format!("{user_column}::text = $1");
            if r.plan.whole_rows {
                let rows: Value = sqlx::query(sqlx::AssertSqlSafe(format!("SELECT coalesce(json_agg(row_to_json(t)), '[]'::json) AS v FROM {table} t WHERE {filter}")))
                    .bind(user_id.to_string())
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(|e| format!("cannot read {}: {}", r.plan.table, db_reason(&e)))?
                    .get("v");
                if rows.as_array().is_some_and(|a| !a.is_empty()) {
                    sqlx::query(sqlx::AssertSqlSafe(format!(
                        "DELETE FROM {table} WHERE {filter}"
                    )))
                    .bind(user_id.to_string())
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| format!("cannot scrub {}: {}", r.plan.table, db_reason(&e)))?;
                    tables.insert(r.plan.table.clone(), json!({ "rows": rows }));
                }
            } else {
                let pk_json =
                    r.pk.iter()
                        .map(|c| Ok(format!("'{c}', t.{}", ident(c)?)))
                        .collect::<Result<Vec<_>, String>>()?
                        .join(", ");
                let values_json = r
                    .placeholders
                    .keys()
                    .map(|c| Ok(format!("'{c}', t.{}", ident(c)?)))
                    .collect::<Result<Vec<_>, String>>()?
                    .join(", ");
                let entries: Value = sqlx::query(sqlx::AssertSqlSafe(format!(
                    "SELECT coalesce(json_agg(json_build_object('pk', json_build_object({pk_json}), 'values', json_build_object({values_json}))), '[]'::json) AS v FROM {table} t WHERE {filter}"
                )))
                .bind(user_id.to_string())
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| format!("cannot read {}: {}", r.plan.table, db_reason(&e)))?
                .get("v");
                if entries.as_array().is_some_and(|a| !a.is_empty()) {
                    let sets = r
                        .placeholders
                        .iter()
                        .map(|(c, p)| Ok(format!("{} = {p}", ident(c)?)))
                        .collect::<Result<Vec<_>, String>>()?
                        .join(", ");
                    sqlx::query(sqlx::AssertSqlSafe(format!(
                        "UPDATE {table} SET {sets} WHERE {filter}"
                    )))
                    .bind(user_id.to_string())
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| format!("cannot scrub {}: {}", r.plan.table, db_reason(&e)))?;
                    tables.insert(r.plan.table.clone(), json!({ "columns": entries }));
                }
            }
        }
        if !tables.is_empty() {
            let blob = serde_json::to_vec(&json!({ "user_id": user_id, "tables": tables }))
                .map_err(|e| e.to_string())?;
            let key = keys.data_key(user_id).await?;
            let sealed = crypto::seal(&key.plaintext, &blob, &aad(user_id))?;
            let file = SealedUser {
                user_id,
                key_id: key.key_id,
                wrapped_key: key.wrapped,
                data: STANDARD.encode(sealed),
            };
            let path = users_dir.join(format!("{user_id}.json"));
            std::fs::write(&path, serde_json::to_vec(&file).map_err(|e| e.to_string())?)
                .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
            report.sealed_users += 1;
        }
        tx.commit().await.map_err(|e| db_reason(&e))?;
    }
    std::fs::write(
        out_dir.join("manifest.json"),
        serde_json::to_vec_pretty(&json!({ "version": 1, "report": report }))
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("cannot write the manifest: {e}"))?;
    Ok(report)
}

/// Puts back, in the restored database `pool`, the values of every user of `in_dir/users` whose key
/// still exists; the others stay sealed (anonymized).
///
/// # Errors
///
/// A database, key manager or file error.
pub async fn unseal(
    pool: &PgPool,
    keys: &dyn KeyManager,
    in_dir: &Path,
) -> Result<UnsealReport, String> {
    let mut report = UnsealReport::default();
    let users_dir = in_dir.join("users");
    let mut files: Vec<_> = std::fs::read_dir(&users_dir)
        .map_err(|e| format!("cannot read {}: {e}", users_dir.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    for path in files {
        let file: SealedUser =
            serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?)
                .map_err(|_| format!("{} is not a sealed user", path.display()))?;
        let Some(key) = keys.unwrap(&file.key_id, &file.wrapped_key).await? else {
            report.left_sealed += 1;
            continue;
        };
        let sealed = STANDARD
            .decode(&file.data)
            .map_err(|_| "sealed data is not base64".to_owned())?;
        let blob: Value = serde_json::from_slice(&crypto::open(&key, &sealed, &aad(file.user_id))?)
            .map_err(|e| e.to_string())?;
        let mut tx = pool.begin().await.map_err(|e| db_reason(&e))?;
        sqlx::query("SET LOCAL session_replication_role = replica")
            .execute(&mut *tx)
            .await
            .map_err(|e| db_reason(&e))?;
        for (table_name, content) in blob["tables"].as_object().into_iter().flatten() {
            let table = ident(table_name)?;
            for row in content["rows"].as_array().into_iter().flatten() {
                sqlx::query(sqlx::AssertSqlSafe(format!("INSERT INTO {table} SELECT * FROM json_populate_record(NULL::{table}, $1::json)")))
                    .bind(row)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| format!("cannot restore a row of {table_name}: {}", db_reason(&e)))?;
            }
            for entry in content["columns"].as_array().into_iter().flatten() {
                let columns: Vec<String> = entry["values"]
                    .as_object()
                    .into_iter()
                    .flatten()
                    .map(|(c, _)| ident(c))
                    .collect::<Result<_, _>>()?;
                let pk: Vec<String> = entry["pk"]
                    .as_object()
                    .into_iter()
                    .flatten()
                    .map(|(c, _)| Ok(format!("{}::text = ($2::json->>'{c}')", ident(c)?)))
                    .collect::<Result<_, String>>()?;
                if columns.is_empty() || pk.is_empty() {
                    continue;
                }
                let list = columns.join(", ");
                sqlx::query(sqlx::AssertSqlSafe(format!(
                    "UPDATE {table} SET ({list}) = (SELECT {list} FROM json_populate_record(NULL::{table}, $1::json)) WHERE {}",
                    pk.join(" AND ")
                )))
                .bind(&entry["values"])
                .bind(&entry["pk"])
                .execute(&mut *tx)
                .await
                .map_err(|e| format!("cannot restore the values of {table_name}: {}", db_reason(&e)))?;
            }
        }
        tx.commit().await.map_err(|e| db_reason(&e))?;
        report.restored += 1;
    }
    Ok(report)
}
