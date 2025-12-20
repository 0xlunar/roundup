use crate::database::{Database, DatabaseError};
use serde::Serialize;

pub struct IMDbDB<'a> {
    database: &'a Database,
}

impl<'a> IMDbDB<'a> {
    pub fn new(database: &'a Database) -> Self {
        Self { database }
    }

    pub async fn insert(&self, item: impl Serialize) -> Result<(), DatabaseError> {
        // let result = sqlx::query!();

        Ok(())
    }
}
