use crate::database::{Database, DatabaseError};
use crate::scrapers::IMDbId;
use sqlx::{FromRow, QueryBuilder};

pub struct WatchlistDB<'a> {
    database: &'a Database,
}

impl<'a> WatchlistDB<'a> {
    pub fn new(database: &'a Database) -> Self {
        Self { database }
    }

    pub async fn in_watchlist(&self, id: IMDbId) -> Result<bool, DatabaseError> {
        #[derive(FromRow)]
        struct Row {
            exists: bool,
        }

        QueryBuilder::new("SELECT EXISTS(SELECT 1 FROM watchlist WHERE id = ")
            .push_bind(id)
            .push(")")
            .build_query_as::<Row>()
            .fetch_one(&self.database.pool)
            .await
            .map(|row| row.exists)
            .map_err(|err| DatabaseError::GetError(err.to_string()))
    }

    pub async fn get_item(&self, id: IMDbId) -> Result<Option<IMDbId>, DatabaseError> {
        QueryBuilder::new("SELECT id FROM watchlist WHERE id = ")
            .push_bind(id)
            .push(" LIMIT 1")
            .build_query_as::<WatchlistRow>()
            .fetch_optional(&self.database.pool)
            .await
            .map(|row| row.map(|row| row.id))
            .map_err(|err| DatabaseError::GetError(err.to_string()))
    }

    pub async fn get_items(&self) -> Result<Vec<IMDbId>, DatabaseError> {
        QueryBuilder::new("SELECT id FROM watchlist")
            .build_query_as::<WatchlistRow>()
            .fetch_all(&self.database.pool)
            .await
            .map(|rows| rows.into_iter().map(|row| row.id).collect())
            .map_err(|err| DatabaseError::GetError(err.to_string()))
    }

    pub async fn add_item(&self, id: IMDbId) -> Result<(), DatabaseError> {
        QueryBuilder::new("INSERT INTO watchlist (id) VALUES (")
            .push_bind(id)
            .push(")")
            .build()
            .execute(&self.database.pool)
            .await
            .map(|_| ())
            .map_err(|err| DatabaseError::InsertionError(err.to_string()))
    }

    pub async fn remove_item(&self, id: IMDbId) -> Result<(), DatabaseError> {
        QueryBuilder::new("DELETE FROM watchlist WHERE id = ")
            .push_bind(id)
            .build()
            .execute(&self.database.pool)
            .await
            .map(|_| ())
            .map_err(|err| DatabaseError::DeleteError(err.to_string()))
    }
}

#[derive(FromRow)]
struct WatchlistRow {
    id: IMDbId,
}
