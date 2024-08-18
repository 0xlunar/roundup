use log::info;
use sqlx::Executor;

use super::DBConnection;

pub struct DatabaseInitialiser<'a> {
    db: &'a DBConnection,
}

impl<'a> DatabaseInitialiser<'a> {
    pub fn new(db: &'a DBConnection) -> Self {
        Self { db }
    }
    pub async fn initialise(&self) -> anyhow::Result<()> {
        info!("Initialising Database");
        let mut tx = self.db.db.begin().await?;

        let item_type_sql = include_str!("sql/item_type.sql");
        let imdb_sql = include_str!("sql/imdb.sql");
        let moviedb_sql = include_str!("sql/moviedb.sql");
        let active_downloads_sql = include_str!("sql/downloads.sql");
        let user_type_sql = include_str!("sql/user_type.sql");
        let user_sql = include_str!("sql/users.sql");
        let oauth_sql = include_str!("sql/oauth.sql");

        // Doesn't return anything useful on success or error so can ignore, if it fails the app just won't work
        tx.execute(item_type_sql).await?;
        tx.execute(imdb_sql).await?;
        tx.execute(moviedb_sql).await?;
        tx.execute(active_downloads_sql).await?;
        tx.execute(user_type_sql).await?;
        tx.execute(user_sql).await?;
        tx.execute(oauth_sql).await?;

        tx.commit().await?;
        Ok(())
    }
}
