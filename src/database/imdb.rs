use crate::database::{Database, DatabaseError};
use crate::scrapers::imdb::{
    IMDbDetailedItem, IMDbItem, IMDbReleaseCalendarItem, IMDbTopMediaItem,
};
use crate::scrapers::{IMDbId, IMDbMediaType};
use sqlx::postgres::PgRow;
use sqlx::{FromRow, QueryBuilder};

pub struct IMDbDB<'a> {
    database: &'a Database,
}

pub enum DatabaseInsertionItem {
    Item(IMDbItem),
    ReleaseItem(IMDbReleaseCalendarItem),
    TopMediaItem(IMDbTopMediaItem),
    FullDetailItem(IMDbDetailedItem),
}

pub enum DatabaseManyInsertionItem {
    ManyItem(Vec<IMDbItem>),
    ManyReleaseItem(Vec<IMDbReleaseCalendarItem>),
    ManyTopMediaItem(Vec<IMDbTopMediaItem>),
    ManyFullDetailItem(Vec<IMDbDetailedItem>),
}

impl<'a> IMDbDB<'a> {
    pub fn new(database: &'a Database) -> Self {
        Self { database }
    }

    pub async fn insert_or_update(&self, item: DatabaseInsertionItem) -> Result<(), DatabaseError> {
        let mut builder = QueryBuilder::new("INSERT INTO imdb (id, title, year, image_url, _type");

        match item {
            DatabaseInsertionItem::Item(item) => {
                builder.push(") VALUES (");

                let mut separated = builder.separated(", ");
                separated.push_bind(item.id);
                separated.push_bind(&item.title);
                separated.push_bind(&item.year);
                separated.push_bind(&item.image_url);
                separated.push_bind(&item._type);
                separated.push_unseparated(
                    ") ON CONFLICT (id) DO UPDATE SET \
                title = EXCLUDED.title,\
                year = EXCLUDED.year,\
                image_url = EXCLUDED.image_url",
                );
            }
            DatabaseInsertionItem::ReleaseItem(item) => {
                builder.push(", release_date) VALUES (");

                let mut separated = builder.separated(", ");
                separated.push_bind(item.item.id);
                separated.push_bind(&item.item.title);
                separated.push_bind(&item.item.year);
                separated.push_bind(&item.item.image_url);
                separated.push_bind(&item.item._type);
                separated.push_bind(&item.release_date);
                separated.push_unseparated(
                    ") ON CONFLICT (id) DO UPDATE SET \
                title = EXCLUDED.title,\
                year = EXCLUDED.year,\
                image_url = EXCLUDED.image_url,\
                release_date = EXCLUDED.release_date",
                );
            }
            DatabaseInsertionItem::TopMediaItem(item) => {
                builder.push(", release_date, ranking) VALUES (");

                let mut separated = builder.separated(", ");
                separated.push_bind(item.item.item.id);
                separated.push_bind(&item.item.item.title);
                separated.push_bind(&item.item.item.year);
                separated.push_bind(&item.item.item.image_url);
                separated.push_bind(&item.item.item._type);
                separated.push_bind(&item.item.release_date);
                separated.push_bind(&item.ranking);
                separated.push_unseparated(
                    ") ON CONFLICT (id) DO UPDATE SET \
                title = EXCLUDED.title,\
                year = EXCLUDED.year,\
                image_url = EXCLUDED.image_url,\
                release_date = EXCLUDED.release_date,\
                ranking = EXCLUDED.ranking",
                );
            }
            DatabaseInsertionItem::FullDetailItem(item) => {
                builder.push(", release_date, plot, runtime_seconds, video_url, seasons) VALUES (");

                let seasons = serde_json::to_string(&item.seasons)
                    .map_err(|err| DatabaseError::AnyhowError(err.into()))?;

                let mut separated = builder.separated(", ");
                separated.push_bind(item.item.id);
                separated.push_bind(&item.item.title);
                separated.push_bind(&item.item.year);
                separated.push_bind(&item.item.image_url);
                separated.push_bind(&item.item._type);
                separated.push_bind(&item.release_date);
                separated.push_bind(&item.plot);
                separated.push_bind(&item.runtime_seconds);
                separated.push_bind(&item.video_url);
                separated.push_bind(&seasons);
                separated.push_unseparated(
                    ") ON CONFLICT (id) DO UPDATE SET \
                title = EXCLUDED.title,\
                year = EXCLUDED.year,\
                image_url = EXCLUDED.image_url,\
                release_date = EXCLUDED.release_date,\
                plot = EXCLUDED.plot,\
                runtime_seconds = EXCLUDED.runtime_seconds,\
                video_url = EXCLUDED.video_url,\
                seasons = EXCLUDED.seasons",
                );
            }
        };

        builder
            .build()
            .execute(&self.database.pool)
            .await
            .map_err(|err| DatabaseError::InsertionError(err.to_string()))?;

        Ok(())
    }

    pub async fn insert_or_update_many(
        &self,
        item: DatabaseManyInsertionItem,
    ) -> Result<(), DatabaseError> {
        let mut builder = QueryBuilder::new("INSERT INTO imdb (id, title, year, image_url, _type");

        match item {
            DatabaseManyInsertionItem::ManyItem(items) => {
                builder.push(") VALUES ");

                for item in items {
                    let mut separated = builder.separated(", ");
                    separated.push_unseparated("(");
                    separated.push_bind(item.id);
                    separated.push_bind(item.title);
                    separated.push_bind(item.year);
                    separated.push_bind(item.image_url);
                    separated.push_bind(item._type);
                    separated.push_unseparated(")");
                }

                builder.push(
                    " ON CONFLICT (id) DO UPDATE SET \
                title = EXCLUDED.title,\
                year = EXCLUDED.year,\
                image_url = EXCLUDED.image_url",
                );
            }
            DatabaseManyInsertionItem::ManyReleaseItem(items) => {
                builder.push(", release_date) VALUES ");

                for item in items {
                    let mut separated = builder.separated(", ");
                    separated.push_unseparated("(");
                    separated.push_bind(item.item.id);
                    separated.push_bind(item.item.title);
                    separated.push_bind(item.item.year);
                    separated.push_bind(item.item.image_url);
                    separated.push_bind(item.item._type);
                    separated.push_bind(item.release_date);
                    separated.push_unseparated(")");
                }
                builder.push(
                    " ON CONFLICT (id) DO UPDATE SET \
                title = EXCLUDED.title,\
                year = EXCLUDED.year,\
                image_url = EXCLUDED.image_url,\
                release_date = EXCLUDED.release_date",
                );
            }
            DatabaseManyInsertionItem::ManyTopMediaItem(items) => {
                builder.push(", release_date, ranking) VALUES ");

                for item in items {
                    let mut separated = builder.separated(", ");
                    separated.push_unseparated("(");
                    separated.push_bind(item.item.item.id);
                    separated.push_bind(&item.item.item.title);
                    separated.push_bind(&item.item.item.year);
                    separated.push_bind(&item.item.item.image_url);
                    separated.push_bind(&item.item.item._type);
                    separated.push_bind(&item.item.release_date);
                    separated.push_bind(&item.ranking);
                    separated.push_unseparated(")");
                }

                builder.push(
                    " ON CONFLICT (id) DO UPDATE SET \
                title = EXCLUDED.title,\
                year = EXCLUDED.year,\
                image_url = EXCLUDED.image_url,\
                release_date = EXCLUDED.release_date,\
                ranking = EXCLUDED.ranking",
                );
            }
            DatabaseManyInsertionItem::ManyFullDetailItem(items) => {
                builder.push(", release_date, plot, runtime_seconds, video_url, seasons) VALUES ");

                for item in items {
                    let seasons = serde_json::to_string(&item.seasons)
                        .map_err(|err| DatabaseError::AnyhowError(err.into()))?;

                    let mut separated = builder.separated(", ");
                    separated.push_unseparated("(");
                    separated.push_bind(item.item.id);
                    separated.push_bind(&item.item.title);
                    separated.push_bind(&item.item.year);
                    separated.push_bind(&item.item.image_url);
                    separated.push_bind(&item.item._type);
                    separated.push_bind(&item.release_date);
                    separated.push_bind(&item.plot);
                    separated.push_bind(&item.runtime_seconds);
                    separated.push_bind(&item.video_url);
                    separated.push_bind(&seasons);
                    separated.push_unseparated(")");
                }

                builder.push(
                    " ON CONFLICT (id) DO UPDATE SET \
                title = EXCLUDED.title,\
                year = EXCLUDED.year,\
                image_url = EXCLUDED.image_url,\
                release_date = EXCLUDED.release_date,\
                plot = EXCLUDED.plot,\
                runtime_seconds = EXCLUDED.runtime_seconds,\
                video_url = EXCLUDED.video_url,\
                seasons = EXCLUDED.seasons",
                );
            }
        };

        builder
            .build()
            .execute(&self.database.pool)
            .await
            .map_err(|err| DatabaseError::InsertionError(err.to_string()))?;

        Ok(())
    }

    pub async fn get_item(&self, id: IMDbId) -> Result<Option<IMDbItem>, DatabaseError> {
        self.get_optional(id).await
    }

    pub async fn get_items(
        &self,
        ids: &[IMDbId],
    ) -> Result<Vec<IMDbItem>, DatabaseError> {
        let mut builder = QueryBuilder::new(r#"SELECT * FROM imdb WHERE id in ("#);

        let mut separated = builder.separated(", ");
        for id in ids {
            separated.push_bind(id);
        }

        builder.push(")");

        builder
            .build_query_as::<IMDbItem>()
            .fetch_all(&self.database.pool)
            .await
            .map_err(|err| DatabaseError::GetError(err.to_string()))
    }

    pub async fn get_full_item(
        &self,
        id: IMDbId,
    ) -> Result<Option<IMDbDetailedItem>, DatabaseError> {
        self.get_optional(id).await
    }

    pub async fn get_release_item(
        &self,
        id: IMDbId,
    ) -> Result<Option<IMDbReleaseCalendarItem>, DatabaseError> {
        self.get_optional(id).await
    }

    pub async fn get_ranked_item(
        &self,
        id: IMDbId,
    ) -> Result<Option<IMDbTopMediaItem>, DatabaseError> {
        self.get_optional(id).await
    }

    pub async fn get_top_ranked(&self) -> Result<Vec<IMDbTopMediaItem>, DatabaseError> {
        let mut builder = QueryBuilder::new(r#"SELECT * FROM imdb WHERE ranking <= 100 LIMIT 100"#);

        builder
            .build_query_as::<IMDbTopMediaItem>()
            .fetch_all(&self.database.pool)
            .await
            .map_err(|err| DatabaseError::GetError(err.to_string()))
    }

    pub async fn get_upcoming_releases(
        &self,
        media_type: IMDbMediaType,
    ) -> Result<Vec<IMDbTopMediaItem>, DatabaseError> {
        let mut builder = QueryBuilder::new(r#"SELECT * FROM imdb WHERE _type = "#);
        builder.push_bind(media_type);
        builder.push(r#" AND release_date < interval '1 year' LIMIT 100"#);

        builder
            .build_query_as::<IMDbTopMediaItem>()
            .fetch_all(&self.database.pool)
            .await
            .map_err(|err| DatabaseError::GetError(err.to_string()))
    }

    async fn get_optional<T: Send + Unpin + for<'r> FromRow<'r, PgRow>>(
        &self,
        id: IMDbId,
    ) -> Result<Option<T>, DatabaseError> {
        let mut builder = QueryBuilder::new(r#"SELECT * FROM imdb WHERE id = "#);
        builder.push_bind(id);
        builder.push(" LIMIT 1");

        builder
            .build_query_as::<T>()
            .fetch_optional(&self.database.pool)
            .await
            .map_err(|err| DatabaseError::GetError(err.to_string()))
    }
}
