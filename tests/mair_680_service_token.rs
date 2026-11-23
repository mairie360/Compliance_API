//! MAIR-680: the service tokens of this API are checked with Core's public key. The routes under
//! `/api` (mounted like `main.rs`, here `GET /api/v1/masking-patterns`, which needs no database)
//! accept an `EdDSA` service token signed with Core's key and refuse, with `401`, an HS256 token
//! once `JWT_SECRET` is unset (whatever secret signed it), a token signed with another key under
//! Core's `kid`, and a well-signed token whose role is not `service`.
//!
//! Own test binary: it sets the process-wide JWT variables.

#[allow(dead_code)]
mod session_tokens;

use actix_web::{http::StatusCode, test, web, App};
use jsonwebtoken::{encode, EncodingKey, Header};
use mairie360_api_lib::jwt_manager::{Claims, SESSION_JWT_AUDIENCE, SESSION_JWT_ISSUER};
use session_tokens::{now, other_private_key, set_jwt_env, sign_eddsa_with, sign_like_core};

const ROUTE: &str = "/api/v1/masking-patterns";

/// Claims of a service token issued by Core for the calling service `sub`.
fn service_claims(role: &str) -> Claims {
    Claims::new("core-api", role, now() + 3600)
        .with_issuer_and_audience(SESSION_JWT_ISSUER, SESSION_JWT_AUDIENCE)
}

async fn status_with(token: Option<String>) -> StatusCode {
    set_jwt_env();
    let app = test::init_service(
        App::new().service(web::scope("/api").configure(compliance_api::endpoints::config)),
    )
    .await;
    let mut request = test::TestRequest::get().uri(ROUTE);
    if let Some(token) = token {
        request = request.insert_header(("Authorization", format!("Bearer {token}")));
    }
    match test::try_call_service(&app, request.to_request()).await {
        Ok(response) => response.status(),
        Err(error) => error.as_response_error().status_code(),
    }
}

#[actix_web::test]
async fn mair_680_eddsa_service_token_signed_with_core_key_is_accepted() {
    let token = sign_like_core(&service_claims("service"));
    assert_eq!(status_with(Some(token)).await, StatusCode::OK);
    assert!(std::env::var_os("JWT_SECRET").is_none());
}

#[actix_web::test]
async fn mair_680_hs256_service_token_is_refused_without_jwt_secret() {
    for secret in [
        "compliance-test-secret-of-at-least-32-bytes",
        "b\"secret\"",
        "any-secret-of-a-compromised-service-0123456789",
    ] {
        let token = encode(
            &Header::default(),
            &Claims::new("core-api", "service", now() + 3600),
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap();
        assert_eq!(
            status_with(Some(token)).await,
            StatusCode::UNAUTHORIZED,
            "{secret}"
        );
    }
}

#[actix_web::test]
async fn mair_680_forged_or_non_service_tokens_are_refused() {
    let forged = sign_eddsa_with(&service_claims("service"), &other_private_key());
    let agent = sign_like_core(&service_claims("admin"));
    let no_audience = sign_like_core(&Claims::new("core-api", "service", now() + 3600));
    for (case, token) in [
        ("another key", forged),
        ("not a service", agent),
        ("no audience", no_audience),
    ] {
        assert_eq!(
            status_with(Some(token)).await,
            StatusCode::UNAUTHORIZED,
            "{case}"
        );
    }
    assert_eq!(status_with(None).await, StatusCode::UNAUTHORIZED);
}
