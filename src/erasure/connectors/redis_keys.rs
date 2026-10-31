//! Redis: deletes the keys of the user, matched by the patterns of `REDIS_ERASURE_PATTERNS`
//! (comma-separated, `{user_id}` replaced, e.g. `session:{user_id}:*,user:{user_id}:*`) on
//! `REDIS_ERASURE_URL` (the only Redis user of the instance allowed to delete everywhere). Every
//! key expires anyway (MAIR-499): this removes the long-lived ones at once.

use super::{Connector, ErasureContext, Outcome};
use async_trait::async_trait;

pub struct RedisConnector {
    url: Option<String>,
    patterns: Vec<String>,
}

impl RedisConnector {
    pub fn from_env(var: &dyn Fn(&str) -> Option<String>) -> Self {
        let patterns = var("REDIS_ERASURE_PATTERNS")
            .map(|p| {
                p.split(',')
                    .map(str::trim)
                    .filter(|p| !p.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        Self {
            url: var("REDIS_ERASURE_URL"),
            patterns,
        }
    }

    /// The patterns of the user's keys.
    #[must_use]
    pub fn patterns_of(&self, user_id: i32) -> Vec<String> {
        self.patterns
            .iter()
            .map(|p| p.replace("{user_id}", &user_id.to_string()))
            .collect()
    }
}

#[async_trait]
impl Connector for RedisConnector {
    async fn erase(&self, context: &ErasureContext) -> Result<Outcome, String> {
        let Some(url) = &self.url else {
            return Ok(Outcome::NotConfigured);
        };
        if self.patterns.is_empty() {
            return Ok(Outcome::NotConfigured);
        }
        let client =
            redis::Client::open(url.as_str()).map_err(|_| "invalid Redis URL".to_owned())?;
        let mut connection = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|_| "Redis unreachable".to_owned())?;
        for pattern in self.patterns_of(context.user_id) {
            let mut cursor: u64 = 0;
            loop {
                let (next, keys): (u64, Vec<String>) = redis::cmd("SCAN")
                    .arg(cursor)
                    .arg("MATCH")
                    .arg(&pattern)
                    .arg("COUNT")
                    .arg(500)
                    .query_async(&mut connection)
                    .await
                    .map_err(|_| "Redis SCAN failed".to_owned())?;
                if !keys.is_empty() {
                    redis::cmd("DEL")
                        .arg(&keys)
                        .query_async::<i64>(&mut connection)
                        .await
                        .map_err(|_| "Redis DEL failed".to_owned())?;
                }
                if next == 0 {
                    break;
                }
                cursor = next;
            }
        }
        Ok(Outcome::Erased)
    }
}

#[cfg(test)]
mod tests {
    use super::RedisConnector;
    use crate::erasure::connectors::{Connector, ErasureContext, Outcome};

    #[actix_web::test]
    async fn the_patterns_are_per_user_and_unconfigured_redis_is_reported() {
        let connector = RedisConnector::from_env(&|name: &str| {
            (name == "REDIS_ERASURE_PATTERNS")
                .then(|| "session:{user_id}:*, user:{user_id}:*".to_owned())
        });
        assert_eq!(connector.patterns_of(42), vec!["session:42:*", "user:42:*"]);
        assert_eq!(
            connector
                .erase(&ErasureContext {
                    user_id: 42,
                    targets: None
                })
                .await,
            Ok(Outcome::NotConfigured),
            "no URL"
        );
    }
}
