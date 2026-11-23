//! Scan of the long-lived Redis keys (MAIR-498): every key expires (MAIR-499), so only the keys
//! without TTL or with a TTL above `REDIS_LONG_TTL_SECONDS` (30 days by default) are reported, by
//! prefix (the part before the first `:`) and count. Never the key itself: it may carry an id or
//! an e-mail.

use crate::store::{Action, ComplianceStore, Finding, JournalEntry, Storage, StoreError};
use async_trait::async_trait;
use std::collections::BTreeMap;

/// Default threshold of a long TTL.
pub const DEFAULT_LONG_TTL_SECONDS: i64 = 30 * 24 * 3600;

// async_trait marks the boxed futures `#[must_use]` again.
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait KeySpace: Send + Sync {
    /// Every key with its TTL in seconds (`-1`: no expiry).
    ///
    /// # Errors
    ///
    /// A short reason, never a key.
    async fn keys_with_ttl(&self) -> Result<Vec<(String, i64)>, String>;
}

/// The instance's Redis (`REDIS_SCAN_URL`: a Redis user allowed to `SCAN` and read TTLs).
pub struct RedisKeySpace {
    url: String,
}

impl RedisKeySpace {
    #[must_use]
    pub fn from_env(var: &dyn Fn(&str) -> Option<String>) -> Option<Self> {
        var("REDIS_SCAN_URL").map(|url| Self { url })
    }
}

#[async_trait]
impl KeySpace for RedisKeySpace {
    async fn keys_with_ttl(&self) -> Result<Vec<(String, i64)>, String> {
        let client =
            redis::Client::open(self.url.as_str()).map_err(|_| "invalid Redis URL".to_owned())?;
        let mut connection = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|_| "Redis unreachable".to_owned())?;
        let mut keys = Vec::new();
        let mut cursor: u64 = 0;
        loop {
            let (next, batch): (u64, Vec<String>) = redis::cmd("SCAN")
                .arg(cursor)
                .arg("COUNT")
                .arg(1000)
                .query_async(&mut connection)
                .await
                .map_err(|_| "Redis SCAN failed".to_owned())?;
            for key in batch {
                let ttl: i64 = redis::cmd("TTL")
                    .arg(&key)
                    .query_async(&mut connection)
                    .await
                    .map_err(|_| "Redis TTL failed".to_owned())?;
                keys.push((key, ttl));
            }
            if next == 0 {
                return Ok(keys);
            }
            cursor = next;
        }
    }
}

fn prefix(key: &str) -> &str {
    key.split(':').next().unwrap_or(key)
}

/// Counts the keys without TTL or with a TTL above `long_ttl`, by prefix, and journals them.
///
/// # Errors
///
/// The key space's reason as a store error, or the journal's error.
pub async fn run_redis_scan(
    store: &dyn ComplianceStore,
    keys: &dyn KeySpace,
    long_ttl: i64,
) -> Result<Vec<Finding>, StoreError> {
    let mut counts: BTreeMap<(&'static str, String), i64> = BTreeMap::new();
    for (key, ttl) in keys.keys_with_ttl().await.map_err(StoreError)? {
        let kind = match ttl {
            -1 => "redis_no_ttl",
            ttl if ttl > long_ttl => "redis_long_ttl",
            _ => continue,
        };
        *counts.entry((kind, prefix(&key).to_owned())).or_default() += 1;
    }
    let mut findings = Vec::new();
    for ((kind, location), rows) in counts {
        let finding = Finding {
            kind: kind.to_owned(),
            location,
            rows,
            detail: if kind == "redis_no_ttl" {
                "keys without expiry".to_owned()
            } else {
                format!("TTL above {long_ttl} s")
            },
        };
        let mut entry = JournalEntry::finding(&finding);
        entry.storage = Storage::Redis;
        entry.action = Action::Detected;
        store.journal(&entry).await?;
        findings.push(finding);
    }
    Ok(findings)
}

#[cfg(test)]
mod tests {
    use super::{run_redis_scan, KeySpace};
    use crate::scan::tests::FakeStore;
    use crate::store::Storage;
    use async_trait::async_trait;

    struct Keys(Vec<(String, i64)>);

    #[async_trait]
    impl KeySpace for Keys {
        async fn keys_with_ttl(&self) -> Result<Vec<(String, i64)>, String> {
            Ok(self.0.clone())
        }
    }

    #[actix_web::test]
    async fn only_keys_without_or_with_a_long_ttl_are_reported_by_prefix() {
        let store = FakeStore::default();
        let keys = Keys(vec![
            ("revoked:9f1c".into(), 3000),
            ("cache:user:jane@example.com".into(), -1),
            ("cache:user:john@example.com".into(), -1),
            ("reset:42".into(), 90 * 24 * 3600),
        ]);
        let findings = run_redis_scan(&store, &keys, 30 * 24 * 3600).await.unwrap();
        let summary: Vec<_> = findings
            .iter()
            .map(|f| (f.kind.as_str(), f.location.as_str(), f.rows))
            .collect();
        assert_eq!(
            summary,
            vec![("redis_long_ttl", "reset", 1), ("redis_no_ttl", "cache", 2)]
        );
        let journal = store.journal.lock().unwrap();
        assert!(journal.iter().all(|e| e.storage == Storage::Redis));
        assert!(
            !format!("{journal:?}").contains("example.com"),
            "never the key itself"
        );
    }
}
