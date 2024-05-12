use std::path::PathBuf;
use actix_files::NamedFile;
use actix_web::{Error, get};

pub mod query;
pub mod download;

#[get("/")]
pub async fn index() -> Result<NamedFile, Error> {
    let path: PathBuf = "./static/index.html".parse().unwrap();
    Ok(NamedFile::open(path)?)
}