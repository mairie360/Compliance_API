use actix_web::{http::Method, test, web, App, HttpResponse};
use api_template::endpoints::swagger::ApiDoc;
use api_template::endpoints::{config, health};
use utoipa::OpenApi;

// Every operation published in the OpenAPI contract (the one the `@mairie360/<name>-api-openapi` package is
// generated from) must hit an actix route that is really mounted, the way `main.rs` mounts them. No database
// nor JWT is needed: a missing route falls through to the default service (418), a routed one fails further on
// (data, JWT, body). It catches a `#[utoipa::path]` whose path differs from the actix one, e.g. a missing
// trailing slash (`path = ""` instead of `path = "/"` under a nest without one).
#[actix_web::test]
async fn every_published_operation_is_routed() {
    let app = test::init_service(
        App::new()
            .service(health::health)
            .service(health::ready)
            .service(web::scope("/api").configure(config))
            .default_service(web::to(HttpResponse::ImATeapot)),
    )
    .await;

    let document = serde_json::to_value(ApiDoc::openapi()).expect("serializable OpenAPI contract");
    let paths = document["paths"].as_object().expect("paths");
    let mut checked = 0;
    let mut unrouted = Vec::new();

    for (template, operations) in paths {
        if template.contains("//") {
            unrouted.push(format!("empty segment in {template}"));
        }
        let uri = template
            .split('/')
            .map(|segment| {
                if segment.starts_with('{') {
                    "1"
                } else {
                    segment
                }
            })
            .collect::<Vec<_>>()
            .join("/");

        for method in operations.as_object().expect("operations").keys() {
            let method = Method::from_bytes(method.to_uppercase().as_bytes()).expect("HTTP method");
            let request = test::TestRequest::default()
                .method(method.clone())
                .uri(&uri)
                .to_request();
            let response = test::call_service(&app, request).await;
            checked += 1;
            if response.status().as_u16() == 418 {
                unrouted.push(format!("{method} {template}"));
            }
        }
    }

    assert!(checked > 0, "no operation found in the contract");
    assert!(
        unrouted.is_empty(),
        "published operations without an actix route: {unrouted:?}"
    );
}
