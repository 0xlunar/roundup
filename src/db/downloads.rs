use std::ops::Not;

use chrono::Local;
use qbittorrent::data::Hash;
use rayon::prelude::*;
use serde::Serialize;
use sqlx::{Postgres, QueryBuilder, Row};

use crate::api::imdb::{IMDBEpisode, ItemType};
use crate::api::torrent::MediaQuality;
use crate::server::download::TorrentQuery;

use super::DBConnection;

#[derive(sqlx::FromRow, Serialize)]
pub struct ActiveDownloadItem {
    id: i32,
    imdb_id: String,
    season: Option<i32>,
    episode: Option<i32>,
    quality: MediaQuality,
    _type: ItemType,
    magnet_hash: String,
    state: String,
    progress: f64,
    #[serde(skip_serializing)]
    pub created_at: chrono::DateTime<Local>,
    #[serde(skip_serializing)]
    pub updated_at: chrono::DateTime<Local>,
}

#[derive(sqlx::FromRow, Serialize)]
pub struct ActiveDownloadIMDBItem {
    pub imdb_id: String,
    pub season: Option<i32>,
    pub episode: Option<i32>,
    pub quality: String,
    pub _type: ItemType,
    pub state: String,
    pub progress: f64,
    pub title: String,
    pub year: i64,
    pub image_url: String,
    pub rating: String,
    pub runtime: Option<i64>,
}

pub struct DownloadDatabase<'a> {
    db: &'a DBConnection,
}

impl<'a> DownloadDatabase<'a> {
    pub fn new(db: &'a DBConnection) -> DownloadDatabase {
        DownloadDatabase { db }
    }

    pub async fn insert(&self, item: &TorrentQuery) -> Result<(), sqlx::Error> {
        let query = "INSERT INTO active_downloads(imdb_id, season, episode, magnet_hash, quality, _type) VALUES ($1, $2, $3, $4, $5, $6);";

        let _type = match item.episode {
            Some(_) => ItemType::TvShow,
            None => ItemType::Movie,
        };

        let hash = item
            .magnet_uri
            .split_at(20)
            .1
            .split_once('&')
            .unwrap()
            .0
            .to_lowercase();

        sqlx::query(query)
            .bind(&item.imdb_id)
            .bind(item.season)
            .bind(item.episode)
            .bind(hash)
            .bind(item.quality.to_string())
            .bind(_type)
            .execute(&self.db.db)
            .await?;

        Ok(())
    }

    pub async fn is_downloading(
        &self,
        imdb_id: &str,
        episodes: Option<&Vec<IMDBEpisode>>,
    ) -> anyhow::Result<(bool, Option<Vec<IMDBEpisode>>)> {
        let mut query_builder: QueryBuilder<Postgres> = QueryBuilder::new(String::new());

        match episodes {
            None => {
                query_builder.push("SELECT id FROM active_downloads WHERE imdb_id = ");
                query_builder.push_bind(imdb_id);
                let resp = query_builder.build().fetch_optional(&self.db.db).await?;
                match resp {
                    Some(_) => Ok((true, None)),
                    None => Ok((false, None)),
                }
            }
            Some(episodes) => {
                let mut sorted = episodes.to_vec();
                sorted.sort_by(|a, b| {
                    let season_cmp = b.season.cmp(&a.season);
                    if season_cmp.is_eq() {
                        b.episode.cmp(&a.episode)
                    } else {
                        season_cmp
                    }
                });

                let seasons = sorted.chunk_by(|a, b| a.season == b.season);

                let mut downloading_episodes = Vec::new();
                for season in seasons {
                    query_builder.reset();
                    let season_number = season.first().unwrap().season;
                    query_builder
                        .push("SELECT season, episode FROM active_downloads WHERE imdb_id = ");
                    query_builder.push_bind(imdb_id);
                    query_builder.push(" AND season = ");
                    query_builder.push_bind(season_number);

                    query_builder.push(" AND episode in (");
                    let mut episodes = season.iter().map(|e| e.episode).peekable();
                    while let Some(episode) = episodes.next() {
                        query_builder.push_bind(episode);
                        if episodes.peek().is_some() {
                            query_builder.push(", ");
                        }
                    }
                    
                    query_builder.push(" )");
                    let mut resp: Vec<(i32, i32)> = query_builder
                        .build_query_as()
                        .fetch_all(&self.db.db)
                        .await?;
                    downloading_episodes.append(&mut resp);
                }
                let sorted_len = sorted.len();

                let filtered = sorted
                    .into_par_iter()
                    .filter(|e| {
                        downloading_episodes
                            .par_iter()
                            .any(|a| a == &(e.season, e.episode))
                            .not()
                    })
                    .collect::<Vec<IMDBEpisode>>();
                if filtered.is_empty() {
                    Ok((true, None)) // We are downloading everything
                } else if sorted_len == filtered.len() {
                    Ok((false, Some(filtered))) // we aren't downloading any
                } else {
                    Ok((true, Some(filtered))) // We are partially downloading
                }
            }
        }
    }

    pub async fn fetch_downloads_with_imdb_data(
        &self,
    ) -> anyhow::Result<Vec<ActiveDownloadIMDBItem>> {
        let mut query_builder: QueryBuilder<Postgres> = QueryBuilder::new(String::new());
        query_builder.push(
            "SELECT imdb_id, season, episode, quality, active_downloads._type, state, progress, title, year, image_url, rating, runtime FROM active_downloads LEFT JOIN imdb ON active_downloads.imdb_id = imdb.id"
        );
        let resp = query_builder
            .build_query_as::<ActiveDownloadIMDBItem>()
            .fetch_all(&self.db.db)
            .await?;

        Ok(resp)
    }

    pub async fn update(
        &self,
        hash: &str,
        state: &str,
        progress: f64,
    ) -> anyhow::Result<(), sqlx::Error> {
        let mut query_builder: QueryBuilder<Postgres> = QueryBuilder::new(String::new());
        query_builder.push("UPDATE active_downloads SET progress = ");
        query_builder.push_bind(progress);
        query_builder.push(", state = ");
        query_builder.push_bind(state);
        query_builder.push(", updated_at = ");
        query_builder.push_bind(Local::now());
        query_builder.push(" WHERE magnet_hash = ");
        query_builder.push_bind(hash);
        query_builder.build().execute(&self.db.db).await?;

        Ok(())
    }
    pub async fn insert_many(&self, items: &[TorrentQuery]) -> Result<(), sqlx::Error> {
        // TODO: FIX WITH ACTUAL PSQL STATEMENT OR SQLX FUNCTION THAT IS BETTER THAN THIS TRASH
        for item in items.iter() {
            self.insert(item).await?;
        }

        Ok(())
    }

    pub async fn remove_all(&self) -> Result<(), sqlx::Error> {
        let mut query_builder: QueryBuilder<Postgres> =
            QueryBuilder::new(String::from("DELETE FROM active_downloads"));
        query_builder.build().execute(&self.db.db).await?;

        Ok(())
    }

    pub async fn remove_all_finished(&self) -> Result<(), sqlx::Error> {
        let mut query_builder: QueryBuilder<Postgres> = QueryBuilder::new(String::from(
            "DELETE FROM active_downloads WHERE state IN (",
        ));

        query_builder.push_bind("pausedUP");
        query_builder.push(", ");
        query_builder.push_bind("forcedUP");
        query_builder.push(")");
        query_builder.build().execute(&self.db.db).await?;

        Ok(())
    }

    pub async fn remove_manually_removed(
        &self,
        active_hashes: &[&Hash],
    ) -> Result<(), sqlx::Error> {
        let mut query_builder: QueryBuilder<Postgres> = QueryBuilder::new(String::from(
            "DELETE FROM active_downloads WHERE magnet_hash NOT IN (",
        ));

        let len = active_hashes.len();
        active_hashes.iter().enumerate().for_each(|(i, x)| {
            query_builder.push_bind(x.as_str());
            if i < len - 1 {
                query_builder.push(",");
            }
        });
        query_builder.push(")");
        query_builder.build().execute(&self.db.db).await?;
        Ok(())
    }
}
