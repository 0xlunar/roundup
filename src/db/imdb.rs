use super::DBConnection;
use crate::api::imdb::{IMDBItem, SearchType};
use anyhow::format_err;
use sqlx::{Postgres, QueryBuilder};
use std::ops::Not;

pub struct IMDBDatabase<'a> {
    db: &'a DBConnection,
}

impl<'a> IMDBDatabase<'a> {
    pub fn new(db: &'a DBConnection) -> IMDBDatabase {
        IMDBDatabase { db }
    }

    pub async fn insert_or_update(&self, item: &IMDBItem) -> Result<(), sqlx::Error> {
        let query = "INSERT INTO imdb as i_db(id, title, year, image_url, rating, popularity_rank, release_order, _type, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) ON CONFLICT (id) DO UPDATE SET image_url = $4, popularity_rank = COALESCE($6, i_db.popularity_rank), release_order = COALESCE($7, i_db.release_order), updated_at = $10;";

        let _ = sqlx::query(query)
            .bind(&item.id)
            .bind(&item.title)
            .bind(item.year)
            .bind(&item.image_url)
            .bind(&item.rating)
            .bind(item.popularity_rank)
            .bind(item.release_order)
            .bind(&item._type)
            .bind(item.created_at)
            .bind(item.updated_at)
            .execute(&self.db.db)
            .await?;

        Ok(())
    }

    pub async fn fetch(
        &self,
        search_type: SearchType,
    ) -> anyhow::Result<Vec<IMDBItem>, sqlx::Error> {
        let mut query_builder: QueryBuilder<Postgres> =
            QueryBuilder::new(String::from("SELECT * FROM imdb WHERE "));

        match search_type {
            SearchType::Query(s) => {
                query_builder.push("title SIMILAR TO '");
                query_builder.push_bind(s.to_ascii_lowercase());
                query_builder.push("*'");
            }
            SearchType::MoviePopular => {
                query_builder.push(
                    "_type = 'movie' AND popularity_rank IS NOT NULL ORDER BY popularity_rank ASC",
                );
            }
            SearchType::MovieLatestRelease => {
                query_builder.push(
                    "_type = 'movie' AND release_order IS NOT NULL ORDER BY release_order ASC",
                );
            }
            SearchType::TVPopular => {
                query_builder.push(
                    "_type = 'tvshow' AND popularity_rank IS NOT NULL ORDER BY popularity_rank ASC",
                );
            }
            SearchType::TVLatestRelease => {
                query_builder.push(
                    "_type = 'tvshow' AND release_order IS NOT NULL ORDER BY release_order ASC",
                );
            }
            SearchType::Watchlist => {
                query_builder.push("watchlist = true");
            }
            SearchType::Downloads => unreachable!(),
        };

        let resp = query_builder
            .build_query_as::<IMDBItem>()
            .fetch_all(&self.db.db)
            .await?;

        Ok(resp)
    }

    pub async fn fetch_item_by_id(&self, id: &str) -> anyhow::Result<Vec<IMDBItem>, sqlx::Error> {
        let query = "SELECT * FROM imdb WHERE id = $1";

        let items = sqlx::query_as::<_, IMDBItem>(&query)
            .bind(id)
            .fetch_all(&self.db.db)
            .await?;

        Ok(items)
    }

    pub async fn fetch_watchlist(&self) -> anyhow::Result<Vec<IMDBItem>, sqlx::Error> {
        let query = "SELECT * FROM imdb WHERE watchlist = true";

        let resp = sqlx::query_as::<_, IMDBItem>(&query)
            .fetch_all(&self.db.db)
            .await?;

        Ok(resp)
    }

    pub async fn update_watchlist_item(
        &self,
        id: &str,
        state: bool,
    ) -> anyhow::Result<(), sqlx::Error> {
        let mut query_builder: QueryBuilder<Postgres> =
            QueryBuilder::new(String::from("UPDATE imdb SET watchlist = "));
        query_builder.push_bind(state);
        query_builder.push(" WHERE id = ");
        query_builder.push_bind(id);

        let built = query_builder.build();

        let _ = built.execute(&self.db.db).await?;

        Ok(())
    }

    pub async fn update_metadata(&self, item: &IMDBItem) -> anyhow::Result<()> {
        let mut query_builder: QueryBuilder<Postgres> =
            QueryBuilder::new(String::from("UPDATE imdb SET "));
        let mut is_empty_query = true;

        match item.rating.as_str() {
            "TBD" => (),
            "" => (),
            _ => {
                query_builder.push("rating = ");
                query_builder.push_bind(&item.rating);
                is_empty_query = false;
            }
        }
        if let Some(t) = item.runtime {
            if is_empty_query.not() {
                query_builder.push(", ");
            }
            query_builder.push("runtime = ");
            query_builder.push_bind(t);
            is_empty_query = false;
        };
        match &item.video_thumbnail_url {
            Some(t) => {
                if is_empty_query.not() {
                    query_builder.push(", ");
                }
                query_builder.push("video_thumbnail_url = ");
                query_builder.push_bind(t);
                is_empty_query = false;
            }
            None => (),
        };
        match &item.video_url {
            Some(t) => {
                if is_empty_query.not() {
                    query_builder.push(", ");
                }
                query_builder.push("video_url = ");
                query_builder.push_bind(t);
                is_empty_query = false;
            }
            None => (),
        };
        match &item.plot {
            Some(t) => {
                if is_empty_query.not() {
                    query_builder.push(", ");
                }
                query_builder.push("plot = ");
                query_builder.push_bind(t);
                is_empty_query = false;
            }
            None => (),
        };

        if is_empty_query {
            return Err(format_err!("Empty Update Query"));
        }

        query_builder.push(" WHERE id = ");
        query_builder.push_bind(&item.id);

        let _ = query_builder.build().execute(&self.db.db).await?;

        Ok(())
    }

    pub async fn clear_ranking(&self, ranking_type: &SearchType) -> anyhow::Result<()> {
        let mut query_builder: QueryBuilder<Postgres> =
            QueryBuilder::new(String::from("UPDATE imdb SET "));

        match ranking_type {
            SearchType::MoviePopular => {
                query_builder.push(
                    "popularity_rank = NULL WHERE _type = 'movie' AND popularity_rank IS NOT NULL",
                );
            }
            SearchType::MovieLatestRelease => {
                query_builder.push(
                    "release_order = NULL WHERE _type = 'movie' AND release_order IS NOT NULL",
                );
            }
            SearchType::TVPopular => {
                query_builder.push(
                    "popularity_rank = NULL WHERE _type = 'tvshow' AND popularity_rank IS NOT NULL",
                );
            }
            SearchType::TVLatestRelease => {
                query_builder.push(
                    "release_order = NULL WHERE _type = 'tvshow' AND release_order IS NOT NULL",
                );
            }
            SearchType::Watchlist => (),
            SearchType::Downloads => (),
            SearchType::Query(_) => (),
        };

        query_builder.build().execute(&self.db.db).await?;

        Ok(())
    }

    pub async fn insert_or_update_many(&self, items: &[IMDBItem]) -> Result<(), sqlx::Error> {
        // TODO: FIX WITH ACTUAL PSQL STATEMENT OR SQLX FUNCTION THAT IS BETTER THAN THIS TRASH
        for item in items {
            self.insert_or_update(item).await?;
        }

        Ok(())
    }
}
