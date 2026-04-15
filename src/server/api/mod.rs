mod download;
mod modal;
mod search;
mod torrents;

use actix_files::NamedFile;
use actix_web::{get, Error};
use std::path::PathBuf;

pub use download::*;
pub use search::*;

#[get("/")]
pub async fn index() -> Result<NamedFile, Error> {
    let path: PathBuf = "./static/index.html".parse()?;
    Ok(NamedFile::open(path)?)
}
