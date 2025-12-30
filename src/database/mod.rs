pub mod imdb;
pub mod torrent;

pub use self::torrent::TorrentDB;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use std::fmt::{Display, Formatter};

#[derive(Clone)]
pub struct Database {
    pool: PgPool,
}

impl Database {
    pub async fn new(url: &str) -> Result<Self, DatabaseError> {
        let pool = match PgPoolOptions::new().connect(url).await {
            Ok(pool) => pool,
            Err(err) => return Err(DatabaseError::SQLxError(err)),
        };
        Ok(Self { pool })
    }

    pub async fn initialise(&self) -> Result<(), DatabaseError> {
        Ok(())
    }
}

#[derive(Debug)]
pub enum DatabaseError {
    InsertionError(String),
    UpdateError(String),
    GetError(String),
    DeleteError(String),
    SQLxError(sqlx::Error),
    AnyhowError(anyhow::Error),
}

impl Display for DatabaseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            DatabaseError::InsertionError(err) => {
                f.write_fmt(format_args!("InsertionError: {err}"))
            }
            DatabaseError::UpdateError(err) => f.write_fmt(format_args!("UpdateError: {err}")),
            DatabaseError::GetError(err) => f.write_fmt(format_args!("GetError: {err}")),
            DatabaseError::DeleteError(err) => f.write_fmt(format_args!("DeleteError: {err}")),
            DatabaseError::SQLxError(err) => f.write_fmt(format_args!("SQLx Error: {err}")),
            DatabaseError::AnyhowError(err) => f.write_fmt(format_args!("Anyhow Error: {err}")),
        }
    }
}

impl core::error::Error for DatabaseError {}
