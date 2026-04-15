use crate::database::{Database, DatabaseError};
use crate::scrapers::imdb::IMDbItem;
use crate::scrapers::{IMDbId, MediaQuality};
use crate::torrent::ProcessableTorrentState;
use sqlx::{FromRow, QueryBuilder};

pub struct TorrentDB<'a> {
    database: &'a Database,
}

impl<'a> TorrentDB<'a> {
    pub fn new(database: &'a Database) -> Self {
        Self { database }
    }

    pub async fn insert(&self, data: Vec<TorrentDBItem>) -> Result<(), DatabaseError> {
        let mut builder = QueryBuilder::new(
            "INSERT INTO torrent(hash, imdb_id, season, episode, size_bytes, state, media_quality) VALUES ",
        );
        let mut separated = builder.separated(", ");
        for item in data {
            separated.push_unseparated("(");
            separated.push_bind(item.hash);
            separated.push_bind(item.imdb_id);
            separated.push_bind(item.season);
            separated.push_bind(item.episode);
            separated.push_bind(item.size_bytes);
            separated.push(item.state);
            separated.push(item.media_quality);
            separated.push_unseparated(")");
        }

        builder
            .build()
            .execute(&self.database.pool)
            .await
            .map(|_| ())
            .map_err(|err| DatabaseError::InsertionError(err.to_string()))
    }

    pub async fn update_torrent_state(
        &self,
        data: Vec<TorrentClientItem<'_>>,
    ) -> Result<(), DatabaseError> {
        match data.len() {
            0 => Err(DatabaseError::UpdateError("No items to update".to_string())),
            1 => {
                let item = data
                    .into_iter()
                    .next()
                    .expect("Missing item when guaranteed");

                QueryBuilder::new("UPDATE torrent SET state = ")
                    .push(item.state)
                    .push(" WHERE torrent.hash = ")
                    .push_bind(item.hash)
                    .build()
                    .execute(&self.database.pool)
                    .await
                    .map(|_| ())
                    .map_err(|err| DatabaseError::UpdateError(err.to_string()))
            }
            _ => {
                let mut builder =
                    QueryBuilder::new("UPDATE torrent SET state = data.state FROM (VALUES ");

                let mut seperated = builder.separated(", ");
                for item in data {
                    seperated.push_unseparated("(");
                    seperated.push_bind(item.hash);
                    seperated.push(item.state);
                    seperated.push_unseparated(")");
                }

                builder.push(") AS data(hash, state) WHERE torrent.hash = data.hash");

                builder
                    .build()
                    .execute(&self.database.pool)
                    .await
                    .map(|_| ())
                    .map_err(|err| DatabaseError::UpdateError(err.to_string()))
            }
        }
    }

    pub async fn delete_torrents(&self, hashes: &[&str]) -> Result<(), DatabaseError> {
        if hashes.is_empty() {
            return Err(DatabaseError::DeleteError("No items to delete".to_string()));
        }

        let mut builder = QueryBuilder::new("DELETE FROM torrent WHERE hash in (");

        let mut separated = builder.separated(", ");
        for hash in hashes {
            separated.push_bind(hash);
        }
        separated.push_unseparated(")");

        builder
            .build()
            .execute(&self.database.pool)
            .await
            .map(|_| ())
            .map_err(|err| DatabaseError::DeleteError(err.to_string()))
    }

    pub async fn get_excluded_file_types(&self) -> Result<Vec<String>, DatabaseError> {
        #[derive(FromRow)]
        struct Row {
            file_type: String,
        }

        QueryBuilder::new(r#"SELECT file_type FROM excluded_files"#)
            .build_query_as::<Row>()
            .fetch_all(&self.database.pool)
            .await
            .map(|row| row.into_iter().map(|row| row.file_type).collect())
            .map_err(|err| DatabaseError::GetError(err.to_string()))
    }

    pub async fn get_all(&self) -> Result<Vec<TorrentDBItem>, DatabaseError> {
        let mut builder = QueryBuilder::new("SELECT * FROM torrent");

        builder
            .build_query_as()
            .fetch_all(&self.database.pool)
            .await
            .map_err(|err| DatabaseError::GetError(err.to_string()))
    }

    pub async fn get_all_with_imdb(&self) -> Result<Vec<TorrentDBItemWithIMDB>, DatabaseError> {
        let mut builder =
            QueryBuilder::new("SELECT * FROM torrent LEFT JOIN imdb ON torrent.imdb_id = imdb.id");

        builder
            .build_query_as()
            .fetch_all(&self.database.pool)
            .await
            .map_err(|err| DatabaseError::GetError(err.to_string()))
    }
}

#[derive(FromRow)]
pub struct TorrentClientItem<'a> {
    pub hash: &'a str,
    pub state: ProcessableTorrentState,
}

#[derive(FromRow)]
pub struct TorrentDBItem {
    pub hash: String,
    pub imdb_id: IMDbId,
    pub season: Option<i64>,
    pub episode: Option<i64>,
    pub size_bytes: i64,
    pub state: ProcessableTorrentState,
    pub media_quality: MediaQuality,
}

#[derive(FromRow)]
pub struct TorrentDBItemWithIMDB {
    #[sqlx(flatten)]
    pub torrent_item: TorrentDBItem,
    #[sqlx(flatten)]
    pub imdb_item: IMDbItem,
}
