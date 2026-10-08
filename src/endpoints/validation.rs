//! Input validation shared by every request body and query string of the API.
//!
//! A request view implements [`Validate`] and the handler extracts it with [`ValidatedJson`] or
//! [`ValidatedQuery`] instead of `web::Json` / `web::Query`: an invalid value is rejected with a
//! `400 Bad Request` (plain-text body naming the field) before the handler runs, so it never
//! reaches Postgres (where an over-long value or a NUL byte used to end in a `500`).
//!
//! Free text is stored as typed: `<`, `>` and `&` are legitimate ("budget > 10 000 €", "->").
//! Responses are JSON served with `X-Content-Type-Options: nosniff`, so the browser never renders
//! them as HTML; escaping is the job of the front that displays the value.

use std::fmt;
use std::future::Future;
use std::pin::Pin;

use actix_web::{dev::Payload, web, FromRequest, HttpRequest};
use serde::de::DeserializeOwned;

// Declare the limits of the API here, one constant per Postgres column (e.g.
// `pub const MAX_TITLE_LENGTH: usize = 255;` for a `VARCHAR(255)`), and use them in the views.

/// Why a request value was rejected; its text is the body of the `400` response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError(String);

impl ValidationError {
    pub fn new(field: &str, reason: &str) -> Self {
        Self(format!("Invalid `{field}`: {reason}"))
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Implemented by every request view extracted with [`ValidatedJson`] or [`ValidatedQuery`].
pub trait Validate {
    /// # Errors
    ///
    /// Returns the first field that does not satisfy its constraints.
    fn validate(&self) -> Result<(), ValidationError>;
}

fn check_length(field: &str, value: &str, max: usize) -> Result<(), ValidationError> {
    if value.chars().count() > max {
        return Err(ValidationError::new(
            field,
            &format!("must be at most {max} characters"),
        ));
    }
    Ok(())
}

fn check_no_control(field: &str, value: &str) -> Result<(), ValidationError> {
    if value.chars().any(char::is_control) {
        return Err(ValidationError::new(
            field,
            "must not contain control characters",
        ));
    }
    Ok(())
}

/// A short label (person name, role or group name): not blank, at most `max` characters and no
/// control character.
///
/// # Errors
///
/// Returns a [`ValidationError`] naming `field` when one of the rules is broken.
pub fn check_label(field: &str, value: &str, max: usize) -> Result<(), ValidationError> {
    if value.trim().is_empty() {
        return Err(ValidationError::new(field, "must not be empty"));
    }
    check_length(field, value, max)?;
    check_no_control(field, value)
}

/// A free-text description: may be empty, at most `max` characters, line breaks and tabs
/// allowed, no other control character.
///
/// # Errors
///
/// Returns a [`ValidationError`] naming `field` when one of the rules is broken.
pub fn check_description(field: &str, value: &str, max: usize) -> Result<(), ValidationError> {
    check_length(field, value, max)?;
    if value
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err(ValidationError::new(
            field,
            "must not contain control characters other than line breaks and tabs",
        ));
    }
    Ok(())
}

/// An opaque value only compared or stored as text (token, credential, `device_info`, search
/// filter): at most `max` characters and no control character (Postgres rejects NUL bytes).
///
/// # Errors
///
/// Returns a [`ValidationError`] naming `field` when one of the rules is broken.
pub fn check_opaque(field: &str, value: &str, max: usize) -> Result<(), ValidationError> {
    check_length(field, value, max)?;
    check_no_control(field, value)
}

/// Runs `check` on `value` when it is present.
///
/// # Errors
///
/// Returns the error of `check`.
pub fn check_optional<F>(value: Option<&str>, check: F) -> Result<(), ValidationError>
where
    F: FnOnce(&str) -> Result<(), ValidationError>,
{
    value.map_or(Ok(()), check)
}

fn bad_request(error: &ValidationError) -> actix_web::Error {
    actix_web::error::ErrorBadRequest(error.to_string())
}

/// `web::Json<T>` followed by [`Validate::validate`]: answers `400` when either fails.
pub struct ValidatedJson<T>(pub T);

impl<T> ValidatedJson<T> {
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> FromRequest for ValidatedJson<T>
where
    T: DeserializeOwned + Validate + 'static,
{
    type Error = actix_web::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self, Self::Error>>>>;

    fn from_request(req: &HttpRequest, payload: &mut Payload) -> Self::Future {
        let json = web::Json::<T>::from_request(req, payload);
        Box::pin(async move {
            let value = json.await?.into_inner();
            value.validate().map_err(|e| bad_request(&e))?;
            Ok(Self(value))
        })
    }
}

/// `web::Query<T>` followed by [`Validate::validate`]: answers `400` when either fails.
pub struct ValidatedQuery<T>(pub T);

impl<T> ValidatedQuery<T> {
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> FromRequest for ValidatedQuery<T>
where
    T: DeserializeOwned + Validate + 'static,
{
    type Error = actix_web::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self, Self::Error>>>>;

    fn from_request(req: &HttpRequest, _: &mut Payload) -> Self::Future {
        let query = web::Query::<T>::from_query(req.query_string());
        Box::pin(async move {
            let value = query?.into_inner();
            value.validate().map_err(|e| bad_request(&e))?;
            Ok(Self(value))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{http::StatusCode, test as actix_test, App, HttpResponse};
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Body {
        name: String,
    }

    impl Validate for Body {
        fn validate(&self) -> Result<(), ValidationError> {
            check_label("name", &self.name, 8)
        }
    }

    async fn echo_json(body: ValidatedJson<Body>) -> HttpResponse {
        HttpResponse::Ok().body(body.into_inner().name)
    }

    async fn echo_query(query: ValidatedQuery<Body>) -> HttpResponse {
        HttpResponse::Ok().body(query.into_inner().name)
    }

    async fn call(request: actix_test::TestRequest) -> (StatusCode, String) {
        let app = actix_test::init_service(
            App::new()
                .route("/json", web::post().to(echo_json))
                .route("/query", web::get().to(echo_query)),
        )
        .await;
        let response = actix_test::call_service(&app, request.to_request()).await;
        let status = response.status();
        let body = actix_test::read_body(response).await;
        (status, String::from_utf8(body.to_vec()).unwrap())
    }

    #[actix_web::test]
    async fn validated_json_passes_a_valid_body_through() {
        let request = actix_test::TestRequest::post()
            .uri("/json")
            .set_json(serde_json::json!({ "name": "a > b" }));
        assert_eq!(call(request).await, (StatusCode::OK, "a > b".to_owned()));
    }

    #[actix_web::test]
    async fn validated_json_answers_400_naming_the_field() {
        let request = actix_test::TestRequest::post()
            .uri("/json")
            .set_json(serde_json::json!({ "name": "much too long" }));
        let (status, body) = call(request).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, "Invalid `name`: must be at most 8 characters");
    }

    #[actix_web::test]
    async fn validated_json_answers_400_on_a_malformed_body() {
        let request = actix_test::TestRequest::post()
            .uri("/json")
            .insert_header(("content-type", "application/json"))
            .set_payload("{");
        assert_eq!(call(request).await.0, StatusCode::BAD_REQUEST);
    }

    #[actix_web::test]
    async fn validated_query_passes_a_valid_query_through() {
        let request = actix_test::TestRequest::get().uri("/query?name=a%3Cb");
        assert_eq!(call(request).await, (StatusCode::OK, "a<b".to_owned()));
    }

    #[actix_web::test]
    async fn validated_query_answers_400_on_invalid_or_missing_values() {
        let (status, body) = call(actix_test::TestRequest::get().uri("/query?name=%20")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, "Invalid `name`: must not be empty");
        assert_eq!(
            call(actix_test::TestRequest::get().uri("/query")).await.0,
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn label_rejects_blank_long_and_control() {
        assert!(check_label("name", "Service urbanisme", 64).is_ok());
        assert!(check_label("name", "  ", 64).is_err());
        assert!(check_label("name", &"a".repeat(65), 64).is_err());
        assert!(check_label("name", "Service\0", 64).is_err());
    }

    #[test]
    fn free_text_keeps_angle_brackets() {
        assert!(check_label("name", "Voirie <-> Urbanisme", 64).is_ok());
        assert!(check_description("description", "budget > 10 000 € <3", 1000).is_ok());
    }

    #[test]
    fn label_counts_characters_not_bytes() {
        assert!(check_label("name", &"é".repeat(64), 64).is_ok());
    }

    #[test]
    fn description_allows_line_breaks_only() {
        assert!(check_description("description", "a\nb\tc", 1000).is_ok());
        assert!(check_description("description", "a\0b", 1000).is_err());
        assert!(check_description("description", &"a".repeat(1001), 1000).is_err());
    }

    #[test]
    fn opaque_rejects_nul() {
        assert!(check_opaque("token", "Zm9vYmFy", 512).is_ok());
        assert!(check_opaque("token", "abc\0", 512).is_err());
    }

    #[test]
    fn optional_skips_absent_values() {
        assert!(check_optional(None, |v| check_label("name", v, 64)).is_ok());
        assert!(check_optional(Some(" "), |v| check_label("name", v, 64)).is_err());
    }
}
