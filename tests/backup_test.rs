//! Backups sealed with a key per user (MAIR-500), on a throwaway Postgres (testcontainers): a
//! backup holds no personal value of the plan and no key; a restore brings back every user whose
//! key exists; once a user's key is destroyed (erasure), the old backup, untouched, restores them
//! anonymized.

use compliance_api::backup::keys::{InMemoryKeyManager, KeyManager};
use compliance_api::backup::{plan, seal};
use sqlx::{PgPool, Row};
use std::path::Path;
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{GenericImage, ImageExt};

const SCHEMA: &str = r#"
CREATE TABLE users (id SERIAL PRIMARY KEY, first_name TEXT NOT NULL, email TEXT NOT NULL UNIQUE,
                    phone_number TEXT, status TEXT NOT NULL DEFAULT 'active');
CREATE TABLE groups (id SERIAL PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE group_members (group_id INT REFERENCES groups(id), user_id INT REFERENCES users(id),
                            PRIMARY KEY (group_id, user_id));
CREATE TABLE sessions (id SERIAL PRIMARY KEY, user_id INT REFERENCES users(id), ip_address TEXT);
CREATE TABLE connection_logs (id SERIAL PRIMARY KEY, session_id INT REFERENCES sessions(id));
CREATE TABLE messages (id SERIAL PRIMARY KEY, owner_id INT REFERENCES users(id), content TEXT NOT NULL);
CREATE TABLE users_audit_log (audit_id SERIAL PRIMARY KEY, user_id INT, previous_data JSONB, new_data JSONB);
-- A trigger that would write the audit log: sealing must not fire it.
CREATE FUNCTION audit() RETURNS TRIGGER AS $$ BEGIN
  INSERT INTO users_audit_log (user_id, new_data) VALUES (NEW.id, to_jsonb(NEW)); RETURN NEW; END $$ LANGUAGE plpgsql;
CREATE TRIGGER tr_audit AFTER UPDATE ON users FOR EACH ROW EXECUTE FUNCTION audit();
INSERT INTO users (first_name, email, phone_number) VALUES
  ('Alicemarker', 'alice.marker@example.com', '611111111'),
  ('Bobmarker', 'bob.marker@example.com', NULL);
INSERT INTO groups (name) VALUES ('Comptabilité');
INSERT INTO group_members VALUES (1, 1), (1, 2);
INSERT INTO sessions (user_id, ip_address) VALUES (1, '10.0.0.1'), (2, '10.0.0.2');
INSERT INTO connection_logs (session_id) VALUES (1);
INSERT INTO messages (owner_id, content) VALUES (2, 'Réunion jeudi');
INSERT INTO users_audit_log (user_id, new_data) VALUES (2, '{"email": "bob.marker@example.com"}');
"#;

const INVENTORY: &str = r#"version: 1
tables:
  users:
    personal:
      id: {category: identifier, erasure: keep, visibility: directory}
      first_name: {category: identity, erasure: anonymize, visibility: directory}
      email: {category: contact, erasure: anonymize, visibility: directory}
      phone_number: {category: contact, erasure: anonymize, visibility: directory}
      status: {category: account, erasure: keep, visibility: directory}
  group_members:
    personal:
      user_id: {category: identifier, erasure: delete, visibility: members}
    not_personal: [group_id]
  sessions:
    personal:
      user_id: {category: identifier, erasure: delete, visibility: self}
      ip_address: {category: connection, erasure: delete, visibility: self}
    not_personal: [id]
  messages:
    personal:
      owner_id: {category: identifier, erasure: anonymize, visibility: members}
      content: {category: content, erasure: keep, visibility: members}
    not_personal: [id]
  users_audit_log:
    audited: false
    personal:
      user_id: {category: identifier, erasure: keep, visibility: internal}
      previous_data: {category: identity, erasure: keep, visibility: internal}
      new_data: {category: identity, erasure: keep, visibility: internal}
    not_personal: [audit_id]
"#;

const MARKERS: &[&str] = &[
    "Alicemarker",
    "alice.marker",
    "Bobmarker",
    "bob.marker",
    "611111111",
    "10.0.0.1",
    "10.0.0.2",
];

/// Every row of every table, as text: what a dump of the database would hold.
async fn everything(pool: &PgPool) -> String {
    let mut out = String::new();
    for table in [
        "users",
        "groups",
        "group_members",
        "sessions",
        "connection_logs",
        "messages",
        "users_audit_log",
    ] {
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT coalesce(json_agg(t ORDER BY t::text), '[]'::json)::text AS v FROM {table} t"
        )))
        .fetch_one(pool)
        .await
        .unwrap();
        out.push_str(table);
        out.push_str(&row.get::<String, _>("v"));
        out.push('\n');
    }
    out
}

async fn database(base: &str, name: &str, template: Option<&str>) -> PgPool {
    let admin = PgPool::connect(&format!("{base}/postgres")).await.unwrap();
    let from = template
        .map(|t| format!(" TEMPLATE {t}"))
        .unwrap_or_default();
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {name}{from}")))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    PgPool::connect(&format!("{base}/{name}")).await.unwrap()
}

fn backup_bytes(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<_> = std::fs::read_dir(dir.join("users"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .map(|p| (p.display().to_string(), std::fs::read(&p).unwrap()))
        .collect();
    files.push((
        "manifest".to_owned(),
        std::fs::read(dir.join("manifest.json")).unwrap(),
    ));
    files.sort();
    files
}

#[tokio::test]
async fn erased_users_stay_unreadable_in_old_backups() {
    let container = GenericImage::new("postgres", "18-alpine")
        .with_wait_for(WaitFor::message_on_stderr(
            "database system is ready to accept connections",
        ))
        .with_exposed_port(5432.tcp())
        .with_env_var("POSTGRES_PASSWORD", "password")
        .start()
        .await
        .unwrap();
    // The image logs "ready" twice (init then real start): retry the first connection.
    let port = container.get_host_port_ipv4(5432.tcp()).await.unwrap();
    let base = format!("postgres://postgres:password@127.0.0.1:{port}");
    let mut admin = None;
    for _ in 0..30 {
        if let Ok(pool) = PgPool::connect(&format!("{base}/postgres")).await {
            admin = Some(pool);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    admin.unwrap().close().await;

    // The "live" database, then the throwaway copy the backup Job seals.
    let live = database(&base, "live", None).await;
    sqlx::raw_sql(SCHEMA).execute(&live).await.unwrap();
    let original = everything(&live).await;
    live.close().await;
    let copy = database(&base, "copy", Some("live")).await;

    let keys = InMemoryKeyManager::default();
    let dir = tempfile_dir("mair500");
    let plans = plan::plan(INVENTORY, &seal::referenced_tables(&copy).await.unwrap()).unwrap();
    let report = seal::seal(&copy, &plans, &keys, &dir).await.unwrap();
    assert_eq!((report.users, report.sealed_users), (2, 2));

    // The sealed copy (what goes to restic) holds none of the personal values of the plan, the
    // trigger did not fire, the kept content is still there.
    let sealed = everything(&copy).await;
    for marker in MARKERS {
        assert!(
            !sealed.contains(marker),
            "{marker} left in the sealed copy:\n{sealed}"
        );
    }
    assert!(sealed.contains("Réunion jeudi"), "content kept by decision");
    assert!(
        sealed.contains("connection_logs[{\"id\":1,\"session_id\":1}]"),
        "a referenced row is never removed"
    );
    // Neither the blobs nor the manifest hold a value or a key in clear.
    let files = backup_bytes(&dir);
    for (_, bytes) in &files {
        let text = String::from_utf8_lossy(bytes);
        for marker in MARKERS {
            assert!(!text.contains(marker));
        }
        for key in keys.all_keys() {
            assert!(
                !bytes.windows(32).any(|w| w == key),
                "a key is in the backup"
            );
        }
    }
    copy.close().await;

    // A restore of that backup, with every key: everything comes back.
    let restored = database(&base, "restored", Some("copy")).await;
    let back = seal::unseal(&restored, &keys, &dir).await.unwrap();
    assert_eq!((back.restored, back.left_sealed), (2, 0));
    assert_eq!(everything(&restored).await, original);
    restored.close().await;

    // Bob is erased: his key is destroyed, the backup is not touched.
    assert!(keys.destroy(2).await.unwrap());
    assert_eq!(
        backup_bytes(&dir),
        files,
        "an erasure never modifies a backup"
    );

    // A restore of the same old backup brings Alice back, Bob stays anonymized.
    let again = database(&base, "again", Some("copy")).await;
    let back = seal::unseal(&again, &keys, &dir).await.unwrap();
    assert_eq!((back.restored, back.left_sealed), (1, 1));
    let text = everything(&again).await;
    assert!(
        text.contains("Alicemarker")
            && text.contains("alice.marker@example.com")
            && text.contains("10.0.0.1")
    );
    for marker in ["Bobmarker", "bob.marker", "10.0.0.2"] {
        assert!(
            !text.contains(marker),
            "{marker} came back after the erasure"
        );
    }
    assert!(
        text.contains("sealed-2@sealed.invalid"),
        "Bob's row stays, anonymized"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

fn tempfile_dir(prefix: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("{prefix}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
