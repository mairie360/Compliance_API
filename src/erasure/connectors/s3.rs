//! S3: deletes the objects the instance stores under the user's prefix (`S3_ERASURE_PREFIX`,
//! `users/{user_id}/` by default) in `S3_ERASURE_BUCKET`, on `S3_ENDPOINT` / `S3_REGION` with
//! `S3_ACCESS_KEY_ID` / `S3_SECRET_ACCESS_KEY` (the only credentials of the instance allowed to
//! delete there).

use super::{Connector, ErasureContext, Outcome};
use async_trait::async_trait;
use s3::creds::Credentials;
use s3::{Bucket, Region};

pub struct S3Connector {
    bucket: Option<Box<Bucket>>,
    prefix: String,
}

impl S3Connector {
    pub fn from_env(var: &dyn Fn(&str) -> Option<String>) -> Self {
        let bucket = (|| {
            let region = Region::Custom {
                region: var("S3_REGION").unwrap_or_else(|| "fr-par".to_owned()),
                endpoint: var("S3_ENDPOINT")?,
            };
            let credentials = Credentials::new(
                Some(&var("S3_ACCESS_KEY_ID")?),
                Some(&var("S3_SECRET_ACCESS_KEY")?),
                None,
                None,
                None,
            )
            .ok()?;
            Bucket::new(&var("S3_ERASURE_BUCKET")?, region, credentials)
                .ok()
                .map(|b| b.with_path_style())
        })();
        Self {
            bucket,
            prefix: var("S3_ERASURE_PREFIX").unwrap_or_else(|| "users/{user_id}/".to_owned()),
        }
    }

    /// The prefix of the user's objects.
    #[must_use]
    pub fn prefix_of(&self, user_id: i32) -> String {
        self.prefix.replace("{user_id}", &user_id.to_string())
    }
}

#[async_trait]
impl Connector for S3Connector {
    async fn erase(&self, context: &ErasureContext) -> Result<Outcome, String> {
        let Some(bucket) = &self.bucket else {
            return Ok(Outcome::NotConfigured);
        };
        let prefix = self.prefix_of(context.user_id);
        let pages = bucket
            .list(prefix, None)
            .await
            .map_err(|_| "S3 listing failed".to_owned())?;
        for object in pages.into_iter().flat_map(|page| page.contents) {
            bucket
                .delete_object(&object.key)
                .await
                .map_err(|_| "S3 deletion failed".to_owned())?;
        }
        Ok(Outcome::Erased)
    }
}

#[cfg(test)]
mod tests {
    use super::S3Connector;
    use crate::erasure::connectors::{Connector, ErasureContext, Outcome};

    #[actix_web::test]
    async fn the_prefix_is_per_user_and_an_unconfigured_bucket_is_reported() {
        let connector = S3Connector::from_env(&|_| None);
        assert_eq!(connector.prefix_of(42), "users/42/");
        assert_eq!(
            connector
                .erase(&ErasureContext {
                    user_id: 42,
                    targets: None
                })
                .await,
            Ok(Outcome::NotConfigured)
        );
    }
}
