use sqlx::{Postgres, QueryBuilder};
use crate::api::imdb::{SearchType};
use crate::api::moviedb::MovieDBItem;
use super::DBConnection;

pub struct MovieDBDatabase<'a> {
    db: &'a DBConnection
}

impl<'a> MovieDBDatabase<'a> {
    pub fn new(db: &'a DBConnection) -> MovieDBDatabase {
        MovieDBDatabase {
            db
        }
    }

    pub async fn insert_or_update(&self, item: &MovieDBItem) -> Result<(), sqlx::Error> {
        let query = "INSERT INTO moviedb as m_db(id, imdb_id, title, plot, release_date, image_url, video_id, certification, runtime, popularity_Rank, _type, watchlist, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14) ON CONFLICT (id) DO UPDATE SET image_url = COALESCE($6, m_db.image_url), video_id = COALESCE($7, m_db.video_id), certification = COALESCE($8, m_db.certification), popularity_rank = COALESCE($10, m_db.popularity_rank), updated_at = $14";

        let _ = sqlx::query(query)
            .bind(&item.id)
            .bind(&item.imdb_id)
            .bind(&item.title)
            .bind(&item.plot)
            .bind(&item.release_date)
            .bind(&item.image_url)
            .bind(&item.video_id)
            .bind(&item.certification)
            .bind(&item.runtime)
            .bind(&item.popularity_rank)
            .bind(&item._type)
            .bind(&item.watchlist)
            .bind(&item.created_at)
            .bind(&item.updated_at)
            .execute(&self.db.db)
            .await?;

        Ok(())
    }

    pub async fn fetch(&self, search_type: SearchType) -> anyhow::Result<Vec<MovieDBItem>, sqlx::Error> {
        let mut query_builder: QueryBuilder<Postgres> = QueryBuilder::new(String::from("SELECT * FROM moviedb WHERE "));

        match search_type {
            SearchType::Query(s) => {
                query_builder.push("title SIMILAR TO '");
                query_builder.push_bind(s.to_ascii_lowercase());
                query_builder.push("*'");
            }
            SearchType::MoviePopular => {
                query_builder.push("_type = 'movie' AND popularity_rank IS NOT NULL ORDER BY popularity_rank ASC LIMIT 100");
            }
            SearchType::MovieLatestRelease => {
                query_builder.push("_type = 'movie' AND release_date IS NOT NULL ORDER BY release_date DESC LIMIT 100");
            }
            SearchType::TVPopular => {
                query_builder.push("_type = 'tvshow' AND popularity_rank IS NOT NULL ORDER BY popularity_rank ASC LIMIT 100");
            }
            SearchType::TVLatestRelease => {
                query_builder.push("_type = 'tvshow' AND release_date IS NOT NULL ORDER BY release_date DESC LIMIT 100");
            }
            SearchType::Watchlist => {
                query_builder.push("watchlist = true");
            },
            SearchType::Downloads => unreachable!(),
        };

        let resp = query_builder
            .build_query_as::<MovieDBItem>()
            .fetch_all(&self.db.db)
            .await?;

        Ok(resp)
    }
    
    pub async fn fetch_item_by_id(&self, id: i32) -> anyhow::Result<Vec<MovieDBItem>, sqlx::Error> {
        let query = "SELECT * FROM moviedb WHERE id = $1";

        let items = sqlx::query_as::<_, MovieDBItem>(&query)
            .bind(id)
            .fetch_all(&self.db.db)
            .await?;

        Ok(items)
    }

    pub async fn fetch_watchlist(&self) -> anyhow::Result<Vec<MovieDBItem>, sqlx::Error> {
        let query = "SELECT * FROM moviedb WHERE watchlist = true";

        let resp = sqlx::query_as::<_, MovieDBItem>(&query)
            .fetch_all(&self.db.db)
            .await?;

        Ok(resp)
    }

    pub async fn update_watchlist_item(&self, id: i32, state: bool) -> anyhow::Result<(), sqlx::Error> {
        let mut query_builder: QueryBuilder<Postgres> = QueryBuilder::new(String::from("UPDATE moviedb SET watchlist = "));
        query_builder.push_bind(state);
        query_builder.push(" WHERE id = ");
        query_builder.push_bind(id);

        let built = query_builder
            .build();

        let _ = built
            .execute(&self.db.db)
            .await?;

        Ok(())
    }

    // pub async fn update_metadata(&self, item: &IMDBItem) -> anyhow::Result<()> {
    //     let mut query_builder: QueryBuilder<Postgres> = QueryBuilder::new(String::from("UPDATE moviedb SET "));
    //     let mut is_empty_query = true;
    //
    //     match item.rating.as_str() {
    //         "TBD" => (),
    //         "" => (),
    //         _ => {
    //             query_builder.push("rating = ");
    //             query_builder.push_bind(&item.rating);
    //             is_empty_query = false;
    //         },
    //     }
    //     match item.runtime {
    //         Some(t) => {
    //             if is_empty_query.not() {
    //                 query_builder.push(", ");
    //             }
    //             query_builder.push("runtime = ");
    //             query_builder.push_bind(t);
    //             is_empty_query = false;
    //         },
    //         None => ()
    //     };
    //     match &item.video_thumbnail_url {
    //         Some(t) => {
    //             if is_empty_query.not() {
    //                 query_builder.push(", ");
    //             }
    //             query_builder.push("video_thumbnail_url = ");
    //             query_builder.push_bind(t);
    //             is_empty_query = false;
    //         },
    //         None => ()
    //     };
    //     match &item.video_url {
    //         Some(t) => {
    //             if is_empty_query.not() {
    //                 query_builder.push(", ");
    //             }
    //             query_builder.push("video_url = ");
    //             query_builder.push_bind(t);
    //             is_empty_query = false;
    //         },
    //         None => ()
    //     };
    //     match &item.plot {
    //         Some(t) => {
    //             if is_empty_query.not() {
    //                 query_builder.push(", ");
    //             }
    //             query_builder.push("plot = ");
    //             query_builder.push_bind(t);
    //             is_empty_query = false;
    //         },
    //         None => ()
    //     };
    //
    //     if is_empty_query {
    //         return Err(format_err!("Empty Update Query"));
    //     }
    //
    //     query_builder.push(" WHERE id = ");
    //     query_builder.push_bind(&item.id);
    //
    //     let _ = query_builder.build().execute(&self.db.db).await?;
    //
    //     Ok(())
    // }

    pub async fn insert_or_update_many(&self, items: &[MovieDBItem]) -> Result<(), sqlx::Error> {
        // TODO: FIX WITH ACTUAL PSQL STATEMENT OR SQLX FUNCTION THAT IS BETTER THAN THIS TRASH
        let mut iter = items.iter();

        for item in iter {
            self.insert_or_update(item).await?;
        }

        Ok(())
    }
}