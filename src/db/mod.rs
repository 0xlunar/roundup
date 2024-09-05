use sqlx::postgres::{PgPool, PgPoolOptions};

pub mod downloads;
pub mod imdb;
pub mod initialiser;

#[derive(Clone)]
pub struct DBConnection {
    db: PgPool,
}

impl DBConnection {
    pub async fn new(connection_uri: &str) -> Result<DBConnection, sqlx::Error> {
        let pool = PgPoolOptions::new().connect(connection_uri).await?;
        Ok(DBConnection { db: pool })
    }

    pub async fn from_env(key: &str) -> anyhow::Result<DBConnection> {
        let connection_uri = std::env::var(key)?;
        let pool = PgPoolOptions::new().connect(&connection_uri).await?;
        Ok(DBConnection { db: pool })
    }
}
