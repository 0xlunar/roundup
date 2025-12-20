use actix_web::http::StatusCode;
use actix_web::{Error, HttpResponse, post};

pub struct DownloadQueryParams {}

#[post("/download")]
pub async fn download() -> Result<HttpResponse<String>, Error> {
    Ok(HttpResponse::with_body(
        StatusCode::NOT_IMPLEMENTED,
        "Download not implemented yet!".to_string(),
    ))
}
