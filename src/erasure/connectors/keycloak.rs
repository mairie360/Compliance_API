//! Keycloak: deletes the user's account in the realm through the Admin REST API, with the service
//! account of a confidential client (`client_credentials`). The account is found by its subject
//! (`user_identities`), else by its e-mail.
//!
//! `KEYCLOAK_ADMIN_URL` (`https://auth.<domain>/admin/realms/<realm>`), `KEYCLOAK_TOKEN_URL`
//! (`https://auth.<domain>/realms/<realm>/protocol/openid-connect/token`),
//! `KEYCLOAK_ADMIN_CLIENT_ID`, `KEYCLOAK_ADMIN_CLIENT_SECRET` (realm role `manage-users`).

use super::{Connector, ErasureContext, Outcome};
use async_trait::async_trait;
use serde::Deserialize;

pub struct KeycloakConnector {
    config: Option<Config>,
    http: reqwest::Client,
}

struct Config {
    admin_url: String,
    token_url: String,
    client_id: String,
    client_secret: String,
}

#[derive(Deserialize)]
struct Token {
    access_token: String,
}

#[derive(Deserialize)]
struct UserRef {
    id: String,
}

impl KeycloakConnector {
    pub fn from_env(var: &dyn Fn(&str) -> Option<String>) -> Self {
        let config = (|| {
            Some(Config {
                admin_url: var("KEYCLOAK_ADMIN_URL")?.trim_end_matches('/').to_owned(),
                token_url: var("KEYCLOAK_TOKEN_URL")?,
                client_id: var("KEYCLOAK_ADMIN_CLIENT_ID")?,
                client_secret: var("KEYCLOAK_ADMIN_CLIENT_SECRET")?,
            })
        })();
        Self {
            config,
            http: reqwest::Client::new(),
        }
    }

    async fn token(&self, config: &Config) -> Result<String, String> {
        let response = self
            .http
            .post(&config.token_url)
            .form(&[
                ("grant_type", "client_credentials"),
                ("client_id", &config.client_id),
                ("client_secret", &config.client_secret),
            ])
            .send()
            .await
            .map_err(|_| "Keycloak token endpoint unreachable".to_owned())?;
        if !response.status().is_success() {
            return Err(format!(
                "Keycloak token endpoint answered {}",
                response.status().as_u16()
            ));
        }
        response
            .json::<Token>()
            .await
            .map(|t| t.access_token)
            .map_err(|_| "unreadable Keycloak token".to_owned())
    }
}

#[async_trait]
impl Connector for KeycloakConnector {
    async fn erase(&self, context: &ErasureContext) -> Result<Outcome, String> {
        let Some(config) = &self.config else {
            return Ok(Outcome::NotConfigured);
        };
        let Some(targets) = &context.targets else {
            return Err(
                "account already anonymized: its Keycloak account can no longer be found"
                    .to_owned(),
            );
        };
        let token = self.token(config).await?;
        let id = if let Some(subject) = &targets.keycloak_subject {
            subject.clone()
        } else {
            let response = self
                .http
                .get(format!("{}/users", config.admin_url))
                .query(&[("email", targets.email.as_str()), ("exact", "true")])
                .bearer_auth(&token)
                .send()
                .await
                .map_err(|_| "Keycloak Admin API unreachable".to_owned())?;
            if !response.status().is_success() {
                return Err(format!(
                    "Keycloak user search answered {}",
                    response.status().as_u16()
                ));
            }
            let users: Vec<UserRef> = response
                .json()
                .await
                .map_err(|_| "unreadable Keycloak user search".to_owned())?;
            match users.into_iter().next() {
                Some(user) => user.id,
                None => return Ok(Outcome::Erased),
            }
        };
        let response = self
            .http
            .delete(format!("{}/users/{}", config.admin_url, id))
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|_| "Keycloak Admin API unreachable".to_owned())?;
        match response.status().as_u16() {
            200..=299 | 404 => Ok(Outcome::Erased),
            status => Err(format!("Keycloak user deletion answered {status}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::KeycloakConnector;
    use crate::erasure::connectors::{Connector, ErasureContext, Outcome};
    use crate::store::ErasureTargets;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn connector(server: &MockServer) -> KeycloakConnector {
        let base = server.uri();
        KeycloakConnector::from_env(&|name: &str| match name {
            "KEYCLOAK_ADMIN_URL" => Some(format!("{base}/admin/realms/m360")),
            "KEYCLOAK_TOKEN_URL" => Some(format!("{base}/token")),
            "KEYCLOAK_ADMIN_CLIENT_ID" => Some("compliance".into()),
            "KEYCLOAK_ADMIN_CLIENT_SECRET" => Some("client-secret".into()),
            _ => None,
        })
    }

    fn context(subject: Option<&str>) -> ErasureContext {
        ErasureContext {
            user_id: 7,
            targets: Some(ErasureTargets {
                email: "jane@example.com".into(),
                keycloak_subject: subject.map(str::to_owned),
            }),
        }
    }

    #[actix_web::test]
    async fn deletes_the_account_found_by_subject_or_email_and_accepts_a_missing_one() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "access_token": "t" })),
            )
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/admin/realms/m360/users/kc-1"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/admin/realms/m360/users"))
            .and(query_param("email", "jane@example.com"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!([{ "id": "kc-2" }])),
            )
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/admin/realms/m360/users/kc-2"))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&server)
            .await;
        let connector = connector(&server);
        assert_eq!(
            connector.erase(&context(Some("kc-1"))).await,
            Ok(Outcome::Erased)
        );
        assert_eq!(
            connector.erase(&context(None)).await,
            Ok(Outcome::Erased),
            "already gone counts as erased"
        );
    }

    #[actix_web::test]
    async fn a_refused_deletion_is_a_failure_without_value() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "access_token": "t" })),
            )
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        let error = connector(&server)
            .erase(&context(Some("kc-1")))
            .await
            .unwrap_err();
        assert_eq!(error, "Keycloak user deletion answered 403");
        assert_eq!(
            KeycloakConnector::from_env(&|_| None)
                .erase(&context(None))
                .await,
            Ok(Outcome::NotConfigured)
        );
    }
}
