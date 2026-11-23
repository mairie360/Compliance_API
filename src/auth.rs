//! Service-to-service authentication (MAIR-498, MAIR-680).
//!
//! Only other services of the instance call the compliance service (Core_API to request an
//! erasure, the OpenTelemetry Collector for the masking patterns, the platform's CronJob for a
//! scan): no agent ever does. They present a JWT whose `role` is `service` (reversible default,
//! decided until mTLS or a dedicated credential is chosen). Unlike the agents' JWT middleware of
//! `mairie360_api_lib`, no account is looked up: the `sub` names the calling service.
//!
//! The token is checked by the lib's `decode_jwt` (MAIR-680): an `EdDSA` token signed with Core's
//! Ed25519 key (`JWT_PUBLIC_KEY`, matching `kid`, `iss` `mairie360-core`, `aud` `mairie360`) is
//! accepted; an HS256 token is only accepted while `JWT_SECRET` is still set (transition) and is
//! refused once it is removed, so a service that leaks its configuration can no longer forge one.

use actix_web::dev::Payload;
use actix_web::{error::ErrorUnauthorized, Error, FromRequest, HttpRequest};
use mairie360_api_lib::jwt_manager::decode_jwt;
use std::future::{ready, Ready};

/// Role a service token must carry.
pub const SERVICE_ROLE: &str = "service";

/// The authenticated calling service (the `sub` of its token).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceCaller(pub String);

/// Checks a bearer token with the session keys of the environment (`JWT_PUBLIC_KEY`, and
/// `JWT_SECRET` during the transition): valid signature, not expired, role `service`.
///
/// # Errors
///
/// A short reason, never the token.
pub fn verify(token: &str) -> Result<ServiceCaller, &'static str> {
    let claims = decode_jwt(token).map_err(|_| "invalid service token")?;
    if claims.role() != SERVICE_ROLE {
        return Err("not a service token");
    }
    Ok(ServiceCaller(claims.user_id().to_owned()))
}

impl FromRequest for ServiceCaller {
    type Error = Error;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _: &mut Payload) -> Self::Future {
        let token = req
            .headers()
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));
        ready(match token {
            Some(token) => verify(token).map_err(ErrorUnauthorized),
            None => Err(ErrorUnauthorized("missing service token")),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::verify;
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
    use mairie360_api_lib::jwt_manager::{
        parse_public_keys, parse_signing_key, SESSION_JWT_AUDIENCE, SESSION_JWT_ISSUER,
    };
    use mairie360_api_lib::test_setup::http_setup::{test_jwt_private_key, TEST_JWT_PUBLIC_KEY};
    use serde_json::{json, Value};

    /// The environment of a deployed service after MAIR-680: Core's public key, no `JWT_SECRET`.
    fn set_jwt_env() {
        static ENV: std::sync::Once = std::sync::Once::new();
        ENV.call_once(|| {
            std::env::set_var("JWT_PUBLIC_KEY", TEST_JWT_PUBLIC_KEY);
            std::env::remove_var("JWT_PRIVATE_KEY");
            std::env::remove_var("JWT_SECRET");
        });
    }

    fn exp(offset: i64) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .cast_signed()
            + offset
    }

    /// A token signed like Core signs it (EdDSA, the lib's test key pair, Core's `kid`).
    fn core_token(claims: &Value) -> String {
        set_jwt_env();
        let keys = parse_public_keys(TEST_JWT_PUBLIC_KEY).unwrap();
        let key = parse_signing_key(&test_jwt_private_key(), &keys).unwrap();
        let mut header = Header::new(Algorithm::EdDSA);
        header.kid = Some(key.kid().to_owned());
        encode(&header, claims, key.encoding_key()).unwrap()
    }

    fn service_claims(role: &str, exp_offset: i64) -> Value {
        json!({
            "sub": "core-api",
            "role": role,
            "exp": exp(exp_offset),
            "iss": SESSION_JWT_ISSUER,
            "aud": SESSION_JWT_AUDIENCE,
        })
    }

    #[test]
    fn only_a_valid_service_token_passes() {
        assert_eq!(
            verify(&core_token(&service_claims("service", 600)))
                .unwrap()
                .0,
            "core-api"
        );
        assert_eq!(
            verify(&core_token(&service_claims("admin", 600))),
            Err("not a service token")
        );
        assert_eq!(
            verify(&core_token(&service_claims("service", -600))),
            Err("invalid service token")
        );
        // Without Core's audience.
        assert_eq!(
            verify(&core_token(
                &json!({ "sub": "core-api", "role": "service", "exp": exp(600) })
            )),
            Err("invalid service token")
        );
    }

    #[test]
    fn mair_680_hs256_service_token_is_refused_without_jwt_secret() {
        set_jwt_env();
        let hs256 = encode(
            &Header::default(),
            &json!({ "sub": "core-api", "role": "service", "exp": exp(600) }),
            &EncodingKey::from_secret(b"compliance-test-secret-of-at-least-32-bytes"),
        )
        .unwrap();
        assert_eq!(verify(&hs256), Err("invalid service token"));
    }
}
