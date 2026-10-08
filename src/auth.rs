//! Service-to-service authentication (MAIR-498).
//!
//! Only other services of the instance call the compliance service (Core_API to request an
//! erasure, the OpenTelemetry Collector for the masking patterns, the platform's CronJob for a
//! scan): no agent ever does. They present an HS256 JWT signed with the instance's `JWT_SECRET`
//! whose `role` is `service` (reversible default, decided until mTLS or a dedicated secret is
//! chosen). Unlike the agents' JWT middleware of `mairie360_api_lib`, no account is looked up:
//! the `sub` names the calling service.

use actix_web::dev::Payload;
use actix_web::{error::ErrorUnauthorized, Error, FromRequest, HttpRequest};
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use std::future::{ready, Ready};

/// Role a service token must carry.
pub const SERVICE_ROLE: &str = "service";

#[derive(Debug, Deserialize)]
struct ServiceClaims {
    sub: String,
    role: String,
}

/// The authenticated calling service (the `sub` of its token).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceCaller(pub String);

/// Checks a bearer token against `secret`: valid signature, not expired, role `service`.
///
/// # Errors
///
/// A short reason, never the token.
pub fn verify(token: &str, secret: &str) -> Result<ServiceCaller, &'static str> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_required_spec_claims(&["exp", "sub"]);
    let data = decode::<ServiceClaims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map_err(|_| "invalid service token")?;
    if data.claims.role != SERVICE_ROLE {
        return Err("not a service token");
    }
    Ok(ServiceCaller(data.claims.sub))
}

impl FromRequest for ServiceCaller {
    type Error = Error;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _: &mut Payload) -> Self::Future {
        let secret = std::env::var("JWT_SECRET").unwrap_or_default();
        let token = req
            .headers()
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));
        ready(match token {
            Some(token) if !secret.is_empty() => verify(token, &secret).map_err(ErrorUnauthorized),
            _ => Err(ErrorUnauthorized("missing service token")),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::verify;
    use jsonwebtoken::{encode, EncodingKey, Header};
    use serde_json::json;

    const SECRET: &str = "compliance-test-secret-of-at-least-32-bytes";

    fn token(role: &str, exp_offset: i64) -> String {
        let exp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .cast_signed()
            + exp_offset;
        encode(
            &Header::default(),
            &json!({ "sub": "core-api", "role": role, "exp": exp }),
            &EncodingKey::from_secret(SECRET.as_bytes()),
        )
        .unwrap()
    }

    #[test]
    fn only_a_valid_service_token_passes() {
        assert_eq!(
            verify(&token("service", 600), SECRET).unwrap().0,
            "core-api"
        );
        assert_eq!(
            verify(&token("admin", 600), SECRET),
            Err("not a service token")
        );
        assert_eq!(
            verify(&token("service", -600), SECRET),
            Err("invalid service token")
        );
        assert_eq!(
            verify(
                &token("service", 600),
                "another-secret-of-at-least-32-bytes!"
            ),
            Err("invalid service token")
        );
    }
}
