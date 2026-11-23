// Access denials of the JWT gate every route under `/api` sits behind (MAIR-419).
//
// The template has no business endpoint, so these tests mount a probe route under `/api` exactly
// like `main.rs` mounts `endpoints::config`, and check that only a valid token of an existing,
// non-archived account reaches it. A new API keeps this file and adds, next to each of its
// endpoints, the denials specific to that endpoint: non-admin on an admin route, another user
// reading / modifying / deleting a resource, an id in the body that differs from the id in the URL.

#[allow(dead_code)]
mod session_tokens;

use actix_web::{get, http::StatusCode, test, web, App, HttpResponse};
use mairie360_api_lib::security::{AuthenticatedUser, JwtMiddleware};
use mairie360_api_lib::state::AppState;
use mairie360_api_lib::test_setup::queries_setup::{
    get_shared_db, get_shared_db_as, ADMIN_ID, BOB_ID,
};
use serial_test::serial;
use session_tokens::{
    core_claims, core_token, hs256_token, now, other_private_key, set_jwt_env, sign_eddsa_with,
    sign_like_core,
};

/// Nothing listens on port 1: no token checked here carries a session id, so Redis is never read.
const UNREACHABLE_REDIS: &str = "redis://127.0.0.1:1";
/// The Postgres role the API connects with in staging (MAIR-556): the gate runs under its grants.
const API_ROLE: &str = "compliance_api";
/// No account has this id in the seeded test database.
const UNKNOWN_USER_ID: i32 = 999_999;

fn valid_token(user_id: i32) -> String {
    core_token(user_id)
}

/// Stands for any business endpoint: answers the id the gate authenticated.
#[get("/v1/whoami/")]
async fn whoami(user: AuthenticatedUser) -> HttpResponse {
    HttpResponse::Ok().body(user.id.to_string())
}

async fn call(path: &str, bearer: Option<&str>) -> (StatusCode, String) {
    set_jwt_env();
    let pg_url = get_shared_db_as(API_ROLE).await;
    let state = AppState::new(UNREACHABLE_REDIS.to_owned(), pg_url).await;
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(state))
            .service(web::scope("/api").wrap(JwtMiddleware).service(whoami)),
    )
    .await;

    let mut request = test::TestRequest::get().uri(path);
    if let Some(bearer) = bearer {
        request = request.insert_header(("Authorization", format!("Bearer {bearer}")));
    }
    // The gate refuses with an `Err`, which the server turns into its error response.
    let response = match test::try_call_service(&app, request.to_request()).await {
        Ok(response) => response.map_into_boxed_body().into_parts().1,
        Err(error) => error.error_response(),
    };
    let status = response.status();
    let body = actix_web::body::to_bytes(response.into_body())
        .await
        .expect("readable body");
    (status, String::from_utf8_lossy(&body).into_owned())
}

async fn admin_id() -> i32 {
    get_shared_db().await;
    *ADMIN_ID.get().expect("seeded admin")
}

#[actix_web::test]
#[serial]
async fn a_valid_token_reaches_the_endpoint_as_its_user() {
    let admin = admin_id().await;
    let (status, body) = call("/api/v1/whoami/", Some(&valid_token(admin))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, admin.to_string());
}

#[actix_web::test]
#[serial]
async fn a_request_without_token_is_refused() {
    assert_eq!(
        call("/api/v1/whoami/", None).await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[actix_web::test]
#[serial]
async fn a_token_signed_with_another_key_is_refused() {
    // MAIR-680: what a compromised service could sign, claiming Core's `kid`.
    let forged = sign_eddsa_with(
        &core_claims(admin_id().await, now() + 3600),
        &other_private_key(),
    );
    assert_eq!(
        call("/api/v1/whoami/", Some(&forged)).await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[actix_web::test]
#[serial]
async fn an_expired_token_is_refused() {
    let expired = sign_like_core(&core_claims(admin_id().await, now() - 3600));
    assert_eq!(
        call("/api/v1/whoami/", Some(&expired)).await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[actix_web::test]
#[serial]
async fn mair_680_runs_with_core_public_key_only() {
    set_jwt_env();
    assert!(std::env::var_os("JWT_SECRET").is_none());
    assert!(std::env::var_os("JWT_PRIVATE_KEY").is_none());
    let admin = admin_id().await;
    // An EdDSA token signed with Core's key reaches the endpoint as its user.
    let (status, body) = call("/api/v1/whoami/", Some(&core_token(admin))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, admin.to_string());
}

#[actix_web::test]
#[serial]
async fn mair_680_an_hs256_token_is_refused_without_jwt_secret() {
    let admin = admin_id().await;
    // The secrets the stacks and tests used to share, and any other one: no JWT_SECRET here.
    for secret in [
        "auth-gate-test-secret-of-at-least-32-bytes",
        "b\"secret\"",
        "any-secret-of-a-compromised-service-0123456789",
    ] {
        let (status, body) = call("/api/v1/whoami/", Some(&hs256_token(admin, secret))).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{secret}");
        assert_ne!(body, admin.to_string());
    }
}

#[actix_web::test]
#[serial]
async fn mair_680_a_token_without_core_audience_is_refused() {
    let admin = admin_id().await;
    let claims =
        mairie360_api_lib::jwt_manager::Claims::new(&admin.to_string(), "user", now() + 3600);
    assert_eq!(
        call("/api/v1/whoami/", Some(&sign_like_core(&claims)))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
}

#[actix_web::test]
#[serial]
async fn a_malformed_token_is_refused() {
    assert_eq!(
        call("/api/v1/whoami/", Some("not.a.jwt")).await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[actix_web::test]
#[serial]
async fn a_token_of_an_unknown_user_is_refused() {
    // The lib answers 404 (`JWTCheckError::UnknownUser`): the token is well signed, the account
    // it names does not exist (or no longer does).
    assert_eq!(
        call("/api/v1/whoami/", Some(&valid_token(UNKNOWN_USER_ID)))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
}

#[actix_web::test]
#[serial]
async fn a_token_of_an_archived_user_is_refused() {
    get_shared_db().await;
    let bob = *BOB_ID.get().expect("seeded archived user");
    // Same answer as an unknown account: an archived account is treated as gone.
    assert_eq!(
        call("/api/v1/whoami/", Some(&valid_token(bob))).await.0,
        StatusCode::NOT_FOUND
    );
}

#[actix_web::test]
#[serial]
async fn a_percent_encoded_path_does_not_bypass_the_gate() {
    // `%61` is `a`, `%2e%2e` is `..`: the router may resolve these to the protected route, the
    // gate must refuse them all the same.
    for path in [
        "/%61pi/v1/whoami/",
        "/api/v1/%77hoami/",
        "/api/v1/auth/%2e%2e/whoami/",
    ] {
        let (status, _) = call(path, None).await;
        assert_ne!(status, StatusCode::OK, "{path} reached the endpoint");
        assert!(
            status == StatusCode::UNAUTHORIZED || status == StatusCode::NOT_FOUND,
            "{path} answered {status}"
        );
    }
}
