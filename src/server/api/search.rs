use actix_web::http::StatusCode;
use actix_web::{Error, HttpResponse, post};

#[post("/search")]
pub async fn search() -> Result<HttpResponse<String>, Error> {
    Ok(HttpResponse::with_body(
        StatusCode::NOT_IMPLEMENTED,
        "Search not implemented yet!".to_string(),
    ))
}
