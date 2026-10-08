use api_template::endpoints::swagger::ApiDoc; // change api name
use utoipa::OpenApi;

fn main() {
    match ApiDoc::openapi().to_json() {
        Ok(json) => println!("{}", json),
        Err(err) => eprintln!("Erreur lors de la génération du JSON : {}", err),
    }
}
