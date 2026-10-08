//! Resend: removes the user's contact from the instance's audience (`RESEND_API_KEY`,
//! `RESEND_AUDIENCE_ID`, `RESEND_API_URL` defaults to `https://api.resend.com`). The e-mails
//! already sent are kept by Resend under its own retention (subprocessor, MAIR-294).

use super::{Connector, ErasureContext, Outcome};
use async_trait::async_trait;
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};

pub struct ResendConnector {
    config: Option<(String, String, String)>,
    http: reqwest::Client,
}

impl ResendConnector {
    pub fn from_env(var: &dyn Fn(&str) -> Option<String>) -> Self {
        let url = var("RESEND_API_URL").unwrap_or_else(|| "https://api.resend.com".to_owned());
        let config = var("RESEND_API_KEY")
            .zip(var("RESEND_AUDIENCE_ID"))
            .map(|(key, audience)| (url.trim_end_matches('/').to_owned(), key, audience));
        Self {
            config,
            http: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl Connector for ResendConnector {
    async fn erase(&self, context: &ErasureContext) -> Result<Outcome, String> {
        let Some((url, key, audience)) = &self.config else {
            return Ok(Outcome::NotConfigured);
        };
        let Some(targets) = &context.targets else {
            return Err(
                "account already anonymized: its Resend contact can no longer be found".to_owned(),
            );
        };
        let email = utf8_percent_encode(&targets.email, NON_ALPHANUMERIC);
        let response = self
            .http
            .delete(format!("{url}/audiences/{audience}/contacts/{email}"))
            .bearer_auth(key)
            .send()
            .await
            .map_err(|_| "Resend API unreachable".to_owned())?;
        match response.status().as_u16() {
            200..=299 | 404 => Ok(Outcome::Erased),
            status => Err(format!("Resend contact deletion answered {status}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ResendConnector;
    use crate::erasure::connectors::{Connector, ErasureContext, Outcome};
    use crate::store::ErasureTargets;
    use wiremock::matchers::{header, method, path_regex};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[actix_web::test]
    async fn removes_the_contact_and_needs_the_targets() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path_regex(
                r"^/audiences/aud-1/contacts/jane(@|%40)example(\.|%2E)com$",
            ))
            .and(header("Authorization", "Bearer re_key"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        let base = server.uri();
        let connector = ResendConnector::from_env(&|name: &str| match name {
            "RESEND_API_URL" => Some(base.clone()),
            "RESEND_API_KEY" => Some("re_key".into()),
            "RESEND_AUDIENCE_ID" => Some("aud-1".into()),
            _ => None,
        });
        let with = ErasureContext {
            user_id: 7,
            targets: Some(ErasureTargets {
                email: "jane@example.com".into(),
                keycloak_subject: None,
            }),
        };
        assert_eq!(connector.erase(&with).await, Ok(Outcome::Erased));
        let without = ErasureContext {
            user_id: 7,
            targets: None,
        };
        assert!(
            connector.erase(&without).await.is_err(),
            "an anonymized account can no longer be found"
        );
    }
}
