use compliance_api::endpoints::swagger::ApiDoc;
use utoipa::OpenApi;

fn main() {
    match ApiDoc::openapi().to_json() {
        Ok(json) => println!("{}", json),
        Err(err) => eprintln!("Erreur lors de la génération du JSON : {}", err),
    }
}
