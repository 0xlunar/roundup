use crate::database::{Database, DatabaseError};
use crate::scrapers::{IMDbId, IMDbMediaType};
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

    pub async fn get_minimal_metadata(
        &self,
        id: IMDbId<'_>,
    ) -> Result<IMDbMinimalMetadata, DatabaseError> {
        todo!()
    }
}

pub struct IMDbMinimalMetadata {
    pub title: String,
    pub year: u32,
    pub media_type: IMDbMediaType,
}
