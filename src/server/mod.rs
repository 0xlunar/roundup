use actix_files::NamedFile;
use actix_web::{get, Error};
use std::path::PathBuf;

pub mod download;
pub mod query;

#[get("/")]
pub async fn index() -> Result<NamedFile, Error> {
    let path: PathBuf = "./static/index.html".parse().unwrap();
    Ok(NamedFile::open(path)?)
}
